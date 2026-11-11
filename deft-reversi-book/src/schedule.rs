//! Transient priority indexes; persisted ordinary queues remain authoritative.
//!
//! 優先順位は「次にどの探索をするか」だけを変える。登録してよいかの判定はしない。
//! 元のfresh/deepenから作る補助索引を持ち、局面値の変更で順位を計算し直す。
//! generate::GenerationState::popが16回に1回は通常順へ戻し、偏りを抑える。
use super::generate::{Context, GenerationState};
use super::*;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SchedulePolicy {
    #[default]
    Breadth,
    Frontier,
}
// Smaller means more urgent: promising incoming/frontier loss, larger backup
// disagreement, then weaker frontier depth/selectivity. Distance breaks ties.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Rank(u16, Reverse<u16>, u8, u8);
#[derive(Clone, Debug, Default)]
pub(super) struct PriorityIndex {
    jobs: BTreeMap<Context, BTreeSet<(usize, u8)>>,
    ranks: BTreeMap<Context, Rank>,
    fresh: BTreeSet<(Rank, u8, Context)>,
    deepen: BTreeSet<(Rank, usize, u8, Context)>,
    dirty: BTreeSet<Context>,
}
impl PriorityIndex {
    pub fn insert(&mut self, c: Context, d: u8, stage: usize) {
        if self.jobs.entry(c).or_default().insert((stage, d)) {
            let rank = *self.ranks.entry(c).or_default();
            self.insert_rank(c, d, stage, rank);
        }
        self.dirty.insert(c);
    }
    fn insert_rank(&mut self, c: Context, d: u8, stage: usize, rank: Rank) {
        if stage == 0 {
            self.fresh.insert((rank, d, c));
        } else {
            self.deepen.insert((rank, stage, d, c));
        }
    }
    fn remove_rank(&mut self, c: Context, d: u8, stage: usize, rank: Rank) {
        if stage == 0 {
            self.fresh.remove(&(rank, d, c));
        } else {
            self.deepen.remove(&(rank, stage, d, c));
        }
    }
    pub fn remove(&mut self, c: Context, d: u8, stage: usize) {
        if let Some(&rank) = self.ranks.get(&c) {
            self.remove_rank(c, d, stage, rank);
        }
        if let Some(jobs) = self.jobs.get_mut(&c) {
            jobs.remove(&(stage, d));
            if jobs.is_empty() {
                self.jobs.remove(&c);
                self.ranks.remove(&c);
            }
        }
    }
    pub fn invalidate(&mut self, c: Context) {
        self.dirty.insert(c);
    }
    fn refresh(&mut self, editor: &BookEditor, state: &GenerationState) {
        let mut dirty = std::mem::take(&mut self.dirty);
        // A parent's changed scores also change each child's incoming loss.
        for c in dirty.clone() {
            for e in editor.nodes[&c.key].edges.iter().filter(|e| e.registered) {
                let child = Context {
                    key: e.child,
                    side: c.side.other(),
                };
                if self.jobs.contains_key(&child) {
                    dirty.insert(child);
                }
            }
        }
        for c in dirty {
            let Some(jobs) = self.jobs.get(&c) else {
                continue;
            };
            let rank = priority(editor, state, c);
            let old = self.ranks[&c];
            if old == rank {
                continue;
            }
            let jobs: Vec<_> = jobs.iter().copied().collect();
            for (s, d) in jobs {
                self.remove_rank(c, d, s, old);
                self.insert_rank(c, d, s, rank);
            }
            self.ranks.insert(c, rank);
        }
    }
    fn choose(&self, fresh: bool) -> Option<(Context, u8, usize)> {
        if fresh {
            self.fresh.first().map(|&(_, d, c)| (c, d, 0))
        } else {
            self.deepen.first().map(|&(_, s, d, c)| (c, d, s))
        }
    }
}
impl GenerationState {
    pub(super) fn priority_job(
        &mut self,
        editor: &BookEditor,
        prefer_new: bool,
    ) -> Option<(Context, u8, usize)> {
        let mut index = self.priority.take().unwrap_or_else(|| {
            let mut index = PriorityIndex::default();
            for &(d, c) in &self.fresh {
                index.insert(c, d, 0);
            }
            for &(s, d, c) in &self.deepen {
                index.insert(c, d, s);
            }
            index
        });
        index.refresh(editor, self);
        let chosen = index
            .choose(prefer_new)
            .or_else(|| index.choose(!prefer_new));
        self.priority = Some(index);
        chosen
    }
}
fn priority(editor: &BookEditor, state: &GenerationState, c: Context) -> Rank {
    let node = &editor.nodes[&c.key];
    let mut incoming = 0;
    if state.roots.binary_search(&c).is_err() && !state.config.frontier_only {
        incoming = 128;
        if let Some(parents) = editor.parents.get(&c.key) {
            for &p in parents {
                let parent = Context {
                    key: p,
                    side: c.side.other(),
                };
                if !state.distances.contains_key(&parent) {
                    continue;
                }
                for e in editor.nodes[&p].edges.iter().filter(|e| e.registered) {
                    if (Context {
                        key: e.child,
                        side: parent.side.other(),
                    }) == c
                    {
                        incoming = incoming
                            .min(super::generate::local_loss(editor, p, e.action).unwrap_or(0));
                    }
                }
            }
        }
    }
    let mut gap = 0;
    let mut frontier = None;
    let mut unknown = false;
    let mut depth = 60;
    let mut selectivity = Selectivity::Full as u8;
    let mut best = None;
    for edge in &node.edges {
        let ev = editor.evidence(c.key, edge);
        if let (Some(d), Some(p)) = (ev.direct, ev.propagated)
            && d.bound() == Bound::Exact
            && p.bound() == Bound::Exact
        {
            gap = gap.max((d.score() - p.score()).unsigned_abs());
        }
        let value = ev.effective.filter(|v| v.bound() == Bound::Exact);
        if let Some(v) = value {
            best = Some(best.map_or(v.score(), |b: i16| b.max(v.score())));
        }
        if !edge.registered {
            if let Some(v) = value {
                frontier = Some(frontier.map_or(v.score(), |b: i16| b.max(v.score())));
                depth = depth.min(v.quality().depth);
                selectivity = selectivity.min(v.quality().selectivity as u8);
            } else {
                unknown = true;
            }
        }
    }
    let loss = if unknown {
        depth = 0;
        selectivity = 0;
        0
    } else {
        match (best, frontier) {
            (Some(b), Some(f)) => (b - f).max(0) as u16,
            _ => 128,
        }
    };
    Rank(incoming + loss, Reverse(gap), depth, selectivity)
}

