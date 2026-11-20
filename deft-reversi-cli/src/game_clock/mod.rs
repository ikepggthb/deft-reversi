//! Time allocation for a whole game. Protocols own the clock and reply margin.
//!
//! The decisions come from four replaceable parts (`parts`): a cost model,
//! a value model, a clock model and a stopping policy. `legacy` holds the
//! current ones. `GameContext` carries what a side remembers between turns.
mod context;
mod journal;
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
use serde::Serialize;
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
        report: impl FnMut(&Progress),
    ) -> Result<SearchResult, EngineError> {
        let extend = extension::seconds(ctx, pos.empty_count());
        self.run(engine, pos, ctx, report, extend)
            .map(|(result, _)| result)
    }

    /// `extend`: run in extension mode up to this time (see `extension`).
    fn run(
        &self,
        engine: &mut Engine,
        pos: Position,
        ctx: &mut GameContext,
        mut report: impl FnMut(&Progress),
        extend: Option<Duration>,
    ) -> Result<(SearchResult, Decision), EngineError> {
        let start = Instant::now();
        let hard = self.hurdle.hard;
        if extend.is_some() {
            ctx.extended = true;
        }
        let limit = extend.map_or(hard, |e| e.max(hard));
        let request = SearchRequest::timed_levels(60, limit)
            .reuse_pv()
            .skip_pv_recovery();
        let handle = request.stop_handle();
        let _armed = stop::arm(handle.clone());
        let mut forecast = Forecast::legacy(pos.empty_count());
        let mut forecast_stopped = false;
        let mut pending = None;
        let mut decision = Decision::default();
        let hurdle = self.hurdle;
        let logging = journal::enabled();
        let mut result = engine.search_with(pos, &request, |p| {
            report(p);
            let done = Finished::from(p);
            let step = forecast.observe(&done, ctx, hurdle);
            let after_decision = decision.made;
            if !decision.made {
                if p.elapsed > hard {
                    // A real search would have ended at hard, inside this
                    // iteration, with the previous iteration's move.
                    decision.made = true;
                } else {
                    decision.best_move = p.best_move;
                    decision.reached = Some(p.kind);
                    decision.pending = p.next;
                    decision.elapsed = p.elapsed;
                    decision.made = step.stop;
                }
            }
            if !after_decision {
                pending = p.next;
            }
            if logging {
                let mut record = IterationRecord::new(ctx, &done, &step, hurdle);
                if extend.is_some() {
                    record.extension = Some(ExtensionFlags {
                        after_decision,
                        beyond_hard: p.elapsed > hard,
                    });
                }
                journal::write(&record, false);
            }
            if step.stop && extend.is_none() {
                forecast_stopped = true;
                handle.stop();
            }
        })?;
        // The forecast stops through the same handle as Ctrl-C. Report its
        // stops as the time decisions they are, so logs can tell them apart.
        if forecast_stopped && result.stop_reason == StopReason::Requested {
            result.stop_reason = StopReason::TimeLimit;
        }
        if extend.is_some() && decision.reached.is_some() {
            // Play what the clock would have played.
            result.best_move = decision.best_move;
        }
        let used = start.elapsed();
        if logging {
            let mut record = TurnRecord::new(ctx, pos, used, Some(self), result.best_move);
            record.searched = true;
            record.stop_reason = Some(format!("{:?}", result.stop_reason));
            record.reached = result.kind.as_ref().map(tag);
            record.pending = pending.as_ref().map(tag);
            record.score = result.score;
            record.extended = extend.is_some();
            record.after_extension = ctx.extended && extend.is_none();
            if extend.is_some() {
                record.decision_elapsed = Some(decision.elapsed.as_secs_f64());
                record.decision_reached = decision.reached.as_ref().map(tag);
                record.pending = decision.pending.as_ref().map(tag);
            }
            journal::write(&record, true);
        }
        ctx.end_turn(TurnSummary {
            position: pos,
            used,
            reached: result.kind,
            pv: result.pv.clone(),
        });
        Ok((result, decision))
    }
}

/// Where the game clock decided to stop. In extension mode the search goes
/// on, but the move played is this one.
#[derive(Clone, Copy, Debug, Default)]
struct Decision {
    made: bool,
    best_move: Option<Action>,
    /// The last iteration before the decision, the one it would have run
    /// next, and when the former finished.
    reached: Option<ResultKind>,
    pending: Option<ResultKind>,
    elapsed: Duration,
}

/// Extension mode, for data on the iterations the clock does not run: with
/// `DEFT_CLOCK_EXTEND=EMPTIES:SECONDS`, the first turn of each game with at
/// most EMPTIES empties searches on to SECONDS (or hard, if later) after the
/// clock would have stopped, logging every iteration, and plays the move
/// the clock would have played. The extra time and table entries change
/// the rest of the game, which the log marks (`after_extension`); data
/// runs end the game there.
mod extension {
    use super::GameContext;
    use std::{sync::OnceLock, time::Duration};

