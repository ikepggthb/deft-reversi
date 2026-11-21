//! Analysis mode (§7.3 of the plan): searches of the shown position, the
//! exact solve on request, and the whole record from the end backwards.
//! Results are kept by position, so moving through the record does not
//! search the same position twice.
//!
//! The shown position is read one move at a time: each legal move is a
//! search of its own, so its value appears as soon as it is read, and
//! while it is read the value of each finished iteration shows, with how
//! it was read.
use super::Effect;
use crate::{
    domain::{History, editor},
    services::engine::{Reading, Report, SearchId, SearchJob, SearchSpec},
};
use deft_reversi_engine::{Action, Color, Goal, Position, Selectivity};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisSettings {
    /// For the shown position, which also scores every move, and for each
    /// position of the whole record.
    pub level: u8,
    /// A move that loses this many discs or more is marked in the record.
    pub loss_threshold: i32,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            level: 20,
            loss_threshold: 3,
        }
    }
}

/// One move's value for the side to move, and how that value was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveValue {
    pub action: Action,
    pub score: i32,
    pub reading: Option<Reading>,
    /// The move is still being read: this is its last finished iteration.
    pub provisional: bool,
}

/// A legal move as the analysis shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownMove {
    pub action: Action,
    pub value: Option<MoveValue>,
    /// Read by the running level, or by the last one when none runs; a
    /// value from an earlier level is shown faintly.
    pub fresh: bool,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    /// The position's value, best move and line, for the side to move.
    pub report: Report,
    /// The level searched; `None` for the exact solve.
    pub level: Option<u8>,
    /// Every move read, best first, each with its own reading.
    pub moves: Vec<MoveValue>,
    /// Stopped by the user: not searched further.
    settled: bool,
}

impl Analysis {
    /// The values are exact: solved to the end with no pruning.
    pub fn is_exact(&self) -> bool {
        matches!(self.report.reading, Some(Reading::Endgame { percent: 100 }))
    }
}

/// The shown position is searched at rising levels up to the setting, so a
/// value comes at once and then improves: 1, 4, 8, 12, … every four levels.
pub fn next_level(searched: u8, target: u8) -> u8 {
    let next = match searched {
        0 => 1,
        1..=3 => 4,
        _ => (searched / 4 + 1) * 4,
    };
    next.min(target)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Shown,
    Exact,
    Batch,
}

struct Running {
    /// The search now running; `None` between two moves.
    id: Option<SearchId>,
    job: Job,
    position: Position,
    /// `None` for the exact solve.
    level: Option<u8>,
    /// The moves still to read, the next one last, and the one being read.
    pending: Vec<Action>,
    current: Option<Action>,
    /// The values read at this level so far.
    values: Vec<MoveValue>,
    /// The search of the best move so far, for the position's value and line.
    best: Option<Report>,
    /// The running search's last progress.
    live: Option<Report>,
    /// The user stopped it: what it found is final.
    stopping: bool,
}

#[derive(Default)]
pub struct Analyzer {
    pub settings: AnalysisSettings,
    cache: HashMap<Position, Analysis>,
    running: Option<Running>,
    /// Positions still to search, the last one first; and how many there were.
    batch: Option<(Vec<Position>, usize)>,
}

/// Positions a search can score: not over, not a forced pass, and with
/// enough discs for the evaluation.
fn searchable(position: Position) -> bool {
    !position.is_terminal() && !position.must_pass() && editor::searchable(position)
}

fn sort_best_first(values: &mut [MoveValue]) {
    values.sort_by_key(|v| std::cmp::Reverse(v.score));
}

impl Analyzer {
    pub fn get(&self, position: Position) -> Option<&Analysis> {
        self.cache.get(&position)
    }

    fn running_for(&self, position: Position) -> Option<&Running> {
        self.running
            .as_ref()
            .filter(|r| r.position == position && r.job != Job::Batch)
    }

