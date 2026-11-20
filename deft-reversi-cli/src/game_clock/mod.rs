//! Time allocation for a whole game. Protocols own the clock and reply margin.
//!
//! The decisions come from four replaceable parts (`parts`): a cost model,
//! a value model, a clock model and a stopping policy. `legacy` holds the
//! current ones. `GameContext` carries what a side remembers between turns.
mod context;
mod legacy;
mod parts;

pub(crate) use context::GameContext;
use context::TurnSummary;
use legacy::{LegacyClock, LegacyCost, LegacyStop, LegacyValue};
use parts::{
    ClockModel, CostEstimate, CostModel, GainEstimate, Hurdle, Observation, StopPolicy, Transition,
    ValueModel,
};
pub(crate) use parts::{ClockState, Protocol};

use crate::stop;
use deft_reversi_engine::{
    Action, Engine, EngineError, Goal, Position, Progress, ResultKind, SearchRequest, SearchResult,
    StopReason,
};
use std::time::{Duration, Instant};

/// The tag of an iteration in logs: `d22` for a midgame depth, `e85` for
/// an endgame stage (`e100` is the exact solve).
pub(crate) fn tag(kind: &ResultKind) -> String {
    match kind {
        ResultKind::Depth { depth, .. } => format!("d{depth}"),
        ResultKind::Endgame { selectivity } => format!("e{}", selectivity.percent()),
    }
}

/// One turn's time, from the clock model.
#[derive(Clone, Copy)]
pub(crate) struct Budget {
    pub clock: ClockState,
    hurdle: Hurdle,
}

impl Budget {
    pub fn new(clock: ClockState, ctx: &GameContext) -> Self {
        Self {
            clock,
            hurdle: LegacyClock.hurdle(&clock, ctx),
        }
    }

    /// The time the search must not pass.
    pub fn hard(&self) -> Duration {
        self.hurdle.hard
    }

    pub fn search(
        &self,
        engine: &mut Engine,
        pos: Position,
        ctx: &mut GameContext,
        mut report: impl FnMut(&Progress),
    ) -> Result<SearchResult, EngineError> {
        let start = Instant::now();
        let request = SearchRequest::timed_levels(60, self.hurdle.hard)
            .reuse_pv()
            .skip_pv_recovery();
        let handle = request.stop_handle();
        let _armed = stop::arm(handle.clone());
        let mut forecast = Forecast::legacy(pos.empty_count());
        let mut forecast_stopped = false;
        let hurdle = self.hurdle;
        let mut result = engine.search_with(pos, &request, |p| {
            report(p);
            if forecast.observe(&Finished::from(p), ctx, hurdle).stop {
                forecast_stopped = true;
                handle.stop();
            }
        })?;
        // The forecast stops through the same handle as Ctrl-C. Report its
        // stops as the time decisions they are, so logs can tell them apart.
        if forecast_stopped && result.stop_reason == StopReason::Requested {
            result.stop_reason = StopReason::TimeLimit;
        }
        ctx.end_turn(TurnSummary {
            position: pos,
            used: start.elapsed(),
            reached: result.kind,
            pv: result.pv.clone(),
        });
        Ok(result)
    }
}

/// A turn decided without a timed search (a forced move or `reflex`).
pub(crate) fn unsearched_turn(ctx: &mut GameContext, position: Position, used: Duration) {
    ctx.end_turn(TurnSummary {
        position,
        used,
        reached: None,
        pv: Vec::new(),
    });
}

/// A two-ply search for a clock too short to budget: it takes microseconds
/// and avoids blunders that the lowest legal square would make.
pub(crate) fn reflex(engine: &mut Engine, pos: Position) -> Result<SearchResult, EngineError> {
    engine.search(pos, &SearchRequest::new(Goal::Depth(2)).skip_pv_recovery())
}

/// A finished iteration, as `Progress` reports it.
struct Finished<'a> {
    elapsed: Duration,
    kind: ResultKind,
    next: Option<ResultKind>,
    best_move: Option<Action>,
    score: i32,
    nodes: u64,
    pv: &'a [Action],
}

