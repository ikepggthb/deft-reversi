//! Time allocation for a whole game. Protocols own the clock and reply margin.
use crate::stop;
use deft_reversi_engine::{
    Action, Engine, EngineError, Goal, Position, Progress, ResultKind, SearchRequest, SearchResult,
    Selectivity,
};
use std::time::Duration;

#[derive(Clone, Copy)]
pub(crate) struct Budget {
    soft: Duration,
    pub hard: Duration,
}

impl Budget {
    pub fn new(empties: u8, remaining: Duration, cap: Duration) -> Self {
        // Full solves in the level-table measurements cost about 6 ms at
        // 17 empties, doubling per empty. Only price *future* turns with
        // this prior: today's position can be much harder than the average.
        let turns = u32::from(empties.div_ceil(2)).max(1);
        let mut pool = remaining;
        let mut turns_left = turns;
        for distance in (1..turns).rev() {
            let future = i32::from(empties) - 2 * distance as i32;
            // Allow for slower hardware, cold tables and the spread of solve
            // costs. Even a trivial turn needs a round trip on an integer-ms clock.
            let solve = Duration::from_millis(2).saturating_add(Duration::from_secs_f64(
                3.0 * 0.006 * 2_f64.powi(future - 17),
            ));
            if solve >= pool / turns_left {
                break;
            }
            pool -= solve;
            turns_left -= 1;
        }
        let share = pool / turns_left;
        // Borrow up to one share, leaving at least half the projected
        // future budget. Already discounted exact solves are fully reserved.
        let hard = share.saturating_add(share.min((pool - share) / 2)).min(cap);
        Self {
            soft: share.min(hard),
            hard,
        }
    }

    pub fn search(
        self,
        engine: &mut Engine,
        pos: Position,
        mut report: impl FnMut(&Progress),
    ) -> Result<SearchResult, EngineError> {
        let request = SearchRequest::timed_levels(60, self.hard)
            .reuse_pv()
            .skip_pv_recovery();
        let handle = request.stop_handle();
        let _armed = stop::arm(handle.clone());
        let mut forecast = Forecast::default();
        engine.search_with(pos, &request, |p| {
            report(p);
            if forecast.stop(p.elapsed, p.kind, p.best_move, self) {
                handle.stop();
            }
        })
    }
}

/// A two-ply search for a clock too short to budget: it takes microseconds
/// and avoids blunders that the lowest legal square would make.
pub(crate) fn reflex(engine: &mut Engine, pos: Position) -> Result<SearchResult, EngineError> {
    engine.search(pos, &SearchRequest::new(Goal::Depth(2)).skip_pv_recovery())
}

#[derive(Default)]
struct Forecast {
    elapsed: Duration,
    last_cost: Duration,
    best: Option<Action>,
    instability: f64,
}

impl Forecast {
    fn stop(
        &mut self,
        elapsed: Duration,
        kind: ResultKind,
        best: Option<Action>,
        budget: Budget,
    ) -> bool {
        let cost = elapsed.saturating_sub(self.elapsed);
        let growth = match kind {
            // Tree growth between successive two-ply iterations. Clamp
            // noisy ratios (TT hits and cheap shallow iterations).
            ResultKind::Depth { .. } if !self.last_cost.is_zero() => {
                (cost.as_secs_f64() / self.last_cost.as_secs_f64()).clamp(1.5, 6.0)
            }
            ResultKind::Depth { .. } => 3.0,
            // Re-searching the same depth at a higher MPC confidence.
            ResultKind::Endgame {
                selectivity: Selectivity::Full,
            } => return false,
            ResultKind::Endgame { .. } => 1.6,
        };
        let changed = self.best.is_some() && self.best != best;
        // A reversal two plies ago still matters even if the latest
        // iteration confirmed the new move. Decay the evidence with each
        // confirmation, and renew it on another reversal.
        self.instability = if changed { 1.0 } else { self.instability * 0.5 };
        self.best = best;
        self.elapsed = elapsed;
        self.last_cost = cost;
        let target = budget.soft.as_secs_f64() * (1.0 + self.instability);
        let predicted = elapsed.as_secs_f64() + cost.as_secs_f64() * growth;
        predicted > target.min(budget.hard.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Color, EngineOptions, Model, PatternModel};

    /// Final disc difference for `me` under best play from both sides.
    fn exact(pos: Position, me: Color) -> i32 {
        if pos.is_terminal() {
            return pos.final_score(me);
        }
        if pos.must_pass() {
            return exact(pos.play(Action::Pass).unwrap(), me);
        }
        let values = pos
            .legal_moves()
            .map(|sq| exact(pos.play(Action::Place(sq)).unwrap(), me));
        if pos.side_to_move() == me {
            values.max().unwrap()
        } else {
            values.min().unwrap()
        }
    }

    #[test]
    fn the_reflex_move_is_not_the_lowest_square_when_that_loses() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let mut checked = 0;
        for game in 0..400_usize {
            let mut pos = Position::initial();
            let mut ply = 0;
            while pos.empty_count() > 2 && !pos.is_terminal() {
                if pos.must_pass() {
                    pos = pos.play(Action::Pass).unwrap();
                    continue;
                }
                let moves: Vec<_> = pos.legal_moves().collect();
                ply += 1;
                pos = pos
                    .play(Action::Place(moves[(game * 7 + ply * ply) % moves.len()]))
                    .unwrap();
            }
            if pos.is_terminal() || pos.must_pass() || pos.legal_moves().count() < 2 {
                continue;
            }
            let me = pos.side_to_move();
            let value = |sq| exact(pos.play(Action::Place(sq)).unwrap(), me);
            let best = pos.legal_moves().map(value).max().unwrap();
            let lowest = pos.legal_moves().next().unwrap();
            if value(lowest) == best {
                continue;
            }
            let Some(Action::Place(chosen)) = reflex(&mut engine, pos).unwrap().best_move else {
                panic!("no move");
            };
            // Two plies from two empties reach the end, so even the zero
            // model finds the best move.
            assert_eq!(value(chosen), best, "{pos:?}");
            checked += 1;
        }
        assert!(checked > 5, "only {checked} positions");
    }