    /// The running search's progress when it is for `position`.
    pub fn live(&self, position: Position) -> Option<&Report> {
        self.running_for(position).and_then(|r| r.live.as_ref())
    }

    /// The move being read at `position`, and at what level (`None` for
    /// the exact solve).
    pub fn reading_move(&self, position: Position) -> Option<(Option<u8>, Action)> {
        self.running_for(position)
            .and_then(|r| Some((r.level, r.current?)))
    }

    pub fn is_searching(&self, position: Position) -> bool {
        self.running_for(position).is_some()
    }

    /// The position's value, best move and line: the last level read
    /// through, or the best move so far before any level is.
    pub fn report(&self, position: Position) -> Option<&Report> {
        self.cache
            .get(&position)
            .map(|a| &a.report)
            .or_else(|| self.running_for(position)?.best.as_ref())
    }

    /// Every legal move with its newest value: the running level's where it
    /// has one, else the last level's. Fresh values come first, best first.
    pub fn moves(&self, position: Position) -> Vec<ShownMove> {
        let running = self.running_for(position);
        let older = self.cache.get(&position).map_or(&[][..], |a| &a.moves[..]);
        let mut shown: Vec<ShownMove> = position
            .legal_moves()
            .map(|square| {
                let action = Action::Place(square);
                let find = |values: &[MoveValue]| values.iter().find(|v| v.action == action).cloned();
                match running.and_then(|r| find(&r.values)) {
                    Some(value) => ShownMove {
                        action,
                        value: Some(value),
                        fresh: true,
                    },
                    None => ShownMove {
                        action,
                        value: find(older),
                        fresh: running.is_none(),
                    },
                }
            })
            .collect();
        shown.sort_by_key(|m| {
            (
                !(m.fresh && m.value.is_some()),
                m.value.is_none(),
                std::cmp::Reverse(m.value.as_ref().map(|v| v.score)),
            )
        });
        shown
    }

    /// The level now searched at `position`: `Some(None)` for the exact solve.
    #[cfg(test)]
    pub fn searching_level(&self, position: Position) -> Option<Option<u8>> {
        self.running_for(position).map(|r| r.level)
    }

    /// The level the shown position needs next, if any.
    fn wanted_level(&self, position: Position) -> Option<u8> {
        let target = self.settings.level;
        let Some(analysis) = self.cache.get(&position) else {
            return Some(next_level(0, target));
        };
        if analysis.settled {
            return None;
        }
        // The exact solve is as far as it goes; it scores the moves itself.
        let level = analysis.level?;
        if analysis.moves.is_empty() {
            // A record search, even an exact one, has no move values yet.
            return Some(if level < target {
                next_level(level, target)
            } else {
                target
            });
        }
        if analysis.is_exact() || level >= target {
            return None;
        }
        Some(next_level(level, target))
    }

    /// Positions done and in all, while the whole record is analysed.
    pub fn batch_progress(&self) -> Option<(usize, usize)> {
        self.batch
            .as_ref()
            .map(|(queue, total)| (total - queue.len(), *total))
    }

    /// The analysis behind the value of the position after `index` actions;
    /// for a forced pass, the next position's.
    pub fn analysis_at(&self, history: &History, index: usize) -> Option<&Analysis> {
        let mut position = history.position(index);
        if position.must_pass() && !position.is_terminal() {
            position = position.play(Action::Pass).ok()?;
        }
        self.cache.get(&position)
    }

    /// The value for Black of the position after `index` actions: exact at
    /// the end of the game, the search's score otherwise. A forced pass
    /// shows the same discs as the next position, so it takes its value.
    pub fn black_value(&self, history: &History, index: usize) -> Option<i32> {
        let mut position = history.position(index);
        if position.is_terminal() {
            return Some(position.final_score(Color::Black));
        }
        if position.must_pass() {
            position = position.play(Action::Pass).ok()?;
        }
        let score = self.cache.get(&position)?.report.score?;
        Some(match position.side_to_move() {
            Color::Black => score,
            Color::White => -score,
        })
    }