impl<'a> From<&'a Progress> for Finished<'a> {
    fn from(p: &'a Progress) -> Self {
        Self {
            elapsed: p.elapsed,
            kind: p.kind,
            next: p.next,
            best_move: p.best_move,
            score: p.score,
            nodes: p.nodes,
            pv: &p.pv,
        }
    }
}

/// What the forecast concluded after one iteration.
struct Step {
    stop: bool,
}

/// Runs the cost model, value model and stopping policy after every
/// finished iteration of one search.
struct Forecast {
    empties: u8,
    elapsed: Duration,
    cost: Box<dyn CostModel>,
    value: Box<dyn ValueModel>,
    policy: Box<dyn StopPolicy>,
    /// The transition and predicted seconds of the iteration now running.
    pending: Option<(Transition, f64)>,
}

impl Forecast {
    fn legacy(empties: u8) -> Self {
        Self::with(
            empties,
            Box::new(LegacyCost::default()),
            Box::new(LegacyValue::default()),
        )
    }

    fn with(empties: u8, cost: Box<dyn CostModel>, value: Box<dyn ValueModel>) -> Self {
        Self {
            empties,
            elapsed: Duration::ZERO,
            cost,
            value,
            policy: Box::new(LegacyStop),
            pending: None,
        }
    }

    fn observe(&mut self, done: &Finished, ctx: &mut GameContext, hurdle: Hurdle) -> Step {
        let cost = done.elapsed.saturating_sub(self.elapsed);
        self.elapsed = done.elapsed;
        if let Some((transition, predicted)) = self.pending.take()
            && predicted > 0.0
            && !cost.is_zero()
        {
            ctx.record_miss(transition, (cost.as_secs_f64() / predicted).ln());
        }
        let obs = Observation {
            empties: self.empties,
            elapsed: done.elapsed,
            cost,
            kind: done.kind,
            next: done.next,
            best_move: done.best_move,
            score: done.score,
            nodes: done.nodes,
            pv: done.pv,
        };
        let estimate: Option<CostEstimate> = self.cost.next(&obs, ctx);
        let gain: GainEstimate = self.value.gain(&obs, ctx);
        let stop = self.policy.stop(&obs, estimate, gain, hurdle);
        self.pending = done
            .next
            .zip(estimate)
            .map(|(next, e)| (Transition::of(done.kind, next), e.median));
        Step { stop }
    }
}

#[cfg(test)]
mod tests {
    use super::legacy::{ENTRY, EXACT, STAGE, budget};
    use super::*;
    use deft_reversi_engine::{Color, EngineOptions, Model, PatternModel, Selectivity};

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

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn hurdle(soft: u64, hard: u64) -> Hurdle {
        Hurdle {
            soft: ms(soft),
            hard: ms(hard),
            price: None,
        }
    }

    fn depth(depth: u8) -> ResultKind {
        ResultKind::Depth {
            depth,
            selectivity: Selectivity::Percent83,
        }
    }

    const fn stage(selectivity: Selectivity) -> ResultKind {
        ResultKind::Endgame { selectivity }
    }

    /// A forecast in the state the legacy one would have after an
    /// iteration of `last_cost` (at `last_depth`) that ended at `elapsed`
    /// with `best` as its move.
    fn forecast_after(
        empties: u8,
        elapsed: Duration,
        last_cost: Duration,
        last_depth: Option<u8>,
        best: Option<&str>,
    ) -> Forecast {
        let mut f = Forecast::with(
            empties,
            Box::new(LegacyCost::after(last_cost, last_depth)),
            Box::new(LegacyValue::after(best.map(|m| m.parse().unwrap()))),
        );
        f.elapsed = elapsed;
        f
    }

