//! The four parts of the game clock. Each can be replaced on its own and
//! compared with the others fixed:
//!
//! - [`CostModel`]: how long the next iteration will take (a distribution).
//! - [`ValueModel`]: how much the next iteration is worth.
//! - [`ClockModel`]: what one second of this turn may cost the rest of the
//!   game, as a soft target and a hard limit.
//! - [`StopPolicy`]: whether to start the next iteration, from the other three.
use super::context::GameContext;
use deft_reversi_engine::{Action, ResultKind, Selectivity};
use serde::Serialize;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Protocol {
    Gtp,
    FlipSight,
}

/// The clock at the start of a turn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClockState {
    pub empties: u8,
    /// Time left for this side, after the protocol's reply margin.
    pub remaining: Duration,
    /// The most this turn may take: the protocol's per-move limit, or
    /// `remaining` where there is none.
    pub cap: Duration,
    pub protocol: Protocol,
    pub threads: usize,
}

/// One finished iteration of the search.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Observation<'a> {
    pub empties: u8,
    /// Search time so far.
    pub elapsed: Duration,
    /// This iteration's own time.
    pub cost: Duration,
    pub kind: ResultKind,
    /// The iteration the search runs next; None after the last one.
    pub next: Option<ResultKind>,
    pub best_move: Option<Action>,
    // Not used by the legacy parts; there for models fitted on the log.
    #[allow(dead_code)]
    pub score: i32,
    #[allow(dead_code)]
    pub nodes: u64,
    #[allow(dead_code)]
    pub pv: &'a [Action],
}

/// The predicted time of an iteration: log-normal with this median and
/// standard deviation of the log. A zero `log_sigma` is a point estimate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CostEstimate {
    /// Seconds.
    pub median: f64,
    pub log_sigma: f64,
}

impl CostEstimate {
    /// The chance that the iteration ends within `seconds` of its start.
    pub fn p_finish(&self, seconds: f64) -> f64 {
        if seconds <= 0.0 {
            return 0.0;
        }
        if self.median <= 0.0 {
            return 1.0;
        }
        if self.log_sigma <= 0.0 {
            return if seconds >= self.median { 1.0 } else { 0.0 };
        }
        normal_cdf((seconds / self.median).ln() / self.log_sigma)
    }
}

/// Standard normal distribution function (Abramowitz and Stegun 7.1.26,
/// absolute error below 1e-7).
fn normal_cdf(z: f64) -> f64 {
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erfc = poly * (-x * x).exp();
    if z >= 0.0 {
        1.0 - erfc / 2.0
    } else {
        erfc / 2.0
    }
}

/// The worth of running the next iteration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GainEstimate {
    /// Expected discs gained (loss avoided), when the model predicts it.
    pub expected_discs: Option<f64>,
    /// Evidence that the best move is still moving: 1 right after a change,
    /// decaying with each confirmation.
    pub instability: f64,
}

/// What this turn may spend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hurdle {
    /// The usual time for this turn.
    pub soft: Duration,
    /// The time the search must not pass.
    pub hard: Duration,
    /// Discs a second must buy to be worth taking from the rest of the game,
    /// when the model prices time.
    pub price: Option<f64>,
}

/// Iteration transitions, as the cost of the next iteration depends on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Transition {
    /// Midgame depth to a deeper one.
    Mid,
    /// Midgame to the first endgame stage (selective or exact).
    Entry,
    /// Selective endgame stage to a more confident selective one.
    Stage,
    /// Selective endgame stage to the exact solve.
    Exact,
    /// Endgame back to the midgame (plans do not do this).
    Back,
}

impl Transition {
    pub fn of(kind: ResultKind, next: ResultKind) -> Self {
        match (kind, next) {
            (ResultKind::Depth { .. }, ResultKind::Depth { .. }) => Self::Mid,
            (ResultKind::Depth { .. }, ResultKind::Endgame { .. }) => Self::Entry,
            (
                ResultKind::Endgame { .. },
                ResultKind::Endgame {
                    selectivity: Selectivity::Full,
                },
            ) => Self::Exact,
            (ResultKind::Endgame { .. }, ResultKind::Endgame { .. }) => Self::Stage,
            (ResultKind::Endgame { .. }, ResultKind::Depth { .. }) => Self::Back,
        }
    }
}

/// Predicts the next iteration's time. Called once after every finished
/// iteration of a search, in order; a new model is made for each search.
pub(crate) trait CostModel {
    fn next(&mut self, obs: &Observation, ctx: &GameContext) -> Option<CostEstimate>;
}

/// Predicts the worth of the next iteration. Called like [`CostModel`].
pub(crate) trait ValueModel {
    fn gain(&mut self, obs: &Observation, ctx: &GameContext) -> GainEstimate;
}

/// Sets the turn's time from the clock, once at the start of a turn.
pub(crate) trait ClockModel {
    fn hurdle(&self, clock: &ClockState, ctx: &GameContext) -> Hurdle;
}

/// Whether to stop instead of running the next iteration.
pub(crate) trait StopPolicy {
    fn stop(
        &self,
        obs: &Observation,
        cost: Option<CostEstimate>,
        gain: GainEstimate,
        hurdle: Hurdle,
    ) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_chance_follows_the_median_and_spread() {
        let point = CostEstimate {
            median: 2.0,
            log_sigma: 0.0,
        };
        assert_eq!(point.p_finish(1.9), 0.0);
        assert_eq!(point.p_finish(2.0), 1.0);
        let spread = CostEstimate {
            median: 2.0,
            log_sigma: 0.6,
        };
        assert!((spread.p_finish(2.0) - 0.5).abs() < 1e-6);
        // One sigma above the median in log space.
        assert!((spread.p_finish(2.0 * 0.6_f64.exp()) - 0.841_344_7).abs() < 1e-6);
        assert!(spread.p_finish(1.0) < 0.5 && spread.p_finish(4.0) > 0.5);
        assert_eq!(spread.p_finish(0.0), 0.0);
    }
}