    /// Discs the move at `index` lost against the best move, for its player,
    /// when both positions are known.
    pub fn loss(&self, history: &History, index: usize) -> Option<i32> {
        let Action::Place(_) = history.actions()[index] else {
            return None;
        };
        let mover = history.position(index).side_to_move();
        let before = self.black_value(history, index)?;
        let after = self.black_value(history, index + 1)?;
        Some(match mover {
            Color::Black => before - after,
            Color::White => after - before,
        })
    }

    pub fn settings_changed(&mut self, settings: AnalysisSettings) {
        if settings.level > self.settings.level {
            // Shown positions go on up to the new level.
            for analysis in self.cache.values_mut() {
                analysis.settled = false;
            }
        }
        self.settings = settings;
    }

    /// Drops the running search and the batch.
    pub fn cancel(&mut self, effects: &mut Vec<Effect>) {
        self.batch = None;
        if let Some(running) = self.running.take()
            && running.id.is_some()
        {
            effects.push(Effect::CancelSearch);
        }
    }

    /// Gives the engine to an AI move: the running search is dropped, and a
    /// batch position goes back to the queue to be searched afterwards.
    pub fn interrupt(&mut self, effects: &mut Vec<Effect>) {
        if let Some(running) = self.running.take() {
            if running.id.is_some() {
                effects.push(Effect::CancelSearch);
            }
            if running.job == Job::Batch
                && let Some((queue, _)) = &mut self.batch
            {
                queue.push(running.position);
            }
        }
    }

    /// Stops the running search but keeps what it found; ends the batch.
    pub fn stop(&mut self, effects: &mut Vec<Effect>) {
        self.batch = None;
        let Some(running) = &mut self.running else {
            return;
        };
        running.stopping = true;
        if running.id.is_some() {
            effects.push(Effect::MoveNow);
            return;
        }
        // Between two moves: nothing runs, so it ends here.
        let running = self.running.take().expect("checked above");
        self.finish_moves(running);
    }

    /// Forgets what is known of `position` and reads it again from the
    /// lowest level.
    pub fn reanalyze(&mut self, position: Position, effects: &mut Vec<Effect>) {
        if self.running_for(position).is_some() {
            return;
        }
        if self.running.as_ref().is_some_and(|r| r.job == Job::Batch) {
            self.cancel(effects);
        }
        self.cache.remove(&position);
    }

    /// Whether a record search of `position` has nothing to add: it is
    /// solved exactly, or with `redo` unset, read at the level already.
    fn batch_done(&self, position: Position, redo: bool) -> bool {
        self.cache.get(&position).is_some_and(|a| match a.level {
            None => true,
            _ if a.is_exact() => true,
            Some(level) => !redo && level >= self.settings.level,
        })
    }

    pub fn start_batch(&mut self, history: &History, redo: bool) {
        let queue: Vec<Position> = (0..=history.actions().len())
            .map(|i| history.position(i))
            .filter(|&p| searchable(p) && !self.batch_done(p, redo))
            .collect();
        // Popped from the end: the record is searched backwards, so the
        // endgame results in the table help the earlier positions.
        let total = queue.len();
        self.batch = (total > 0).then_some((queue, total));
    }

    /// The exact solve of `position`, move by move, after the running search.
    pub fn exact(&mut self, position: Position, next_id: &mut u64, effects: &mut Vec<Effect>) {
        if !searchable(position) {
            return;
        }
        self.cancel(effects);
        self.begin_moves(Job::Exact, position, None, next_id, effects);
    }