    fn decide(
        f: &mut Forecast,
        elapsed: Duration,
        kind: ResultKind,
        next: Option<ResultKind>,
        best: Option<&str>,
        h: Hurdle,
    ) -> bool {
        let done = Finished {
            elapsed,
            kind,
            next,
            best_move: best.map(|m| m.parse().unwrap()),
            score: 0,
            nodes: 0,
            pv: &[],
        };
        f.observe(&done, &mut GameContext::default(), h).stop
    }

    #[test]
    fn a_long_think_preserves_round_trips_for_cheap_future_solves() {
        let left = ms(80);
        let (_, hard) = budget(12, left, left);
        assert!(left - hard >= ms(10));
    }

    #[test]
    fn move_change_can_extend_soft_target_only_within_the_cap() {
        let decision = |mv: &str, hard| {
            let mut f = forecast_after(0, ms(10), ms(5), Some(8), Some("d3"));
            // Last step took 10 ms and doubled in cost. The projected
            // completion at 40 ms lies beyond the normal 32 ms share.
            decide(
                &mut f,
                ms(20),
                depth(10),
                Some(depth(12)),
                Some(mv),
                hurdle(32, hard),
            )
        };
        assert!(decision("d3", 60));
        assert!(!decision("c4", 60));
        assert!(decision("c4", 35));
    }

    #[test]
    fn a_recent_reversal_keeps_some_time_for_confirmation() {
        let h = hurdle(32, 64);
        let mut f = Forecast::legacy(0);
        let next = |d: u8| Some(depth(d + 2));
        assert!(!decide(&mut f, ms(4), depth(6), next(6), Some("d3"), h));
        assert!(!decide(&mut f, ms(10), depth(8), next(8), Some("c4"), h));
        // 20 + 10 * (10 / 6) exceeds the ordinary share, but fits
        // after a single confirmation of the new best move.
        assert!(!decide(&mut f, ms(20), depth(10), next(10), Some("c4"), h));
        assert!(decide(&mut f, ms(35), depth(12), next(12), Some("c4"), h));
    }

    /// Whether a forecast that saw the midgame grow 1.5x per two plies
    /// (4 ms, then 6 ms ending at 12 ms) stops before `next`.
    fn after_midgame(empties: u8, next: Option<ResultKind>) -> bool {
        let mut f = forecast_after(empties, ms(6), ms(4), Some(14), None);
        decide(&mut f, ms(12), depth(16), next, None, hurdle(32, 64))
    }

    #[test]
    fn the_first_endgame_stage_is_not_priced_like_a_midgame_step() {
        // 12 + 6 * 1.5 fits in the 32 ms share, 12 + 6 * 4.4 does not.
        assert!(!after_midgame(22, Some(depth(18))));
        assert!(after_midgame(22, Some(stage(Selectivity::Percent85))));
        assert!(after_midgame(22, Some(stage(Selectivity::Full))));
    }

    #[test]
    fn the_last_iteration_has_nothing_to_forecast() {
        let h = hurdle(1, 2);
        let mut f = Forecast::legacy(0);
        assert!(!decide(
            &mut f,
            ms(50),
            stage(Selectivity::Full),
            None,
            None,
            h
        ));
        assert!(!decide(
            &mut f,
            ms(90),
            stage(Selectivity::Percent99),
            None,
            None,
            h
        ));
    }

