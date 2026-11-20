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
    pub fn new(empties: u8, remaining: Duration, cap: Duration, threads: usize) -> Self {
        // Full solves in the level-table measurements cost about 6 ms at
        // 17 empties, doubling per empty, on one thread. Four threads solve
        // 20-30 empties about 3.3 times faster (4^0.86). Only price *future*
        // turns with this prior: today's position can be much harder than
        // the average.
        let speedup = (threads.max(1) as f64).powf(0.85);
        let turns = u32::from(empties.div_ceil(2)).max(1);
        let mut pool = remaining;
        let mut turns_left = turns;
        for distance in (1..turns).rev() {
            let future = i32::from(empties) - 2 * distance as i32;
            // Allow for slower hardware, cold tables and the spread of solve
            // costs. Even a trivial turn needs a round trip on an integer-ms clock.
            let solve = Duration::from_millis(2).saturating_add(Duration::from_secs_f64(
                3.0 * 0.006 * 2_f64.powi(future - 17) / speedup,
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
        let mut forecast = Forecast {
            empties: pos.empty_count(),
            ..Forecast::default()
        };
        engine.search_with(pos, &request, |p| {
            report(p);
            if forecast.stop(p.elapsed, p.kind, p.next, p.best_move, self) {
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
    empties: u8,
    elapsed: Duration,
    last_cost: Duration,
    last_depth: Option<u8>,
    best: Option<Action>,
    instability: f64,
}

impl Forecast {
    /// After an iteration finishes: whether the planned `next` one would
    /// end beyond the budget, so the search should stop now.
    fn stop(
        &mut self,
        elapsed: Duration,
        kind: ResultKind,
        next: Option<ResultKind>,
        best: Option<Action>,
        budget: Budget,
    ) -> bool {
        let cost = elapsed.saturating_sub(self.elapsed);
        let growth = next.map(|next| self.growth(kind, next, cost));
        let changed = self.best.is_some() && self.best != best;
        // A reversal two plies ago still matters even if the latest
        // iteration confirmed the new move. Decay the evidence with each
        // confirmation, and renew it on another reversal.
        self.instability = if changed { 1.0 } else { self.instability * 0.5 };
        self.best = best;
        self.elapsed = elapsed;
        self.last_cost = cost;
        self.last_depth = match kind {
            ResultKind::Depth { depth, .. } => Some(depth),
            ResultKind::Endgame { .. } => None,
        };
        // The last iteration: nothing left to forecast.
        let Some(growth) = growth else {
            return false;
        };
        let target = budget.soft.as_secs_f64() * (1.0 + self.instability);
        let predicted = elapsed.as_secs_f64() + cost.as_secs_f64() * growth;
        predicted > target.min(budget.hard.as_secs_f64())
    }

    /// Predicted cost of `next` over the cost of `kind`, which just took `cost`.
    fn growth(&self, kind: ResultKind, next: ResultKind, cost: Duration) -> f64 {
        let empties = f64::from(self.empties);
        match (kind, next) {
            (ResultKind::Depth { depth, .. }, ResultKind::Depth { depth: deeper, .. }) => {
                // Tree growth per two plies, from the last two iterations.
                // Clamp noisy ratios (TT hits and cheap shallow iterations).
                let per_two_plies = match self.last_depth {
                    Some(previous) if !self.last_cost.is_zero() && depth > previous => {
                        let ratio = cost.as_secs_f64() / self.last_cost.as_secs_f64();
                        ratio
                            .powf(2.0 / f64::from(depth - previous))
                            .clamp(1.5, 6.0)
                    }
                    _ => 3.0,
                };
                per_two_plies.powf(f64::from(deeper.saturating_sub(depth)) / 2.0)
            }
            // The first endgame stage searches to the end of the game, far
            // deeper than the last midgame depth.
            (
                ResultKind::Depth { .. },
                ResultKind::Endgame {
                    selectivity: Selectivity::Full,
                },
            ) => ENTRY.at(empties) * EXACT.at(empties),
            (ResultKind::Depth { .. }, ResultKind::Endgame { .. }) => ENTRY.at(empties),
            (
                ResultKind::Endgame { .. },
                ResultKind::Endgame {
                    selectivity: Selectivity::Full,
                },
            ) => EXACT.at(empties),
            // Re-searching the same depth at a higher MPC confidence.
            (ResultKind::Endgame { .. }, ResultKind::Endgame { .. }) => STAGE,
            // Plans never return to the midgame; be cautious if one does.
            (ResultKind::Endgame { .. }, ResultKind::Depth { .. }) => ENTRY.at(empties),
        }
    }
}

/// A cost ratio that grows geometrically with the empties, from `base` at
/// `pivot` empties, and never falls below `floor`.
struct Ratio {
    floor: f64,
    base: f64,
    pivot: f64,
    per_empty: f64,
}

impl Ratio {
    fn at(&self, empties: f64) -> f64 {
        (self.base * self.per_empty.powf(empties - self.pivot)).max(self.floor)
    }
}

// Median ratios measured on cold tables (probe, 1 and 4 threads) and in
// games (DEFT_GTP_LOG), see docs/benchmarks/2026-11-20-time-control-forecast.
// The soft target leaves room up to the hard limit for the upper tail. The
// thread count barely changes the medians.
/// Last midgame depth -> first endgame stage: about 5 from 20 to 31
/// empties, less with few empties and a warm table.
const ENTRY: Ratio = Ratio {
    floor: 2.0,
    base: 5.0,
    pivot: 24.0,
    per_empty: 1.06,
};
/// Last selective endgame stage -> exact solve: 2.5-3.4 at 20-23 empties,
/// 4-5 at 24-27, 6 or more from 28.
const EXACT: Ratio = Ratio {
    floor: 1.0,
    base: 3.0,
    pivot: 22.0,
    per_empty: 1.15,
};
/// Selective endgame stage -> the next one: medians 1.0-1.25.
const STAGE: f64 = 1.25;

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
        let budget = Budget::new(12, left, left, 1);
        assert!(left - budget.hard >= Duration::from_millis(10));
    }

    #[test]
    fn move_change_can_extend_soft_target_only_within_the_cap() {
        let ms = Duration::from_millis;
        let decision = |mv: &str, hard| {
            let mut forecast = Forecast {
                elapsed: ms(10),
                last_cost: ms(5),
                last_depth: Some(8),
                best: Some("d3".parse().unwrap()),
                ..Forecast::default()
            };
            // Last step took 10 ms and doubled in cost. The projected
            // completion at 40 ms lies beyond the normal 32 ms share.
            forecast.stop(
                ms(20),
                ResultKind::Depth {
                    depth: 10,
                    selectivity: Selectivity::Percent83,
                },
                Some(ResultKind::Depth {
                    depth: 12,
                    selectivity: Selectivity::Percent83,
                }),
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
        let next = |d: u8| Some(depth(d + 2));
        assert!(!forecast.stop(ms(4), depth(6), next(6), first, budget));
        assert!(!forecast.stop(ms(10), depth(8), next(8), second, budget));
        // 20 + 10 * (10 / 6) exceeds the ordinary share, but fits
        // after a single confirmation of the new best move.
        assert!(!forecast.stop(ms(20), depth(10), next(10), second, budget));
        assert!(forecast.stop(ms(35), depth(12), next(12), second, budget));
    }

    const fn stage(selectivity: Selectivity) -> ResultKind {
        ResultKind::Endgame { selectivity }
    }

    /// Whether a forecast that saw the midgame grow 1.5x per two plies
    /// (4 ms, then 6 ms ending at 12 ms) stops before `next`.
    fn after_midgame(empties: u8, next: Option<ResultKind>) -> bool {
        let ms = Duration::from_millis;
        let mut forecast = Forecast {
            empties,
            elapsed: ms(6),
            last_cost: ms(4),
            last_depth: Some(14),
            ..Forecast::default()
        };
        let depth = ResultKind::Depth {
            depth: 16,
            selectivity: Selectivity::Percent83,
        };
        let budget = Budget {
            soft: ms(32),
            hard: ms(64),
        };
        forecast.stop(ms(12), depth, next, None, budget)
    }

    #[test]
    fn the_first_endgame_stage_is_not_priced_like_a_midgame_step() {
        let deeper = ResultKind::Depth {
            depth: 18,
            selectivity: Selectivity::Percent83,
        };
        // 12 + 6 * 1.5 fits in the 32 ms share, 12 + 6 * 4.4 does not.
        assert!(!after_midgame(22, Some(deeper)));
        assert!(after_midgame(22, Some(stage(Selectivity::Percent85))));
        assert!(after_midgame(22, Some(stage(Selectivity::Full))));
    }

    #[test]
    fn the_last_iteration_has_nothing_to_forecast() {
        let ms = Duration::from_millis;
        let mut forecast = Forecast::default();
        let budget = Budget {
            soft: ms(1),
            hard: ms(2),
        };
        assert!(!forecast.stop(ms(50), stage(Selectivity::Full), None, None, budget));
        assert!(!forecast.stop(ms(90), stage(Selectivity::Percent99), None, None, budget));
    }

    #[test]
    fn an_exact_solve_costs_more_than_another_selective_stage() {
        let ms = Duration::from_millis;
        let budget = Budget {
            soft: ms(32),
            hard: ms(64),
        };
        let decide = |next| {
            let mut forecast = Forecast {
                empties: 22,
                elapsed: ms(12),
                ..Forecast::default()
            };
            // The 99% stage took 8 ms, ending at 20 ms.
            forecast.stop(
                ms(20),
                stage(Selectivity::Percent99),
                Some(next),
                None,
                budget,
            )
        };
        assert!(!decide(stage(Selectivity::Percent99)));
        assert!(decide(stage(Selectivity::Full)));
    }

    #[test]
    fn transition_ratios_grow_with_the_empties_above_their_floor() {
        for ratio in [&ENTRY, &EXACT] {
            assert!(ratio.at(10.0) >= ratio.floor);
            assert!(ratio.at(30.0) > ratio.at(24.0));
        }
        assert!(EXACT.at(22.0) > STAGE);
    }

    #[test]
    fn future_exact_solves_release_time_as_the_clock_grows() {
        let share = |sec| {
            let left = Duration::from_secs(sec);
            Budget::new(40, left, left, 1).soft.as_secs_f64() / sec as f64
        };
        assert!(share(1) > 1.0 / 20.0);
        assert!(share(60) > share(1));
    }

    #[test]
    fn more_threads_reserve_less_for_future_solves() {
        let left = Duration::from_secs(2);
        let share = |threads| Budget::new(30, left, left, threads).soft;
        assert!(share(4) > share(1));
        assert_eq!(share(1), share(0));
    }

    #[test]
    fn protocol_cap_and_future_turns_bound_borrowing() {
        let left = Duration::from_secs(1);
        let cap = Duration::from_millis(5);
        for empty in 1..=60 {
            let b = Budget::new(empty, left, cap, 1);
            assert!(b.soft <= b.hard && b.hard <= cap);
            assert_eq!(
                Budget::new(empty, Duration::ZERO, cap, 1).hard,
                Duration::ZERO
            );
        }
        assert!(Budget::new(22, left, left, 1).hard < left);
        assert_eq!(Budget::new(1, left, left, 1).soft, left);
    }
}
