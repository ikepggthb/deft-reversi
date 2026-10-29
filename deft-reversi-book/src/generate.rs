use super::editor::{Pending, dominates};
use super::*;
use deft_reversi_engine::{SearchMode, SearchRequest, StopHandle, StopReason};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Instant,
};
/// Absolute side is generation context, never part of the relative-board key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum BookSide {
    Black,
    White,
}
impl BookSide {
    pub(super) fn other(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct GenerationRoot {
    pub board: Board,
    pub side: BookSide,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Context {
    pub key: Key,
    pub side: BookSide,
}
impl Context {
    fn child(self, action: Action) -> Self {
        Self {
            key: Key::new(self.key.board().play(action).unwrap()).0,
            side: self.side.other(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    pub depth: u8,
    pub selectivity: Selectivity,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationConfig {
    pub reuse_completed: bool,
    pub stages: Vec<Stage>,
    pub expansion_loss: u8,
    pub black_loss: Option<u8>,
    pub white_loss: Option<u8>,
    pub new_job_weight: u8,
    pub deepen_job_weight: u8,
    pub max_plies: u8,
    pub max_memory_bytes: u64,
    pub checkpoint_interval: Duration,
}
impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            reuse_completed: true,
            stages: vec![
                Stage {
                    depth: 12,
                    selectivity: Selectivity::Percent95,
                },
                Stage {
                    depth: 16,
                    selectivity: Selectivity::Percent95,
                },
                Stage {
                    depth: 20,
                    selectivity: Selectivity::Percent95,
                },
            ],
            expansion_loss: 2,
            black_loss: None,
            white_loss: None,
            new_job_weight: 3,
            deepen_job_weight: 1,
            max_plies: 24,
            max_memory_bytes: 512 * 1024 * 1024,
            checkpoint_interval: Duration::from_secs(60),
        }
    }
}
impl GenerationConfig {
    fn loss(&self, side: BookSide) -> u8 {
        match side {
            BookSide::Black => self.black_loss,
            BookSide::White => self.white_loss,
        }
        .unwrap_or(self.expansion_loss)
    }
    pub fn validate(&self) -> Result<()> {
        if self.stages.is_empty()
            || self.stages.len() > 60
            || self.stages.iter().any(|s| s.depth < 2 || s.depth > 60)
            || self.stages.windows(2).any(|s| {
                s[0].depth >= s[1].depth || (s[0].selectivity as u8) > (s[1].selectivity as u8)
            })
        {
            return Err(invalid(
                "stages must increase in depth and not decrease in selectivity",
            ));
        }
        if self.black_loss.is_some_and(|n| n > 128)
            || self.white_loss.is_some_and(|n| n > 128)
            || u16::from(self.new_job_weight) + u16::from(self.deepen_job_weight) == 0
            || self.expansion_loss > 128
            || self.max_plies > 60
            || self.max_memory_bytes < 16384
            || self.checkpoint_interval.is_zero()
        {
            return Err(invalid("generation settings"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct GenerationLimits {
    pub time: Option<Duration>,
    pub nodes: Option<u64>,
    pub new_positions: Option<u64>,
}
impl GenerationLimits {
    fn validate(self) -> Result<()> {
        if self.time.is_none() && self.nodes.is_none() && self.new_positions.is_none() {
            return Err(invalid("explicit generation budget required"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationStop {
    Exhausted,
    Requested,
    TimeLimit,
    NodeLimit,
    PositionLimit,
    MemoryLimit,
}
#[derive(Clone, Debug)]
pub struct GenerationReport {
    pub reason: GenerationStop,
    pub nodes: u64,
    pub new_positions: u64,
    pub completed_stages: u64,
    pub elapsed: Duration,
}
#[derive(Clone, Debug)]
pub(super) struct GenerationState {
    pub config: GenerationConfig,
    pub roots: Vec<Context>,
    pub iteration: u64,
    pub expansions: BTreeSet<(u8, Context)>,
    pub expanded: BTreeMap<Context, (u64, u8)>,
    pub distances: BTreeMap<Context, u8>,
    pub fresh: BTreeSet<(u8, Context)>,
    pub deepen: BTreeSet<(usize, u8, Context)>,
}
impl GenerationState {
    pub(super) fn invalidate(&mut self, key: Key) {
        for side in [BookSide::Black, BookSide::White] {
            let c = Context { key, side };
            if let Some(&d) = self.distances.get(&c) {
                self.expansions.insert((d, c));
            }
        }
    }
    fn schedule(&mut self, key: Context, stages: usize, distance: u8) {
        if stages < self.config.stages.len() {
            self.enqueue(key, stages, distance);
        } else {
            self.expansions.insert((distance, key));
        }
    }
    pub fn estimated_bytes(&self) -> u64 {
        (self.expanded.len() as u64
            + self.expansions.len() as u64
            + self.distances.len() as u64
            + self.fresh.len() as u64
            + self.deepen.len() as u64)
            * 128
            + self.roots.capacity() as u64 * std::mem::size_of::<Context>() as u64
    }
    fn enqueue(&mut self, key: Context, stages: usize, distance: u8) {
        if stages == 0 {
            self.fresh.insert((distance, key));
        } else if stages < self.config.stages.len() {
            self.deepen.insert((stages, distance, key));
        }
    }
    fn pop(&mut self) -> Option<(Context, u8, usize)> {
        let total =
            u64::from(self.config.new_job_weight) + u64::from(self.config.deepen_job_weight);
        let prefer_new = self.iteration % total < u64::from(self.config.new_job_weight);
        if prefer_new && let Some((d, k)) = self.fresh.pop_first() {
            return Some((k, d, 0));
        }
        if let Some((s, d, k)) = self.deepen.pop_first() {
            return Some((k, d, s));
        }
        self.fresh.pop_first().map(|(d, k)| (k, d, 0))
    }
}
impl BookEditor {
    /// Replace the scheduling configuration. Completed compatible analyses are retained;
    /// stage publication restarts, and the serialized old work queue is replaced.
    pub fn configure_generation(
        &mut self,
        roots: &[Board],
        config: GenerationConfig,
    ) -> Result<()> {
        let roots: Vec<_> = roots
            .iter()
            .map(|&board| GenerationRoot {
                board,
                side: BookSide::Black,
            })
            .collect();
        self.configure_generation_roots(&roots, config)
    }
    pub fn configure_generation_roots(
        &mut self,
        roots: &[GenerationRoot],
        config: GenerationConfig,
    ) -> Result<()> {
        config.validate()?;
        if roots.is_empty() {
            return Err(invalid("at least one starting position required"));
        }
        let mut keys: Vec<_> = roots
            .iter()
            .map(|r| Context {
                key: Key::new(r.board).0,
                side: r.side,
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        // Count before inserting roots so a small memory budget cannot be bypassed.
        if self
            .estimated_bytes()
            .saturating_add(keys.len() as u64 * 16384)
            > config.max_memory_bytes
        {
            return Err(BookError::Limit("generation memory"));
        }
        if keys.iter().any(|c| c.key.board().empty_count() > 60) {
            return Err(invalid("starting position has fewer than four stones"));
        }
        for &k in &keys {
            self.ensure(k.key)?;
        }
        for n in self.nodes.values_mut() {
            n.completed_stages = 0;
            n.pending = None;
        }
        let mut state = GenerationState {
            config,
            roots: keys.clone(),
            iteration: 0,
            expansions: BTreeSet::new(),
            expanded: BTreeMap::new(),
            distances: BTreeMap::new(),
            fresh: BTreeSet::new(),
            deepen: BTreeSet::new(),
        };
        let mut todo: VecDeque<_> = keys.into_iter().map(|k| (k, 0)).collect();
        while let Some((k, d)) = todo.pop_front() {
            if state.distances.get(&k).is_some_and(|old| *old <= d) {
                continue;
            }
            state.distances.insert(k, d);
            if !k.key.board().is_terminal() {
                state.enqueue(k, 0, d);
            }
            for e in self.nodes[&k.key].edges.iter().filter(|e| e.registered) {
                let next = d + u8::from(matches!(e.action, Action::Place(_)));
                if next <= state.config.max_plies {
                    todo.push_back((k.child(e.action), next));
                }
            }
        }
        self.extensions.clear();
        self.generation = Some(state);
        Ok(())
    }
    pub fn generation_config(&self) -> Option<&GenerationConfig> {
        self.generation.as_ref().map(|g| &g.config)
    }
    pub fn generate(
        &mut self,
        engine: &mut Engine,
        limits: GenerationLimits,
        stop: StopHandle,
    ) -> Result<GenerationReport> {
        self.generate_with_checkpoint(engine, limits, stop, |_, _| Ok(()))
    }
    /// Checkpoint callback runs between searches and at every normal return.
    /// It sees a consistent editor; in-progress candidate analyses are persisted separately.
    pub fn generate_with_checkpoint(
        &mut self,
        engine: &mut Engine,
        limits: GenerationLimits,
        stop: StopHandle,
        mut checkpoint: impl FnMut(&BookEditor, &GenerationReport) -> Result<()>,
    ) -> Result<GenerationReport> {
        limits.validate()?;
        if analysis_identity(engine)? != self.identity {
            return Err(invalid(
                "engine/model/calibration does not match workspace; create a new workspace for a new identity",
            ));
        }
        let mut state = self
            .generation
            .take()
            .ok_or_else(|| invalid("configure generation first"))?;
        let started = Instant::now();
        let mut last_checkpoint = started;
        let mut report = GenerationReport {
            reason: GenerationStop::Exhausted,
            nodes: 0,
            new_positions: 0,
            completed_stages: 0,
            elapsed: Duration::ZERO,
        };
        // State is restored on errors as well as success. A panic cannot publish a stage.
        let mut active_job = None;
        let mut active_expansion = None;
        let outcome = (|| -> Result<()> {
            loop {
                if let Some(reason) = budget_reason(limits, &report, started, &stop) {
                    report.reason = reason;
                    break;
                }
                if self
                    .estimated_bytes()
                    .saturating_add(state.estimated_bytes())
                    .saturating_add(16384)
                    > state.config.max_memory_bytes
                {
                    report.reason = GenerationStop::MemoryLimit;
                    break;
                }
                if let Some((distance, key)) = state.expansions.pop_first() {
                    active_expansion = Some((distance, key));
                    if state.distances.get(&key) == Some(&distance) {
                        if let Some(actual_distance) = active_distance(self, &state, key) {
                            expand(self, &mut state, key, actual_distance, limits, &mut report)?;
                        }
                        if report.reason != GenerationStop::Exhausted {
                            state.expansions.insert((distance, key));
                            break;
                        }
                    }
                    active_expansion = None;
                    continue;
                }
                let Some((context, distance, stage_index)) = state.pop() else {
                    break;
                };
                let key = context.key;
                if state.distances.get(&context) != Some(&distance) {
                    continue;
                }
                if self.nodes[&key].completed_stages != stage_index {
                    state.schedule(context, self.nodes[&key].completed_stages, distance);
                    continue;
                }
                if key.board().is_terminal() || active_distance(self, &state, context).is_none() {
                    continue;
                }
                state.iteration = state.iteration.wrapping_add(1);
                active_job = Some((context, distance, stage_index));
                let target = state.config.stages[stage_index];
                if self.nodes[&key].pending.as_ref().is_none_or(|p| {
                    p.stage != stage_index || p.revision != self.nodes[&key].revision
                }) {
                    let n = self.nodes.get_mut(&key).unwrap();
                    // Reuse analyses at least as strong, never downgrade completed work.
                    let values = n
                        .edges
                        .iter()
                        .map(|e| {
                            e.direct.filter(|v| {
                                state.config.reuse_completed
                                    && v.eligible(self.identity, target.depth, target.selectivity)
                            })
                        })
                        .collect();
                    n.pending = Some(Pending {
                        stage: stage_index,
                        revision: n.revision,
                        values,
                    });
                }
                let mut interrupted = false;
                for i in 0..self.nodes[&key].edges.len() {
                    if self.nodes[&key].pending.as_ref().unwrap().values[i].is_some() {
                        continue;
                    }
                    if let Some(reason) = budget_reason(limits, &report, started, &stop) {
                        report.reason = reason;
                        interrupted = true;
                        break;
                    }
                    let action = self.nodes[&key].edges[i].action;
                    let child = key.board().play(action).unwrap();
                    // Depth and selectivity form a partial order. When settings
                    // change, deepen at the stronger of both settings instead of
                    // completing a stage whose incomparable result is discarded.
                    let previous = self.nodes[&key].edges[i]
                        .direct
                        .filter(|v| !v.quality.solved);
                    let parent_depth =
                        previous.map_or(target.depth, |v| target.depth.max(v.quality.depth));
                    let selectivity = previous.map_or(target.selectivity, |v| {
                        if (v.quality.selectivity as u8) > (target.selectivity as u8) {
                            v.quality.selectivity
                        } else {
                            target.selectivity
                        }
                    });
                    let depth = parent_depth - u8::from(matches!(action, Action::Place(_)));
                    let value = if child.is_terminal() {
                        terminal_value(child).child_to_parent(matches!(action, Action::Place(_)))
                    } else {
                        let exact = depth >= child.empty_count();
                        let mut request = SearchRequest::new(
                            child,
                            if exact {
                                SearchMode::Exact
                            } else {
                                SearchMode::Depth(depth)
                            },
                        );
                        request.selectivity = if exact {
                            Selectivity::Full
                        } else {
                            selectivity
                        };
                        request.stop = stop.clone();
                        request.node_limit = limits.nodes.map(|n| n.saturating_sub(report.nodes));
                        request.time_limit =
                            limits.time.map(|t| t.saturating_sub(started.elapsed()));
                        let result = engine.search(request)?;
                        report.nodes = report.nodes.saturating_add(result.stats.nodes);
                        if result.stop_reason != StopReason::Completed
                            || result.bound != Some(Bound::Exact)
                            || result.score.is_none()
                            || result.completed_depth < depth.min(child.empty_count())
                        {
                            report.reason = match result.stop_reason {
                                StopReason::Requested => GenerationStop::Requested,
                                StopReason::TimeLimit => GenerationStop::TimeLimit,
                                StopReason::NodeLimit => GenerationStop::NodeLimit,
                                _ => {
                                    return Err(invalid(
                                        "analysis did not complete the requested stage",
                                    ));
                                }
                            };
                            interrupted = true;
                            break;
                        }
                        Assessment {
                            score: result.score.unwrap() as i16,
                            quality: AnalysisQuality {
                                depth: result.completed_depth,
                                selectivity: result
                                    .selectivity
                                    .ok_or_else(|| invalid("untracked sigma"))?,
                                solved: result.mode == SearchMode::Exact && !result.selective,
                            },
                            bound: Bound::Exact,
                            identity: self.identity,
                            origin: ValueOrigin::Search,
                        }
                        .child_to_parent(matches!(action, Action::Place(_)))
                    };
                    value.validate()?;
                    self.nodes
                        .get_mut(&key)
                        .unwrap()
                        .pending
                        .as_mut()
                        .unwrap()
                        .values[i] = Some(value);
                    if last_checkpoint.elapsed() >= state.config.checkpoint_interval {
                        report.elapsed = started.elapsed();
                        state.enqueue(context, stage_index, distance);
                        let empty = GenerationState {
                            config: state.config.clone(),
                            roots: Vec::new(),
                            iteration: 0,
                            distances: BTreeMap::new(),
                            fresh: BTreeSet::new(),
                            deepen: BTreeSet::new(),
                            expansions: BTreeSet::new(),
                            expanded: BTreeMap::new(),
                        };
                        self.generation = Some(std::mem::replace(&mut state, empty));
                        let saved = checkpoint(self, &report);
                        state = self.generation.take().unwrap();
                        saved?;
                        last_checkpoint = Instant::now();
                    }
                }
                if interrupted {
                    state.enqueue(context, stage_index, distance);
                    break;
                }
                let values = self.nodes[&key].pending.as_ref().unwrap().values.clone();
                self.revision = self
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("revision overflow"))?;
                let node = self.nodes.get_mut(&key).unwrap();
                for (e, v) in node.edges.iter_mut().zip(values) {
                    let v = v.unwrap();
                    if e.direct.is_none_or(|d| dominates(v, d)) {
                        e.direct = Some(v);
                    }
                }
                node.pending = None;
                node.completed_stages += 1;
                node.revision = self.revision;
                self.extensions.clear();
                report.completed_stages += 1;
                active_job = None;
                let affected = self.refresh_from(key);
                for k in affected {
                    state.invalidate(k);
                }
                state.enqueue(context, stage_index + 1, distance);
                state.expansions.insert((distance, context));
            }
            Ok(())
        })();
        if outcome.is_err() {
            if let Some((key, distance, stage)) = active_job {
                state.enqueue(key, stage, distance);
            }
            if let Some(job) = active_expansion {
                state.expansions.insert(job);
            }
        }
        self.generation = Some(state);
        report.elapsed = started.elapsed();
        outcome?;
        checkpoint(self, &report)?;
        Ok(report)
    }
}
fn budget_reason(
    l: GenerationLimits,
    r: &GenerationReport,
    start: Instant,
    stop: &StopHandle,
) -> Option<GenerationStop> {
    if stop.is_stopped() {
        Some(GenerationStop::Requested)
    } else if l.time.is_some_and(|t| start.elapsed() >= t) {
        Some(GenerationStop::TimeLimit)
    } else if l.nodes.is_some_and(|n| r.nodes >= n) {
        Some(GenerationStop::NodeLimit)
    } else {
        None
    }
}
fn expand(
    editor: &mut BookEditor,
    state: &mut GenerationState,
    context: Context,
    distance: u8,
    limits: GenerationLimits,
    report: &mut GenerationReport,
) -> Result<()> {
    let key = context.key;
    let mask = eligible_mask(editor, key, state.config.loss(context.side));
    if state.expanded.get(&context) == Some(&(mask, distance)) {
        return Ok(());
    }
    let actions: Vec<_> = editor.nodes[&key]
        .edges
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1u64 << i) != 0)
        .map(|(_, e)| e.action)
        .collect();
    for action in actions {
        let d = distance + u8::from(matches!(action, Action::Place(_)));
        if d > state.config.max_plies {
            continue;
        }
        let child = Key::new(key.board().play(action).unwrap()).0;
        if !editor.nodes.contains_key(&child) {
            if limits
                .new_positions
                .is_some_and(|n| report.new_positions >= n)
            {
                report.reason = GenerationStop::PositionLimit;
                return Ok(());
            }
            if editor
                .estimated_bytes()
                .saturating_add(state.estimated_bytes())
                .saturating_add(16384)
                > state.config.max_memory_bytes
            {
                report.reason = GenerationStop::MemoryLimit;
                return Ok(());
            }
            editor.ensure(child)?;
            report.new_positions += 1;
        }
        let was_registered = editor.nodes[&key]
            .edges
            .iter()
            .find(|e| e.action == action)
            .unwrap()
            .registered;
        editor.register(key, action)?;
        if !was_registered {
            state
                .expansions
                .insert((state.distances[&context], context));
        }
        let child_context = context.child(action);
        if state
            .distances
            .get(&child_context)
            .is_none_or(|old| *old > d)
        {
            state.distances.insert(child_context, d);
        }
        let queued_distance = state.distances[&child_context];
        if !child.board().is_terminal() {
            state.schedule(
                child_context,
                editor.nodes[&child].completed_stages,
                queued_distance,
            );
        }
    }
    state.expanded.insert(context, (mask, distance));
    Ok(())
}

fn eligible_mask(editor: &BookEditor, key: Key, loss: u8) -> u64 {
    let n = &editor.nodes[&key];
    if !n.complete {
        return 0;
    }
    let Some(best) = n
        .edges
        .iter()
        .filter_map(|e| editor.effective(key, e))
        .filter(|v| v.bound == Bound::Exact)
        .map(|v| v.score)
        .max()
    else {
        return 0;
    };
    n.edges.iter().enumerate().fold(0, |mask, (i, e)| {
        if editor
            .effective(key, e)
            .is_some_and(|v| v.bound == Bound::Exact && v.score >= best - i16::from(loss))
        {
            mask | (1u64 << i)
        } else {
            mask
        }
    })
}
// Path context is separate from board identity. A registered branch can become
// inactive and later reactivate; an old shortest distance is only a queue hint.
fn active_distance(editor: &BookEditor, state: &GenerationState, context: Context) -> Option<u8> {
    fn visit(
        editor: &BookEditor,
        state: &GenerationState,
        context: Context,
        memo: &mut BTreeMap<Context, Option<u8>>,
    ) -> Option<u8> {
        if state.roots.binary_search(&context).is_ok() {
            return Some(0);
        }
        if let Some(&v) = memo.get(&context) {
            return v;
        }
        let mut best = None;
        if let Some(parents) = editor.parents.get(&context.key) {
            for &parent in parents {
                let parent_context = Context {
                    key: parent,
                    side: context.side.other(),
                };
                if !state.distances.contains_key(&parent_context) {
                    continue;
                }
                let mask = eligible_mask(editor, parent, state.config.loss(parent_context.side));
                for (i, e) in editor.nodes[&parent].edges.iter().enumerate() {
                    if !e.registered
                        || mask & (1u64 << i) == 0
                        || parent_context.child(e.action) != context
                    {
                        continue;
                    }
                    if let Some(d) = visit(editor, state, parent_context, memo) {
                        let d = d + u8::from(matches!(e.action, Action::Place(_)));
                        if d <= state.config.max_plies && best.is_none_or(|old| d < old) {
                            best = Some(d);
                        }
                    }
                }
            }
        }
        memo.insert(context, best);
        best
    }
    visit(editor, state, context, &mut BTreeMap::new())
}