    #[test]
    fn an_exact_solve_costs_more_than_another_selective_stage() {
        let decide_next = |next| {
            let mut f = forecast_after(22, ms(12), Duration::ZERO, None, None);
            // The 99% stage took 8 ms, ending at 20 ms.
            decide(
                &mut f,
                ms(20),
                stage(Selectivity::Percent99),
                Some(next),
                None,
                hurdle(32, 64),
            )
        };
        assert!(!decide_next(stage(Selectivity::Percent99)));
        assert!(decide_next(stage(Selectivity::Full)));
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
            budget(40, left, left).0.as_secs_f64() / sec as f64
        };
        assert!(share(1) > 1.0 / 20.0);
        assert!(share(60) > share(1));
    }

    #[test]
    fn protocol_cap_and_future_turns_bound_borrowing() {
        let left = Duration::from_secs(1);
        let cap = ms(5);
        for empty in 1..=60 {
            let (soft, hard) = budget(empty, left, cap);
            assert!(soft <= hard && hard <= cap);
            assert_eq!(budget(empty, Duration::ZERO, cap).1, Duration::ZERO);
        }
        assert!(budget(22, left, left).1 < left);
        assert_eq!(budget(1, left, left).0, left);
    }

    #[test]
    fn misses_of_finished_iterations_are_recorded_in_the_context() {
        let mut f = Forecast::legacy(30);
        let mut ctx = GameContext::default();
        let h = hurdle(1000, 2000);
        let done = |elapsed, kind, next| Finished {
            elapsed,
            kind,
            next,
            best_move: None,
            score: 0,
            nodes: 0,
            pv: &[],
        };
        // No previous ratio: 3 per two plies, so 10 ms predicts 30 ms.
        f.observe(&done(ms(10), depth(10), Some(depth(12))), &mut ctx, h);
        f.observe(&done(ms(70), depth(12), None), &mut ctx, h);
        assert_eq!(ctx.misses.len(), 1);
        let (transition, miss) = ctx.misses[0];
        assert_eq!(transition, Transition::Mid);
        assert!((miss - 2_f64.ln()).abs() < 1e-9, "{miss}");
    }

    /// The game clock of 0c53a14, kept verbatim to check that the split
    /// into parts decides exactly as before.
    mod reference {
        use deft_reversi_engine::{Action, ResultKind, Selectivity};
        use std::time::Duration;

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

        const ENTRY: Ratio = Ratio {
            floor: 2.0,
            base: 5.0,
            pivot: 24.0,
            per_empty: 1.06,
        };
        const EXACT: Ratio = Ratio {
            floor: 1.0,
            base: 3.0,
            pivot: 22.0,
            per_empty: 1.15,
        };
        const STAGE: f64 = 1.25;

        pub fn budget(empties: u8, remaining: Duration, cap: Duration) -> (Duration, Duration) {
            let turns = u32::from(empties.div_ceil(2)).max(1);
            let mut pool = remaining;
            let mut turns_left = turns;
            for distance in (1..turns).rev() {
                let future = i32::from(empties) - 2 * distance as i32;
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
            let hard = share.saturating_add(share.min((pool - share) / 2)).min(cap);
            (share.min(hard), hard)
        }

        #[derive(Default)]
        pub struct Forecast {
            empties: u8,
            elapsed: Duration,
            last_cost: Duration,
            last_depth: Option<u8>,
            best: Option<Action>,
            instability: f64,
        }

        impl Forecast {
            pub fn new(empties: u8) -> Self {
                Self {
                    empties,
                    ..Self::default()
                }
            }

            #[allow(clippy::too_many_arguments)]
            pub fn stop(
                &mut self,
                elapsed: Duration,
                kind: ResultKind,
                next: Option<ResultKind>,
                best: Option<Action>,
                soft: Duration,
                hard: Duration,
            ) -> bool {
                let cost = elapsed.saturating_sub(self.elapsed);
                let growth = next.map(|next| self.growth(kind, next, cost));
                let changed = self.best.is_some() && self.best != best;
                self.instability = if changed { 1.0 } else { self.instability * 0.5 };
                self.best = best;
                self.elapsed = elapsed;
                self.last_cost = cost;
                self.last_depth = match kind {
                    ResultKind::Depth { depth, .. } => Some(depth),
                    ResultKind::Endgame { .. } => None,
                };
                let Some(growth) = growth else {
                    return false;
                };
                let target = soft.as_secs_f64() * (1.0 + self.instability);
                let predicted = elapsed.as_secs_f64() + cost.as_secs_f64() * growth;
                predicted > target.min(hard.as_secs_f64())
            }

            fn growth(&self, kind: ResultKind, next: ResultKind, cost: Duration) -> f64 {
                let empties = f64::from(self.empties);
                match (kind, next) {
                    (ResultKind::Depth { depth, .. }, ResultKind::Depth { depth: deeper, .. }) => {
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
                    (ResultKind::Endgame { .. }, ResultKind::Endgame { .. }) => STAGE,
                    (ResultKind::Endgame { .. }, ResultKind::Depth { .. }) => ENTRY.at(empties),
                }
            }
        }
    }

    /// Random searches: plans like `timed_level_iterations` (midgame steps,
    /// then endgame stages), iteration times spread over five orders of
    /// magnitude, best moves that sometimes change, random budgets.
    #[test]
    fn the_parts_decide_exactly_like_the_previous_game_clock() {
        use rand::{Rng, SeedableRng, rngs::StdRng};
        let mut rng = StdRng::seed_from_u64(20261120);
        let stages = [
            Selectivity::Percent85,
            Selectivity::Percent95,
            Selectivity::Percent98,
            Selectivity::Percent99,
            Selectivity::Full,
        ];
        let moves: Vec<Action> = ["d3", "c4", "f5", "e6"]
            .iter()
            .map(|m| m.parse().unwrap())
            .collect();
        let (mut decisions, mut stops) = (0, 0);
        for _ in 0..20_000 {
            let empties = rng.gen_range(1..=60u8);
            let mut plan = Vec::new();
            let mut d = rng.gen_range(1..=6u8);
            let last_depth = rng.gen_range(d..=30u8);
            if rng.gen_bool(0.8) {
                while d <= last_depth {
                    plan.push(depth(d));
                    // Mostly two plies, sometimes one or three.
                    d += [2, 2, 2, 1, 3][rng.gen_range(0..5)];
                }
            }
            for s in stages {
                if rng.gen_bool(0.7) || s == Selectivity::Full {
                    plan.push(stage(s));
                }
            }
            let soft = Duration::from_nanos(rng.gen_range(0..2_000_000_000));
            let hard = soft + Duration::from_nanos(rng.gen_range(0..2_000_000_000));
            let h = Hurdle {
                soft,
                hard,
                price: None,
            };
            let mut old = reference::Forecast::new(empties);
            let mut new = Forecast::legacy(empties);
            let mut ctx = GameContext::default();
            let mut elapsed = Duration::ZERO;
            let mut best = None;
            for (k, &kind) in plan.iter().enumerate() {
                // Zero, sub-millisecond and multi-second iterations.
                let step = 10_f64.powf(rng.gen_range(-6.0..1.0));
                elapsed += if rng.gen_bool(0.05) {
                    Duration::ZERO
                } else {
                    Duration::from_secs_f64(step)
                };
                if best.is_none() || rng.gen_bool(0.3) {
                    best = Some(moves[rng.gen_range(0..moves.len())]);
                }
                if rng.gen_bool(0.02) {
                    best = None;
                }
                let next = plan.get(k + 1).copied();
                let a = old.stop(elapsed, kind, next, best, soft, hard);
                let done = Finished {
                    elapsed,
                    kind,
                    next,
                    best_move: best,
                    score: 0,
                    nodes: 0,
                    pv: &[],
                };
                let b = new.observe(&done, &mut ctx, h).stop;
                assert_eq!(a, b, "plan {plan:?} at {k}, elapsed {elapsed:?}, {h:?}");
                decisions += 1;
                stops += usize::from(a);
            }
        }
        assert!(
            stops > 1000 && decisions - stops > 1000,
            "{stops} of {decisions}"
        );
        for empties in 0..=60u8 {
            for _ in 0..200 {
                let remaining = Duration::from_nanos(rng.gen_range(0..200_000_000_000));
                let cap = if rng.gen_bool(0.5) {
                    remaining
                } else {
                    Duration::from_nanos(rng.gen_range(0..20_000_000_000))
                };
                assert_eq!(
                    budget(empties, remaining, cap),
                    reference::budget(empties, remaining, cap)
                );
            }
        }
    }
}
