//! Analysis mode (§7.3 of the plan): searches of the shown position, the
//! exact solve on request, and the whole record from the end backwards.
//! Results are kept by position, so moving through the record does not
//! search the same position twice.
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

#[derive(Clone, Debug)]
pub struct Analysis {
    /// Scores are for the side to move.
    pub report: Report,
    /// The level searched; `None` for the exact solve.
    pub level: Option<u8>,
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
    id: SearchId,
    job: Job,
    position: Position,
    /// `None` for the exact solve.
    level: Option<u8>,
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

impl Analyzer {
    pub fn get(&self, position: Position) -> Option<&Analysis> {
        self.cache.get(&position)
    }

    /// The running search's progress when it is for `position`.
    pub fn live(&self, position: Position) -> Option<&Report> {
        self.running
            .as_ref()
            .filter(|r| r.position == position && r.job != Job::Batch)
            .and_then(|r| r.live.as_ref())
    }

    pub fn is_searching(&self, position: Position) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.position == position && r.job != Job::Batch)
    }

    /// The level now searched at `position`: `Some(None)` for the exact solve.
    #[cfg(test)]
    pub fn searching_level(&self, position: Position) -> Option<Option<u8>> {
        self.running
            .as_ref()
            .filter(|r| r.position == position && r.job != Job::Batch)
            .map(|r| r.level)
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
        if analysis.report.moves.is_empty() {
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

    /// The value for Black of the position after `index` actions: exact at
    /// the end of the game, the search's score otherwise. A forced pass
    /// shows the same discs as the next position, so it takes its value.
    /// The analysis behind the value of the position after `index` actions;
    /// for a forced pass, the next position's.
    pub fn analysis_at(&self, history: &History, index: usize) -> Option<&Analysis> {
        let mut position = history.position(index);
        if position.must_pass() && !position.is_terminal() {
            position = position.play(Action::Pass).ok()?;
        }
        self.cache.get(&position)
    }

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
        if self.running.take().is_some() {
            effects.push(Effect::CancelSearch);
        }
    }

    /// Gives the engine to an AI move: the running search is dropped, and a
    /// batch position goes back to the queue to be searched afterwards.
    pub fn interrupt(&mut self, effects: &mut Vec<Effect>) {
        if let Some(running) = self.running.take() {
            effects.push(Effect::CancelSearch);
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
        if let Some(running) = &mut self.running {
            running.stopping = true;
            effects.push(Effect::MoveNow);
        }
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

    /// The exact solve of `position`, after the running search.
    pub fn exact(&mut self, position: Position, next_id: &mut u64, effects: &mut Vec<Effect>) {
        if !searchable(position) {
            return;
        }
        self.cancel(effects);
        let spec = SearchSpec {
            selectivity: Some(Selectivity::Full),
            all_moves: true,
            ..SearchSpec::new(Goal::Endgame)
        };
        self.start(Job::Exact, position, None, spec, next_id, effects);
    }

    /// Starts the next search when the engine is free: the batch first,
    /// then the shown position. A search of another shown position stops.
    pub fn pump(&mut self, shown: Position, next_id: &mut u64, effects: &mut Vec<Effect>) {
        if let Some(running) = &self.running {
            if running.job == Job::Batch || running.position == shown {
                return;
            }
            self.running = None;
            effects.push(Effect::CancelSearch);
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
                self.start(Job::Batch, position, Some(level), spec, next_id, effects);
                return;
            }
            self.batch = None;
        }
        if !searchable(shown) {
            return;
        }
        if let Some(level) = self.wanted_level(shown) {
            let spec = SearchSpec {
                all_moves: true,
                ..SearchSpec::new(Goal::Level(level))
            };
            self.start(Job::Shown, shown, Some(level), spec, next_id, effects);
        }
    }

    fn start(
        &mut self,
        job: Job,
        position: Position,
        level: Option<u8>,
        spec: SearchSpec,
        next_id: &mut u64,
        effects: &mut Vec<Effect>,
    ) {
        let id = SearchId(*next_id);
        *next_id += 1;
        self.running = Some(Running {
            id,
            job,
            position,
            level,
            live: None,
            stopping: false,
        });
        effects.push(Effect::StartSearch {
            job: SearchJob { id, position, spec },
        });
    }

    pub fn on_progress(&mut self, id: SearchId, report: &Report) -> bool {
        match self.running.as_mut().filter(|r| r.id == id) {
            Some(running) => {
                running.live = Some(report.clone());
                true
            }
            None => false,
        }
    }

    /// Keeps the result; false when `id` is not this analyzer's search.
    pub fn on_finished(&mut self, id: SearchId, report: Report) -> bool {
        let Some(running) = self.running.take_if(|r| r.id == id) else {
            return false;
        };
        let previous = self.cache.remove(&running.position);
        // A search stopped while scoring the moves keeps those it scored.
        let partial = running.live.map(|live| live.moves).unwrap_or_default();
        let keep_moves = |mut report: Report| {
            if report.moves.is_empty() {
                report.moves = match &previous {
                    Some(p) if p.report.moves.len() >= partial.len() => p.report.moves.clone(),
                    _ => partial.clone(),
                };
            }
            report
        };
        let settled = running.stopping;
        let analysis = match running.job {
            // Stopped before every move was scored: the lower level's whole
            // list stays, so the board shows one level's values.
            Job::Shown if report.moves.is_empty() && previous.is_some() => Analysis {
                settled,
                ..previous.clone().unwrap()
            },
            Job::Shown => Analysis {
                report: keep_moves(report),
                level: running.level,
                settled,
            },
            // A solve without a score was stopped early; the old one stays.
            Job::Exact if report.score.is_none() => match previous.clone() {
                Some(previous) => previous,
                None => return true,
            },
            Job::Exact => Analysis {
                report: keep_moves(report),
                level: None,
                settled: true,
            },
            // A stopped record search keeps what was there.
            Job::Batch if report.score.is_none() => match previous.clone() {
                Some(previous) => previous,
                None => return true,
            },
            Job::Batch => match previous.clone() {
                Some(previous) if previous.level.is_none() || previous.is_exact() => previous,
                // The new value replaces the old; the move values stay only
                // when they are of the same level.
                previous => Analysis {
                    report: Report {
                        moves: previous
                            .filter(|p| p.level == running.level)
                            .map(|p| p.report.moves)
                            .unwrap_or_default(),
                        ..report
                    },
                    level: running.level,
                    settled: false,
                },
            },
        };
        if analysis.report.score.is_some() || !analysis.report.moves.is_empty() {
            self.cache.insert(running.position, analysis);
        }
        true
    }

    pub fn on_failed(&mut self, id: SearchId) -> bool {
        let failed = self.running.take_if(|r| r.id == id).is_some();
        if failed {
            self.batch = None;
        }
        failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Grouping;
    use deft_reversi_engine::{MoveScore, Square};
    use std::time::Duration;

    fn place(name: &str) -> Action {
        Action::Place(name.parse::<Square>().unwrap())
    }

    fn report(score: i32, moves: Vec<MoveScore>) -> Report {
        Report {
            best_move: None,
            score: Some(score),
            reading: None,
            pv: Vec::new(),
            moves,
            nodes: 0,
            elapsed: Duration::ZERO,
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

    fn f5_scored(score: i32) -> Vec<MoveScore> {
        vec![MoveScore {
            action: place("f5"),
            score,
        }]
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
    fn the_shown_position_is_searched_at_rising_levels_with_every_move() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let start = Position::initial();
        let mut levels = Vec::new();
        loop {
            let mut effects = Vec::new();
            a.pump(start, &mut ids, &mut effects);
            let Some(&(id, position, spec)) = started(&effects).first() else {
                break;
            };
            assert_eq!(position, start);
            assert!(spec.all_moves);
            let Goal::Level(level) = spec.goal else {
                panic!("a level search");
            };
            levels.push(level);
            // While it runs, nothing else starts.
            effects.clear();
            a.pump(start, &mut ids, &mut effects);
            assert!(effects.is_empty());
            assert_eq!(a.searching_level(start), Some(Some(level)));
            assert!(a.on_finished(id, report(i32::from(level), f5_scored(0))));
            assert_eq!(a.get(start).unwrap().level, Some(level));
        }
        assert_eq!(levels, [1, 4, 8, 12, 16, 20]);
        assert_eq!(a.get(start).unwrap().report.score, Some(20));
    }

    #[test]
    fn a_stopped_level_keeps_the_lower_values_and_goes_no_further() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let mut effects = Vec::new();
        let start = Position::initial();
        a.pump(start, &mut ids, &mut effects);
        a.on_finished(SearchId(1), report(1, f5_scored(1)));
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        let (id, _, _) = started(&effects)[0];
        a.stop(&mut effects);
        a.on_finished(id, report(9, Vec::new()));
        let analysis = a.get(start).unwrap();
        assert_eq!((analysis.level, analysis.report.score), (Some(1), Some(1)));
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        assert!(started(&effects).is_empty());
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
        assert!(!spec.all_moves);
        let exact = Report {
            reading: Some(Reading::Endgame { percent: 100 }),
            ..report(0, Vec::new())
        };
        a.on_finished(id, exact);
        effects.clear();
        a.pump(history.current(), &mut ids, &mut effects);
        let [(_, _, spec)] = started(&effects)[..] else {
            panic!("the moves are searched");
        };
        assert!(spec.all_moves);
        assert_eq!(spec.goal, Goal::Level(20));
    }

    #[test]
    fn a_redo_searches_the_record_again_but_not_what_is_exact() {
        let mut history = History::new(Position::initial());
        history.play(place("f5"), Grouping::NewGroup).unwrap();
        let mut a = Analyzer::default();
        let mut ids = 1;
        a.start_batch(&history, false);
        let exact = Report {
            reading: Some(Reading::Endgame { percent: 100 }),
            ..report(0, Vec::new())
        };
        for finished in [report(1, Vec::new()), exact] {
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
    fn an_exact_result_ends_the_levels() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let mut effects = Vec::new();
        let start = Position::initial();
        a.pump(start, &mut ids, &mut effects);
        let exact = Report {
            reading: Some(Reading::Endgame { percent: 100 }),
            ..report(2, f5_scored(2))
        };
        a.on_finished(SearchId(1), exact);
        assert!(a.get(start).unwrap().is_exact());
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        assert!(started(&effects).is_empty());
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
        assert!(!a.on_finished(SearchId(1), report(5, Vec::new())));
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
            assert!(!spec.all_moves);
            a.on_finished(id, report(scores[expected], Vec::new()));
        }
        let mut effects = Vec::new();
        a.pump(history.current(), &mut ids, &mut effects);
        assert_eq!(a.batch_progress(), None);
        // The shown position still gets its full search.
        assert!(started(&effects)[0].2.all_moves);

        assert_eq!(a.black_value(&history, 0), Some(0));
        assert_eq!(a.black_value(&history, 1), Some(1));
        assert_eq!(a.black_value(&history, 2), Some(3));
        // f5 gained a disc; d6 took White from -1 to -3.
        assert_eq!(a.loss(&history, 0), Some(-1));
        assert_eq!(a.loss(&history, 1), Some(2));
    }

    #[test]
    fn an_exact_solve_keeps_the_move_scores() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let mut effects = Vec::new();
        let start = Position::initial();
        a.pump(start, &mut ids, &mut effects);
        let moves = vec![MoveScore {
            action: place("f5"),
            score: 0,
        }];
        a.on_finished(SearchId(1), report(1, moves.clone()));
        a.exact(start, &mut ids, &mut effects);
        let (id, _, spec) = *started(&effects).last().unwrap();
        assert_eq!(spec.goal, Goal::Endgame);
        a.on_finished(id, report(0, Vec::new()));
        let analysis = a.get(start).unwrap();
        assert_eq!(analysis.report.score, Some(0));
        assert_eq!(analysis.report.moves, moves);
    }
}