    #[test]
    fn a_long_think_preserves_round_trips_for_cheap_future_solves() {
        let left = Duration::from_millis(80);
        let budget = Budget::new(12, left, left);
        assert!(left - budget.hard >= Duration::from_millis(10));
    }

    #[test]
    fn move_change_can_extend_soft_target_only_within_the_cap() {
        let ms = Duration::from_millis;
        let decision = |mv: &str, hard| {
            let mut forecast = Forecast {
                elapsed: ms(10),
                last_cost: ms(5),
                best: Some("d3".parse().unwrap()),
                instability: 0.0,
            };
            // Last step took 10 ms and doubled in cost. The projected
            // completion at 40 ms lies beyond the normal 32 ms share.
            forecast.stop(
                ms(20),
                ResultKind::Depth {
                    depth: 10,
                    selectivity: Selectivity::Percent83,
                },
                Some(mv.parse().unwrap()),
                Budget {
                    soft: ms(32),
                    hard: ms(hard),
                },
            )
        };
        assert!(decision("d3", 60));
        assert!(!decision("c4", 60));
        assert!(decision("c4", 35));
    }

    #[test]
    fn a_recent_reversal_keeps_some_time_for_confirmation() {
        let ms = Duration::from_millis;
        let budget = Budget {
            soft: ms(32),
            hard: ms(64),
        };
        let depth = |depth| ResultKind::Depth {
            depth,
            selectivity: Selectivity::Percent83,
        };
        let mut forecast = Forecast::default();
        let first = Some("d3".parse().unwrap());
        let second = Some("c4".parse().unwrap());
        assert!(!forecast.stop(ms(4), depth(6), first, budget));
        assert!(!forecast.stop(ms(10), depth(8), second, budget));
        // 20 + 10 * (10 / 6) exceeds the ordinary share, but fits
        // after a single confirmation of the new best move.
        assert!(!forecast.stop(ms(20), depth(10), second, budget));
        assert!(forecast.stop(ms(35), depth(12), second, budget));
    }

    #[test]
    fn future_exact_solves_release_time_as_the_clock_grows() {
        let share = |sec| {
            let left = Duration::from_secs(sec);
            Budget::new(40, left, left).soft.as_secs_f64() / sec as f64
        };
        assert!(share(1) > 1.0 / 20.0);
        assert!(share(60) > share(1));
    }

    #[test]
    fn protocol_cap_and_future_turns_bound_borrowing() {
        let left = Duration::from_secs(1);
        let cap = Duration::from_millis(5);
        for empty in 1..=60 {
            let b = Budget::new(empty, left, cap);
            assert!(b.soft <= b.hard && b.hard <= cap);
            assert_eq!(Budget::new(empty, Duration::ZERO, cap).hard, Duration::ZERO);
        }
        assert!(Budget::new(22, left, left).hard < left);
        assert_eq!(Budget::new(1, left, left).soft, left);
    }
}