    /// Starts the next search when the engine is free: the next move of the
    /// shown position's level, the batch, then the shown position's next
    /// level. A search of another shown position stops.
    pub fn pump(&mut self, shown: Position, next_id: &mut u64, effects: &mut Vec<Effect>) {
        if let Some(running) = &self.running {
            if running.job == Job::Batch || running.position == shown {
                if running.id.is_none() {
                    self.next_move(next_id, effects);
                }
                return;
            }
            if running.id.is_some() {
                effects.push(Effect::CancelSearch);
            }
            self.running = None;
        }
        if let Some((queue, _)) = &mut self.batch {
            while let Some(position) = queue.pop() {
                // Solved exactly since it was queued, say by the shown search.
                if self
                    .cache
                    .get(&position)
                    .is_some_and(|a| a.level.is_none() || a.is_exact())
                {
                    continue;
                }
                let level = self.settings.level;
                let spec = SearchSpec::new(Goal::Level(level));
                let id = Self::start(position, spec, next_id, effects);
                self.running = Some(Running {
                    id: Some(id),
                    job: Job::Batch,
                    position,
                    level: Some(level),
                    pending: Vec::new(),
                    current: None,
                    values: Vec::new(),
                    best: None,
                    live: None,
                    stopping: false,
                });
                return;
            }
            self.batch = None;
        }
        if !searchable(shown) {
            return;
        }
        if let Some(level) = self.wanted_level(shown) {
            self.begin_moves(Job::Shown, shown, Some(level), next_id, effects);
        }
    }

    /// A level of `position` read one move at a time: the best moves of the
    /// last level first, so the position's value settles early.
    fn begin_moves(
        &mut self,
        job: Job,
        position: Position,
        level: Option<u8>,
        next_id: &mut u64,
        effects: &mut Vec<Effect>,
    ) {
        let older = self.cache.get(&position).map_or(&[][..], |a| &a.moves[..]);
        let mut order: Vec<Action> = older.iter().map(|v| v.action).collect();
        let rest: Vec<Action> = position
            .legal_moves()
            .map(Action::Place)
            .filter(|a| !order.contains(a))
            .collect();
        order.extend(rest);
        order.retain(|a| matches!(a, Action::Place(s) if position.legal_moves().contains(*s)));
        order.reverse();
        self.running = Some(Running {
            id: None,
            job,
            position,
            level,
            pending: order,
            current: None,
            values: Vec::new(),
            best: None,
            live: None,
            stopping: false,
        });
        self.next_move(next_id, effects);
    }