    static SETTING: OnceLock<Option<(u8, Duration)>> = OnceLock::new();

    pub fn parse(text: &str) -> Option<(u8, Duration)> {
        let (empties, seconds) = text.split_once(':')?;
        let seconds: f64 = seconds.parse().ok()?;
        (seconds.is_finite() && seconds > 0.0)
            .then(|| Some((empties.parse().ok()?, Duration::from_secs_f64(seconds))))
            .flatten()
    }

    /// The extension for this turn, if it is the one to extend.
    pub fn seconds(ctx: &GameContext, empties: u8) -> Option<Duration> {
        let setting = SETTING.get_or_init(|| {
            let text = std::env::var("DEFT_CLOCK_EXTEND").ok()?;
            let parsed = parse(&text);
            if parsed.is_none() {
                eprintln!("DEFT_CLOCK_EXTEND: expected EMPTIES:SECONDS, got {text}");
            }
            parsed
        });
        let (at_most, seconds) = (*setting)?;
        (!ctx.extended && empties <= at_most).then_some(seconds)
    }
}

/// A turn decided without a timed search: a forced move, a book move, a
/// move while the model loads, or `reflex`. `budget` is the turn's budget
/// when one was computed.
pub(crate) fn unsearched_turn(
    ctx: &mut GameContext,
    position: Position,
    used: Duration,
    budget: Option<&Budget>,
    played: Option<Action>,
) {
    if journal::enabled() {
        journal::write(&TurnRecord::new(ctx, position, used, budget, played), true);
    }
    ctx.end_turn(TurnSummary {
        position,
        used,
        reached: None,
        pv: played.into_iter().collect(),
    });
}

/// One line of the clock log per finished iteration.
#[derive(Serialize)]
struct IterationRecord {
    record: &'static str,
    session: u32,
    game: u64,
    empties: u8,
    kind: String,
    /// The iteration planned next; None after the last one.
    next: Option<String>,
    /// Seconds since the search started, and of this iteration alone.
    elapsed: f64,
    cost: f64,
    nodes: u64,
    score: i32,
    best_move: Option<String>,
    pv: Vec<String>,
    /// The cost model's estimate of `next`'s own time (median seconds and
    /// sigma of the log); `elapsed + predicted` is its predicted end.
    predicted: Option<f64>,
    log_sigma: Option<f64>,
    /// Its chance to finish before hard.
    p_finish_hard: Option<f64>,
    instability: f64,
    expected_discs: Option<f64>,
    /// The policy stopped the search here.
    stop: bool,
    /// Extension mode only.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    extension: Option<ExtensionFlags>,
}

#[derive(Serialize)]
struct ExtensionFlags {
    /// The clock had already decided to stop before this iteration ended
    /// (a real search would not have finished it).
    after_decision: bool,
    /// The iteration ended after hard.
    beyond_hard: bool,
}

impl IterationRecord {
    fn new(ctx: &GameContext, done: &Finished, step: &Step, hurdle: Hurdle) -> Self {
        let left = hurdle.hard.saturating_sub(done.elapsed).as_secs_f64();
        Self {
            record: "iteration",
            session: journal::session(),
            game: ctx.game,
            empties: step.empties,
            kind: tag(&done.kind),
            next: done.next.as_ref().map(tag),
            elapsed: done.elapsed.as_secs_f64(),
            cost: step.cost.as_secs_f64(),
            nodes: done.nodes,
            score: done.score,
            best_move: done.best_move.map(|m| m.to_string().to_lowercase()),
            pv: journal::moves(done.pv),
            predicted: step.estimate.map(|e| e.median),
            log_sigma: step.estimate.map(|e| e.log_sigma),
            p_finish_hard: step.estimate.map(|e| e.p_finish(left)),
            instability: step.gain.instability,
            expected_discs: step.gain.expected_discs,
            stop: step.stop,
            extension: None,
        }
    }
}

/// One line of the clock log per turn of this side.
#[derive(Serialize)]
struct TurnRecord {
    record: &'static str,
    session: u32,
    game: u64,
    /// False for forced, book, loading and reflex moves.
    searched: bool,
    empties: u8,
    #[serde(flatten)]
    board: journal::Board,
    protocol: Option<Protocol>,
    threads: Option<usize>,
    /// Seconds: the clock after the reply margin, the turn's limit, and the
    /// clock model's soft target and hard limit.
    remaining: Option<f64>,
    cap: Option<f64>,
    soft: Option<f64>,
    hard: Option<f64>,
    price: Option<f64>,
    /// Seconds this turn took in the game clock code.
    used: f64,
    stop_reason: Option<String>,
    /// The last finished iteration, and the one planned next when the
    /// search ended.
    reached: Option<String>,
    pending: Option<String>,
    played: Option<String>,
    score: Option<i32>,
    previous_empties: Option<u8>,
    previous_used: Option<f64>,
    previous_reached: Option<String>,
    /// The opponent played the reply the previous turn's pv expected.
    reply_matched: Option<bool>,
    /// This turn ran in extension mode: `reached` and `pending` are then
    /// where the search ended, `decision_*` where the clock decided.
    extended: bool,
    /// An earlier turn of this game ran in extension mode.
    after_extension: bool,
    decision_elapsed: Option<f64>,
    decision_reached: Option<String>,
}

