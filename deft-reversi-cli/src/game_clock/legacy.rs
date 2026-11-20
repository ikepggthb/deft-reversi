//! The game clock as of 0c53a14, split into the four parts. Decisions are
//! identical to that version (a test in `mod.rs` compares them with a copy).
use super::{
    context::GameContext,
    parts::{
        ClockModel, ClockState, CostEstimate, CostModel, GainEstimate, Hurdle, Observation,
        StopPolicy, ValueModel,
    },
};
use deft_reversi_engine::{Action, ResultKind, Selectivity};
use std::time::Duration;

/// Equal shares of the time left after reserving future exact solves.
pub(crate) struct LegacyClock;

impl ClockModel for LegacyClock {
    fn hurdle(&self, clock: &ClockState, _ctx: &GameContext) -> Hurdle {
        let (soft, hard) = budget(clock.empties, clock.remaining, clock.cap);
        Hurdle {
            soft,
            hard,
            price: None,
        }
    }
}

/// (soft, hard) for a turn with `empties` empties.
pub(crate) fn budget(empties: u8, remaining: Duration, cap: Duration) -> (Duration, Duration) {
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
    (share.min(hard), hard)
}

/// The next iteration's time as the last one's times a ratio by transition.
#[derive(Default)]
pub(crate) struct LegacyCost {
    last_cost: Duration,
    last_depth: Option<u8>,
}

impl CostModel for LegacyCost {
    fn next(&mut self, obs: &Observation, _ctx: &GameContext) -> Option<CostEstimate> {
        let growth = obs
            .next
            .map(|next| self.growth(obs.empties, obs.kind, next, obs.cost));
        self.last_cost = obs.cost;
        self.last_depth = match obs.kind {
            ResultKind::Depth { depth, .. } => Some(depth),
            ResultKind::Endgame { .. } => None,
        };
        growth.map(|growth| CostEstimate {
            median: obs.cost.as_secs_f64() * growth,
            log_sigma: 0.0,
        })
    }
}

impl LegacyCost {
    /// The state after an iteration of `last_cost` at `last_depth`.
    #[cfg(test)]
    pub fn after(last_cost: Duration, last_depth: Option<u8>) -> Self {
        Self {
            last_cost,
            last_depth,
        }
    }

    /// Predicted cost of `next` over the cost of `kind`, which just took `cost`.
    fn growth(&self, empties: u8, kind: ResultKind, next: ResultKind, cost: Duration) -> f64 {
        let empties = f64::from(empties);
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
pub(crate) struct Ratio {
    pub floor: f64,
    base: f64,
    pivot: f64,
    per_empty: f64,
}

impl Ratio {
    pub fn at(&self, empties: f64) -> f64 {
        (self.base * self.per_empty.powf(empties - self.pivot)).max(self.floor)
    }
}

// Median ratios measured on cold tables (probe, 1 and 4 threads) and in
// games (DEFT_GTP_LOG), see docs/benchmarks/2026-11-20-time-control-forecast.
// The soft target leaves room up to the hard limit for the upper tail. The
// thread count barely changes the medians.
/// Last midgame depth -> first endgame stage: about 5 from 20 to 31
/// empties, less with few empties and a warm table.
pub(crate) const ENTRY: Ratio = Ratio {
    floor: 2.0,
    base: 5.0,
    pivot: 24.0,
    per_empty: 1.06,
};
/// Last selective endgame stage -> exact solve: 2.5-3.4 at 20-23 empties,
/// 4-5 at 24-27, 6 or more from 28.
pub(crate) const EXACT: Ratio = Ratio {
    floor: 1.0,
    base: 3.0,
    pivot: 22.0,
    per_empty: 1.15,
};
/// Selective endgame stage -> the next one: medians 1.0-1.25.
pub(crate) const STAGE: f64 = 1.25;

/// Instability: 1 when the best move changed, halved on each confirmation.
#[derive(Default)]
pub(crate) struct LegacyValue {
    best: Option<Action>,
    instability: f64,
}

impl LegacyValue {
    /// The state after an iteration that chose `best`.
    #[cfg(test)]
    pub fn after(best: Option<Action>) -> Self {
        Self {
            best,
            instability: 0.0,
        }
    }
}

impl ValueModel for LegacyValue {
    fn gain(&mut self, obs: &Observation, _ctx: &GameContext) -> GainEstimate {
        let changed = self.best.is_some() && self.best != obs.best_move;
        // A reversal two plies ago still matters even if the latest
        // iteration confirmed the new move. Decay the evidence with each
        // confirmation, and renew it on another reversal.
        self.instability = if changed { 1.0 } else { self.instability * 0.5 };
        self.best = obs.best_move;
        GainEstimate {
            expected_discs: None,
            instability: self.instability,
        }
    }
}

/// Stop when the next iteration would end beyond soft (extended by the
/// instability) or hard.
pub(crate) struct LegacyStop;

impl StopPolicy for LegacyStop {
    fn stop(
        &self,
        obs: &Observation,
        cost: Option<CostEstimate>,
        gain: GainEstimate,
        hurdle: Hurdle,
    ) -> bool {
        // The last iteration: nothing left to forecast.
        let Some(cost) = cost else {
            return false;
        };
        let target = hurdle.soft.as_secs_f64() * (1.0 + gain.instability);
        let predicted = obs.elapsed.as_secs_f64() + cost.median;
        predicted > target.min(hurdle.hard.as_secs_f64())
    }
}