    /// Reads the next move of the running level, or ends the level.
    fn next_move(&mut self, next_id: &mut u64, effects: &mut Vec<Effect>) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        let Some(action) = running.pending.pop() else {
            let running = self.running.take().expect("checked above");
            self.finish_moves(running);
            return;
        };
        let Action::Place(square) = action else {
            return;
        };
        let spec = match running.level {
            Some(level) => SearchSpec::new(Goal::Level(level)),
            None => SearchSpec {
                selectivity: Some(Selectivity::Full),
                ..SearchSpec::new(Goal::Endgame)
            },
        };
        let spec = SearchSpec {
            root_moves: Some(square.bit()),
            recover_pv: true,
            ..spec
        };
        running.id = Some(Self::start(running.position, spec, next_id, effects));
        running.current = Some(action);
        running.live = None;
    }

    fn start(
        position: Position,
        spec: SearchSpec,
        next_id: &mut u64,
        effects: &mut Vec<Effect>,
    ) -> SearchId {
        let id = SearchId(*next_id);
        *next_id += 1;
        effects.push(Effect::StartSearch {
            job: SearchJob { id, position, spec },
        });
        id
    }

    /// Ends a level read move by move. Read through, it replaces the last
    /// level; stopped part way, the moves it read join the last level's,
    /// each with its own reading, and the position keeps its last value.
    fn finish_moves(&mut self, running: Running) {
        let previous = self.cache.remove(&running.position);
        let legal = running.position.legal_moves().count();
        let complete = running.values.len() == legal;
        let mut moves = running.values.clone();
        if let Some(previous) = &previous {
            moves.extend(
                previous
                    .moves
                    .iter()
                    .filter(|old| !running.values.iter().any(|v| v.action == old.action))
                    .cloned(),
            );
        }
        sort_best_first(&mut moves);
        let analysis = match (complete, previous) {
            (true, previous) => Analysis {
                report: running.best.unwrap_or_else(|| {
                    previous.map(|p| p.report).unwrap_or_else(empty_report)
                }),
                level: running.level,
                moves,
                settled: running.stopping || running.job == Job::Exact,
            },
            (false, Some(previous)) => Analysis {
                moves,
                settled: running.stopping || previous.settled,
                ..previous
            },
            (false, None) => match running.best {
                Some(best) => Analysis {
                    report: best,
                    level: running.level,
                    moves,
                    settled: true,
                },
                None => return,
            },
        };
        self.cache.insert(running.position, analysis);
    }

    pub fn on_progress(&mut self, id: SearchId, report: &Report) -> bool {
        let Some(running) = self.running.as_mut().filter(|r| r.id == Some(id)) else {
            return false;
        };
        running.live = Some(report.clone());
        // Each finished iteration of a move shows at once, with its reading.
        if let (Some(action), Some(score)) = (running.current, report.score) {
            running.values.retain(|v| v.action != action);
            running.values.push(MoveValue {
                action,
                score,
                reading: report.reading,
                provisional: true,
            });
            sort_best_first(&mut running.values);
        }
        true
    }

    /// Keeps the result; false when `id` is not this analyzer's search.
    pub fn on_finished(&mut self, id: SearchId, report: Report) -> bool {
        let Some(running) = self.running.as_mut().filter(|r| r.id == Some(id)) else {
            return false;
        };
        if running.job == Job::Batch {
            let running = self.running.take().expect("checked above");
            self.finish_batch(running, report);
            return true;
        }
        running.id = None;
        running.live = None;
        if let Some(action) = running.current.take() {
            running.values.retain(|v| v.action != action);
            if let Some(score) = report.score {
                running.values.push(MoveValue {
                    action,
                    score,
                    reading: report.reading,
                        provisional: false,
                });
                sort_best_first(&mut running.values);
                if running
                    .best
                    .as_ref()
                    .is_none_or(|best| best.score.is_none_or(|b| score > b))
                {
                    running.best = Some(Report {
                        best_move: Some(action),
                        moves: Vec::new(),
                        ..report
                    });
                }
            }
        }
        if running.stopping || running.pending.is_empty() {
            let running = self.running.take().expect("checked above");
            self.finish_moves(running);
        }
        true
    }

    fn finish_batch(&mut self, running: Running, report: Report) {
        let previous = self.cache.remove(&running.position);
        let analysis = match previous {
            // A stopped record search keeps what was there.
            previous if report.score.is_none() => match previous {
                Some(previous) => previous,
                None => return,
            },
            Some(previous) if previous.level.is_none() || previous.is_exact() => previous,
            // The new value replaces the old; the move values stay only
            // when they are of the same level.
            previous => Analysis {
                moves: previous
                    .filter(|p| p.level == running.level)
                    .map(|p| p.moves)
                    .unwrap_or_default(),
                report: Report {
                    moves: Vec::new(),
                    ..report
                },
                level: running.level,
                settled: false,
            },
        };
        self.cache.insert(running.position, analysis);
    }

    pub fn on_failed(&mut self, id: SearchId) -> bool {
        let failed = self.running.take_if(|r| r.id == Some(id)).is_some();
        if failed {
            self.batch = None;
        }
        failed
    }
}

