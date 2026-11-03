//! Analysis mode (§7.3 of the plan): searches of the shown position, the
//! exact solve on request, and the whole record from the end backwards.
//! Results are kept by position, so moving through the record does not
//! search the same position twice.
use super::Effect;
use crate::{
    domain::History,
    services::engine::{Report, SearchId, SearchJob, SearchSpec},
};
use deft_reversi_engine::{Action, Color, Goal, Position, Selectivity};
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisSettings {
    pub level: u8,
    /// For the shown position, which also scores every move.
    pub time_limit: Duration,
    /// For each position of the whole record.
    pub batch_time: Duration,
    /// A move that loses this many discs or more is marked in the record.
    pub loss_threshold: i32,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            level: 20,
            time_limit: Duration::from_secs(10),
            batch_time: Duration::from_secs(1),
            loss_threshold: 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Analysis {
    /// Scores are for the side to move.
    pub report: Report,
    /// The full analysis ran here, so it is not asked for again.
    full: bool,
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
    live: Option<Report>,
}

#[derive(Default)]
pub struct Analyzer {
    pub settings: AnalysisSettings,
    cache: HashMap<Position, Analysis>,
    running: Option<Running>,
    /// Positions still to search, the last one first; and how many there were.
    batch: Option<(Vec<Position>, usize)>,
}

/// Positions a search can score: not over, and not a forced pass.
fn searchable(position: Position) -> bool {
    !position.is_terminal() && !position.must_pass()
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

    /// Positions done and in all, while the whole record is analysed.
    pub fn batch_progress(&self) -> Option<(usize, usize)> {
        self.batch
            .as_ref()
            .map(|(queue, total)| (total - queue.len(), *total))
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
        if settings.level != self.settings.level {
            // Shown positions are searched again at the new level.
            for analysis in self.cache.values_mut() {
                analysis.full = false;
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

    /// Stops the running search but keeps what it found; ends the batch.
    pub fn stop(&mut self, effects: &mut Vec<Effect>) {
        self.batch = None;
        if self.running.is_some() {
            effects.push(Effect::MoveNow);
        }
    }

    pub fn start_batch(&mut self, history: &History) {
        let queue: Vec<Position> = (0..=history.actions().len())
            .map(|i| history.position(i))
            .filter(|&p| searchable(p) && !self.cache.contains_key(&p))
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
            ..SearchSpec::new(Goal::Endgame)
        };
        self.start(Job::Exact, position, spec, next_id, effects);
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
                if self.cache.contains_key(&position) {
                    continue;
                }
                let spec = SearchSpec {
                    time_limit: Some(self.settings.batch_time),
                    ..SearchSpec::new(Goal::Level(self.settings.level))
                };
                self.start(Job::Batch, position, spec, next_id, effects);
                return;
            }
            self.batch = None;
        }
        let needed = searchable(shown) && self.cache.get(&shown).is_none_or(|a| !a.full);
        if needed {
            let spec = SearchSpec {
                time_limit: Some(self.settings.time_limit),
                all_moves: true,
                ..SearchSpec::new(Goal::Level(self.settings.level))
            };
            self.start(Job::Shown, shown, spec, next_id, effects);
        }
    }

    fn start(
        &mut self,
        job: Job,
        position: Position,
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
            live: None,
        });
        effects.push(Effect::StartSearch {
            job: SearchJob { id, position, spec },
            delay: Duration::ZERO,
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
        let keep_moves = |mut report: Report| {
            if report.moves.is_empty()
                && let Some(p) = &previous
            {
                report.moves = p.report.moves.clone();
            }
            report
        };
        let analysis = match running.job {
            Job::Shown => Analysis {
                report: keep_moves(report),
                full: true,
            },
            // A solve without a score was stopped early; the old one stays.
            Job::Exact if report.score.is_none() => match previous.clone() {
                Some(previous) => previous,
                None => return true,
            },
            Job::Exact => Analysis {
                report: keep_moves(report),
                full: previous.as_ref().is_some_and(|p| p.full),
            },
            Job::Batch => match previous.clone() {
                Some(previous) => previous,
                None => Analysis {
                    report,
                    full: false,
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

    #[test]
    fn the_shown_position_is_searched_once_with_every_move() {
        let mut a = Analyzer::default();
        let mut ids = 1;
        let mut effects = Vec::new();
        let start = Position::initial();
        a.pump(start, &mut ids, &mut effects);
        let [(id, position, spec)] = started(&effects)[..] else {
            panic!("one search");
        };
        assert_eq!(position, start);
        assert!(spec.all_moves);
        // While it runs, nothing else starts.
        effects.clear();
        a.pump(start, &mut ids, &mut effects);
        assert!(effects.is_empty());
        assert!(a.on_finished(id, report(0, Vec::new())));
        a.pump(start, &mut ids, &mut effects);
        assert!(effects.is_empty(), "searched already");
        assert_eq!(a.get(start).unwrap().report.score, Some(0));
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
        a.start_batch(&history);
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