#[cfg(test)]
mod tests {
    use super::super::generate::{BookSide, GenerationRoot};
    use super::*;
    fn boards() -> (Board, Board) {
        let a = Board::new();
        let b = a.play(legal_actions(a)[0]).unwrap();
        (a, b)
    }
    fn fill(ed: &mut BookEditor, b: Board, depth: u8) {
        for action in legal_actions(b) {
            ed.record_candidate(
                b,
                action,
                Assessment::import(ImportedValue {
                    score: 0,
                    quality: AnalysisQuality {
                        depth,
                        selectivity: Selectivity::Full,
                        solved: false,
                    },
                    bound: Bound::Exact,
                    origin: ValueOrigin::Search,
                    identity: [0; 32],
                })
                .unwrap(),
            )
            .unwrap();
        }
    }
    fn configured(ed: &mut BookEditor, roots: &[Board]) {
        let roots: Vec<_> = roots
            .iter()
            .map(|&board| GenerationRoot {
                board,
                side: BookSide::Black,
            })
            .collect();
        ed.configure_generation_roots(
            &roots,
            GenerationConfig {
                schedule: SchedulePolicy::Frontier,
                max_plies: 0,
                ..Default::default()
            },
        )
        .unwrap();
    }
    #[test]
    fn unknown_frontier_is_prioritized_and_quality_changes_update_the_index() {
        let (a, b) = boards();
        let mut ed = BookEditor::new([0; 32]);
        fill(&mut ed, b, 2);
        configured(&mut ed, &[a, b]);
        let mut state = ed.generation.take().unwrap();
        let first = state.priority_job(&ed, true).unwrap();
        assert_eq!(first.0.key, Key::new(a).0);
        fill(&mut ed, a, 20);
        state.invalidate(Key::new(a).0);
        let next = state.pop(&ed).unwrap();
        assert_eq!(next.0.key, Key::new(b).0);
        assert_eq!(state.pop(&ed).unwrap().0.key, Key::new(a).0);
        assert!(state.pop(&ed).is_none());
    }
    #[test]
    fn weights_and_periodic_breadth_slot_remain_effective() {
        let (a, b) = boards();
        let mut ed = BookEditor::new([0; 32]);
        configured(&mut ed, &[a, b]);
        let mut state = ed.generation.take().unwrap();
        let first = *state.fresh.first().unwrap();
        let second = *state.fresh.last().unwrap();
        state.fresh.remove(&first);
        state.deepen.insert((1, first.0, first.1));
        state.iteration = 3; // default 3 new : 1 deepen
        assert_eq!(state.pop(&ed).unwrap(), (first.1, first.0, 1));
        assert_eq!(state.pop(&ed).unwrap(), (second.1, second.0, 0));
        configured(&mut ed, &[a, b]);
        let mut state = ed.generation.take().unwrap();
        let shallow_key = state.fresh.first().unwrap().1.key;
        fill(&mut ed, shallow_key.board(), 20); // priority would choose the other root
        state.iteration = 15;
        assert_eq!(state.pop(&ed).unwrap().0.key, shallow_key);
    }
    #[test]
    fn saving_discards_only_the_index_and_keeps_the_next_jobs_identical() {
        let (a, b) = boards();
        let mut ed = BookEditor::new([0; 32]);
        fill(&mut ed, b, 2);
        configured(&mut ed, &[a, b]);
        let mut state = ed.generation.take().unwrap();
        state.priority_job(&ed, true); // populate a cache that is not serialized
        ed.generation = Some(state);
        fill(&mut ed, a, 20); // dirty the live cache before saving
        let dir = std::env::temp_dir().join(format!("deft-schedule-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("work");
        ed.save(&path).unwrap();
        let mut loaded = BookEditor::load(&path, ReadLimits::default()).unwrap();
        assert_eq!(
            loaded.generation_config().unwrap().schedule,
            SchedulePolicy::Frontier
        );
        let mut live = ed.generation.take().unwrap();
        let mut restored = loaded.generation.take().unwrap();
        assert!(restored.priority.is_none());
        loop {
            let a = live.pop(&ed);
            let b = restored.pop(&loaded);
            assert_eq!(a, b);
            if a.is_none() {
                break;
            }
            live.iteration += 1;
            restored.iteration += 1;
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn parent_score_changes_reorder_promising_incoming_branches() {
        let (_, b) = boards();
        let mut ed = BookEditor::new([0; 32]);
        let aa = legal_actions(b);
        assert!(aa.len() > 1);
        for (i, &action) in aa.iter().enumerate() {
            let v = Assessment::import(ImportedValue {
                score: if i == 0 { -3 } else { 0 },
                quality: AnalysisQuality {
                    depth: 20,
                    selectivity: Selectivity::Full,
                    solved: false,
                },
                bound: Bound::Exact,
                origin: ValueOrigin::Search,
                identity: [0; 32],
            })
            .unwrap();
            ed.record_candidate(b, action, v).unwrap();
            ed.register_move(b, action).unwrap();
        }
        ed.configure_generation(
            &[b],
            GenerationConfig {
                schedule: SchedulePolicy::Frontier,
                max_plies: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let mut state = ed.generation.take().unwrap();
        let bad = Context {
            key: Key::new(b.play(aa[0]).unwrap()).0,
            side: BookSide::White,
        };
        assert_ne!(state.priority_job(&ed, true).unwrap().0, bad);
        let mut v = ed
            .candidate_evidence(b, aa[0])
            .unwrap()
            .direct
            .unwrap()
            .as_imported();
        v.score = 5;
        ed.record_imported(b, aa[0], v).unwrap();
        state.invalidate(Key::new(b).0);
        assert_eq!(state.pop(&ed).unwrap().0, bad);
    }
    #[test]
    fn backup_disagreement_prioritizes_reanalysis_without_replacing_evidence() {
        let (a, b) = boards();
        let mut ed = BookEditor::new([0; 32]);
        fill(&mut ed, a, 12);
        fill(&mut ed, b, 12);
        ed.register_move(a, legal_actions(a)[0]).unwrap();
        // Other root has known frontier with same depth, but no disagreement.
        let c = b.play(legal_actions(b)[0]).unwrap();
        fill(&mut ed, c, 12);
        configured(&mut ed, &[a, c]);
        let mut state = ed.generation.take().unwrap();
        // Change one candidate's direct value while its propagated score is 0.
        let action = legal_actions(a)[0];
        let mut direct = ed
            .candidate_evidence(a, action)
            .unwrap()
            .direct
            .unwrap()
            .as_imported();
        direct.score = -10;
        direct.quality.depth = 20;
        ed.record_imported(a, action, direct).unwrap();
        assert_eq!(state.pop(&ed).unwrap().0.key, Key::new(a).0);
        assert_eq!(
            ed.candidate_evidence(a, action).unwrap().direct,
            Some(Assessment::import(direct).unwrap())
        );
    }
}