fn empty_report() -> Report {
    Report {
        best_move: None,
        score: None,
        reading: None,
        pv: Vec::new(),
        moves: Vec::new(),
        nodes: 0,
        elapsed: std::time::Duration::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Grouping;
    use deft_reversi_engine::Square;
    use std::time::Duration;

    fn place(name: &str) -> Action {
        Action::Place(name.parse::<Square>().unwrap())
    }

    fn report(score: i32) -> Report {
        Report {
            score: Some(score),
            ..empty_report()
        }
    }

    fn read(score: i32, reading: Reading) -> Report {
        Report {
            reading: Some(reading),
            ..report(score)
        }
    }

    fn started(effects: &[Effect]) -> Vec<(SearchId, Position, SearchSpec)> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::StartSearch { job, .. } => Some((job.id, job.position, job.spec)),
                _ => None,
            })
            .collect()
    }

    /// The move a search reads, from its root mask.
    fn move_of(spec: SearchSpec) -> Action {
        let mask = spec.root_moves.expect("one move at a time");
        assert_eq!(mask.count_ones(), 1);
        Action::Place(Square::new(mask.trailing_zeros() as u8).unwrap())
    }

    /// Reads every move of the level the analyzer starts, scoring each by
    /// `score`; returns the level.
    fn read_level(
        a: &mut Analyzer,
        shown: Position,
        ids: &mut u64,
        score: impl Fn(Action) -> i32,
    ) -> Option<Goal> {
        let mut goal = None;
        loop {
            let mut effects = Vec::new();
            a.pump(shown, ids, &mut effects);
            let Some(&(id, position, spec)) = started(&effects).first() else {
                return goal;
            };
            assert_eq!(position, shown);
            if goal.is_some_and(|g| g != spec.goal) {
                // The next level began: leave it for the caller.
                a.cancel(&mut Vec::new());
                return goal;
            }
            goal = Some(spec.goal);
            assert!(a.on_finished(id, report(score(move_of(spec)))));
        }
    }

    #[test]
    fn levels_rise_by_four_up_to_the_setting() {
        assert_eq!(next_level(0, 20), 1);
        assert_eq!(next_level(1, 20), 4);
        assert_eq!(next_level(4, 20), 8);
        assert_eq!(next_level(17, 20), 20);
        assert_eq!(next_level(0, 2), 1);
        assert_eq!(next_level(1, 2), 2);
    }

    #[test]
    fn the_shown_position_is_read_one_move_at_a_time_at_rising_levels() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let mut levels = Vec::new();
        let mut searches = 0;
        loop {
            let mut effects = Vec::new();
            a.pump(start, &mut ids, &mut effects);
            let Some(&(id, position, spec)) = started(&effects).first() else {
                break;
            };
            assert_eq!(position, start);
            let Goal::Level(level) = spec.goal else {
                panic!("a level search");
            };
            if levels.last() != Some(&level) {
                levels.push(level);
            }
            let action = move_of(spec);
            // While it runs, nothing else starts.
            effects.clear();
            a.pump(start, &mut ids, &mut effects);
            assert!(effects.is_empty());
            assert_eq!(a.searching_level(start), Some(Some(level)));
            // f5 is the best move; each value shows as soon as it is read.
            let score = if action == place("f5") { i32::from(level) } else { -1 };
            assert!(a.on_finished(id, report(score)));
            let shown = a.moves(start);
            assert!(
                shown
                    .iter()
                    .any(|m| m.action == action && m.fresh && m.value.as_ref().unwrap().score == score)
            );
            searches += 1;
        }
        assert_eq!(levels, [1, 4, 8, 12, 16, 20]);
        assert_eq!(searches, 6 * 4);
        let analysis = a.get(start).unwrap();
        assert_eq!(analysis.report.score, Some(20));
        assert_eq!(analysis.report.best_move, Some(place("f5")));
        assert_eq!(analysis.moves.len(), 4);
        assert_eq!(analysis.moves[0].action, place("f5"));
    }

    #[test]
    fn a_move_shows_each_iteration_with_its_own_reading() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let mut effects = Vec::new();
        a.pump(start, &mut ids, &mut effects);
        let (id, _, spec) = started(&effects)[0];
        let action = move_of(spec);
        let depth = Reading::Depth {
            depth: 1,
            percent: 83,
        };
        a.on_progress(id, &read(3, depth));
        let value = a.moves(start).into_iter().find(|m| m.action == action);
        let value = value.unwrap().value.unwrap();
        assert!(value.provisional);
        assert_eq!((value.score, value.reading), (3, Some(depth)));
        // The moves not read yet have no value at the first level.
        assert!(a.moves(start).iter().filter(|m| m.value.is_none()).count() == 3);
    }

    #[test]
    fn a_stopped_level_keeps_what_it_read_each_with_its_reading() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let low = Reading::Depth {
            depth: 1,
            percent: 83,
        };
        let high = Reading::Depth {
            depth: 4,
            percent: 83,
        };
        let mut effects = Vec::new();
        // Level 1, every move.
        loop {
            effects.clear();
            a.pump(start, &mut ids, &mut effects);
            let (id, _, spec) = started(&effects)[0];
            if spec.goal != Goal::Level(1) {
                break;
            }
            a.on_finished(id, read(1, low));
        }
        // Level 4: one move read, then stopped while reading the next.
        let (id, _, spec) = started(&effects)[0];
        let first = move_of(spec);
        a.on_finished(id, read(9, high));
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        let (id, _, _) = started(&effects)[0];
        effects.clear();
        a.stop(&mut effects);
        assert_eq!(effects, [Effect::MoveNow]);
        a.on_finished(id, Report { score: None, ..empty_report() });
        let analysis = a.get(start).unwrap();
        // The position keeps level 1's value; the move read at level 4 says so.
        assert_eq!((analysis.level, analysis.report.score), (Some(1), Some(1)));
        assert_eq!(analysis.moves.len(), 4);
        let value = analysis.moves.iter().find(|v| v.action == first).unwrap();
        assert_eq!((value.score, value.reading), (9, Some(high)));
        assert!(analysis.moves.iter().filter(|v| v.reading == Some(low)).count() == 3);
        // Stopped: nothing more until asked again.
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        assert!(started(&effects).is_empty());
        a.reanalyze(start, &mut effects);
        a.pump(start, &mut ids, &mut effects);
        assert_eq!(started(&effects)[0].2.goal, Goal::Level(1));
    }

    #[test]
    fn an_exact_record_value_still_gets_its_move_values() {
        let history = History::new(Position::initial());
        let mut a = Analyzer::default();
        let mut ids = 1;
        a.start_batch(&history, false);
        let mut effects = Vec::new();
        a.pump(history.current(), &mut ids, &mut effects);
        let (id, _, spec) = started(&effects)[0];
        assert_eq!(spec.root_moves, None);
        a.on_finished(id, read(0, Reading::Endgame { percent: 100 }));
        effects.clear();
        a.pump(history.current(), &mut ids, &mut effects);
        let [(_, _, spec)] = started(&effects)[..] else {
            panic!("the moves are searched");
        };
        assert!(spec.root_moves.is_some());
        assert_eq!(spec.goal, Goal::Level(20));
    }

    #[test]
    fn a_redo_searches_the_record_again_but_not_what_is_exact() {
        let mut history = History::new(Position::initial());
        history.play(place("f5"), Grouping::NewGroup).unwrap();
        let mut a = Analyzer::default();
        let mut ids = 1;
        a.start_batch(&history, false);
        for finished in [report(1), read(0, Reading::Endgame { percent: 100 })] {
            let mut effects = Vec::new();
            a.pump(history.current(), &mut ids, &mut effects);
            a.on_finished(started(&effects)[0].0, finished);
        }
        a.start_batch(&history, false);
        assert_eq!(a.batch_progress(), None, "nothing new at this level");
        a.start_batch(&history, true);
        assert_eq!(a.batch_progress(), Some((0, 1)), "all but the exact one");
    }

    #[test]
    fn an_exact_level_ends_the_levels() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let exact = Reading::Endgame { percent: 100 };
        loop {
            let mut effects = Vec::new();
            a.pump(start, &mut ids, &mut effects);
            let Some(&(id, _, _)) = started(&effects).first() else {
                break;
            };
            a.on_finished(id, read(2, exact));
        }
        let analysis = a.get(start).unwrap();
        assert!(analysis.is_exact());
        assert_eq!(analysis.level, Some(1));
    }

    #[test]
    fn moving_away_stops_the_old_search() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let mut effects = Vec::new();
        let start = Position::initial();
        a.pump(start, &mut ids, &mut effects);
        let next = start.play(place("f5")).unwrap();
        effects.clear();
        a.pump(next, &mut ids, &mut effects);
        assert_eq!(effects[0], Effect::CancelSearch);
        assert_eq!(started(&effects)[0].1, next);
        // The old result arrives late and is not this search.
        assert!(!a.on_finished(SearchId(1), report(5)));
        assert!(a.get(start).is_none());
    }

    #[test]
    fn the_batch_runs_backwards_then_values_and_losses_follow() {
        let mut history = History::new(Position::initial());
        for m in ["f5", "d6", "c3"] {
            history.play(place(m), Grouping::NewGroup).unwrap();
        }
        let mut a = Analyzer::default();
        let mut ids = 1;
        a.start_batch(&history, false);
        assert_eq!(a.batch_progress(), Some((0, 4)));
        // Scores for the side to move: 0, -1, +3 (a bad d6), -2.
        let scores = [0, -1, 3, -2];
        for expected in (0..4).rev() {
            let mut effects = Vec::new();
            a.pump(history.current(), &mut ids, &mut effects);
            let [(id, position, spec)] = started(&effects)[..] else {
                panic!("one search");
            };
            assert_eq!(position, history.position(expected));
            assert_eq!(spec.root_moves, None);
            a.on_finished(id, report(scores[expected]));
        }
        let mut effects = Vec::new();
        a.pump(history.current(), &mut ids, &mut effects);
        assert_eq!(a.batch_progress(), None);
        // The shown position still gets its moves read.
        assert!(started(&effects)[0].2.root_moves.is_some());

        assert_eq!(a.black_value(&history, 0), Some(0));
        assert_eq!(a.black_value(&history, 1), Some(1));
        assert_eq!(a.black_value(&history, 2), Some(3));
        // f5 gained a disc; d6 took White from -1 to -3.
        assert_eq!(a.loss(&history, 0), Some(-1));
        assert_eq!(a.loss(&history, 1), Some(2));
    }

    #[test]
    fn the_exact_solve_reads_the_moves_best_first() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let goal = read_level(&mut a, start, &mut ids, |action| {
            if action == place("d3") { 2 } else { 0 }
        });
        assert_eq!(goal, Some(Goal::Level(1)));
        let mut effects = Vec::new();
        a.exact(start, &mut ids, &mut effects);
        let (_, _, spec) = *started(&effects).last().unwrap();
        assert_eq!(spec.goal, Goal::Endgame);
        assert_eq!(spec.selectivity, Some(Selectivity::Full));
        // The last level's best move comes first.
        assert_eq!(move_of(spec), place("d3"));
        let exact = Reading::Endgame { percent: 100 };
        let mut id = started(&effects).last().unwrap().0;
        loop {
            a.on_finished(id, read(0, exact));
            effects.clear();
            a.pump(start, &mut ids, &mut effects);
            match started(&effects).first() {
                Some(&(next, _, _)) => id = next,
                None => break,
            }
        }
        let analysis = a.get(start).unwrap();
        assert!(analysis.is_exact());
        assert_eq!(analysis.level, None);
        assert!(analysis.moves.iter().all(|v| v.reading == Some(exact)));
        let _ = Duration::ZERO;
    }
}