impl TurnRecord {
    fn new(
        ctx: &GameContext,
        position: Position,
        used: Duration,
        budget: Option<&Budget>,
        played: Option<Action>,
    ) -> Self {
        let previous = ctx.previous.as_ref();
        Self {
            record: "turn",
            session: journal::session(),
            game: ctx.game,
            searched: false,
            empties: position.empty_count(),
            board: journal::Board::of(position),
            protocol: budget.map(|b| b.clock.protocol),
            threads: budget.map(|b| b.clock.threads),
            remaining: budget.map(|b| b.clock.remaining.as_secs_f64()),
            cap: budget.map(|b| b.clock.cap.as_secs_f64()),
            soft: budget.map(|b| b.hurdle.soft.as_secs_f64()),
            hard: budget.map(|b| b.hurdle.hard.as_secs_f64()),
            price: budget.and_then(|b| b.hurdle.price),
            used: used.as_secs_f64(),
            stop_reason: None,
            reached: None,
            pending: None,
            played: played.map(|m| m.to_string().to_lowercase()),
            score: None,
            previous_empties: previous.map(|p| p.position.empty_count()),
            previous_used: previous.map(|p| p.used.as_secs_f64()),
            previous_reached: previous.and_then(|p| p.reached.as_ref().map(tag)),
            reply_matched: ctx.reply_matched,
            extended: false,
            after_extension: ctx.extended,
            decision_elapsed: None,
            decision_reached: None,
        }
    }
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
    empties: u8,
    /// The iteration's own time.
    cost: Duration,
    estimate: Option<CostEstimate>,
    gain: GainEstimate,
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
        let estimate = self.cost.next(&obs, ctx);
        let gain = self.value.gain(&obs, ctx);
        let stop = self.policy.stop(&obs, estimate, gain, hurdle);
        self.pending = done
            .next
            .zip(estimate)
            .map(|(next, e)| (Transition::of(done.kind, next), e.median));
        Step {
            empties: self.empties,
            cost,
            estimate,
            gain,
            stop,
        }
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
    fn extension_settings_are_parsed_strictly() {
        assert_eq!(
            extension::parse("26:10"),
            Some((26, Duration::from_secs(10)))
        );
        assert_eq!(extension::parse("30:0.5"), Some((30, ms(500))));
        for bad in [
            "26", "26:", ":10", "x:10", "26:0", "26:-1", "26:inf", "26:nan",
        ] {
            assert_eq!(extension::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn extension_mode_searches_on_but_plays_the_clock_move() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(16),
        )
        .unwrap();
        let mut pos = Position::initial();
        let mut ply = 0;
        while pos.empty_count() > 34 {
            if pos.must_pass() {
                pos = pos.play(Action::Pass).unwrap();
                continue;
            }
            let moves: Vec<_> = pos.legal_moves().collect();
            ply += 1;
            pos = pos
                .play(Action::Place(moves[(ply * 5) % moves.len()]))
                .unwrap();
        }
        let budget = Budget {
            clock: ClockState {
                empties: pos.empty_count(),
                remaining: Duration::from_secs(1),
                cap: Duration::from_secs(1),
                protocol: Protocol::Gtp,
                threads: 1,
            },
            hurdle: hurdle(1, 3),
        };
        let mut ctx = GameContext::default();
        ctx.begin_turn(pos);
        let mut seen = Vec::new();
        let (result, decision) = budget
            .run(
                &mut engine,
                pos,
                &mut ctx,
                |p| seen.push(p.elapsed),
                Some(ms(300)),
            )
            .unwrap();
        assert!(decision.made && decision.reached.is_some());
        assert!(decision.elapsed <= ms(3), "{decision:?}");
        assert!(
            seen.iter().any(|&t| t > ms(3)),
            "nothing after the decision: {seen:?}"
        );
        assert_eq!(result.best_move, decision.best_move);
        assert!(ctx.extended);
        // Without the extension the same budget stops at the decision.
        let mut plain = GameContext::default();
        plain.begin_turn(pos);
        engine.clear_tt().unwrap();
        let (result, decision) = budget
            .run(&mut engine, pos, &mut plain, |_| {}, None)
            .unwrap();
        assert!(!plain.extended);
        assert!(result.elapsed < ms(100), "{:?}", result.elapsed);
        assert_eq!(result.kind, decision.reached);
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
