//! Time allocation for a whole game. Protocols own the clock and reply margin.
use crate::stop;
use deft_reversi_engine::{
    Action, Engine, EngineError, Position, Progress, ResultKind, SearchRequest, SearchResult,
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
        let request = SearchRequest::timed_levels(60, self.hard).skip_pv_recovery();
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

#[derive(Default)]
struct Forecast {
    elapsed: Duration,
    last_cost: Duration,
    best: Option<Action>,
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
        self.best = best;
        self.elapsed = elapsed;
        self.last_cost = cost;
        let target = budget.soft.as_secs_f64() * if changed { 1.4 } else { 1.0 };
        let predicted = elapsed.as_secs_f64() + cost.as_secs_f64() * growth;
        predicted > target.min(budget.hard.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
