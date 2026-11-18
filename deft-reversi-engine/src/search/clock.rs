//! Time control: spreading a game clock over the moves of a game, and pacing
//! the iterations of one search against the time given to its move.
//!
//! The approach follows Edax and Egaroucid. The clock is spread over this
//! side's moves until the point where an exact solve fits in a reserve kept
//! for it (Edax `solvable_depth`, Egaroucid `calc_time_limit_ply`). Each move
//! gets a target and a hard limit (Edax `mini`/`extra`): the search starts no
//! iteration expected to run far past the target, more time is allowed when
//! the best move is unstable, and the hard limit stops a running iteration.
use super::*;

/// The time for one move: what the search aims to use, and a hard limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveTime {
    /// After an iteration completes, the search starts the next one only if
    /// it is expected to end around this time. An unstable best move extends
    /// it, up to twice and never past `limit`.
    pub target: Duration,
    /// Stops a running iteration; the result is that of the last completed one.
    pub limit: Duration,
}

/// A side's clock in a game where each side has a total time.
///
/// ```
/// # use deft_reversi_engine::GameClock;
/// # use std::time::Duration;
/// let clock = GameClock::new(Duration::from_secs(60)).threads(4);
/// let time = clock.move_time(40);
/// assert!(time.target <= time.limit && time.limit < Duration::from_secs(60));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct GameClock {
    remaining: Duration,
    overhead: Duration,
    move_cap: Option<Duration>,
    threads: usize,
}

/// Exact solves at `SOLVE_EMPTIES` empties, every stage included, take about
/// this long with one thread; each further empty multiplies it by
/// `SOLVE_GROWTH`. Measured with the v3 model on a 4-thread x86-64 machine
/// (20 empties: 0.04-0.07 s, 26 empties: 8.5 s); it is only a planning guess.
const SOLVE_EMPTIES: u8 = 20;
const SOLVE_SECONDS_ONE_THREAD: f64 = 0.2;
const SOLVE_GROWTH: f64 = 2.2;
/// Solve now when an exact solve is expected to take at most this share of the clock.
const SOLVE_NOW_SHARE: f64 = 0.4;
/// The reserve for the endgame is kept below this share of the clock.
const RESERVE_SHARE: f64 = 0.4;
/// The hard limit of a move, as a multiple of its target.
const LIMIT_FACTOR: u32 = 3;

impl GameClock {
    /// `remaining` is the time left on this side's clock.
    pub fn new(remaining: Duration) -> Self {
        Self {
            remaining,
            overhead: Duration::from_millis(10),
            move_cap: None,
            threads: 1,
        }
    }
    /// Time lost outside the search on each move, such as communication. It
    /// is kept back for every move this side may still play. Default 10 ms.
    pub fn overhead(mut self, overhead: Duration) -> Self {
        self.overhead = overhead;
        self
    }
    /// A limit on any single move, such as a host's per-move budget.
    pub fn move_cap(mut self, cap: Duration) -> Self {
        self.move_cap = Some(cap);
        self
    }
    /// Search threads, used to estimate how many empties can be solved.
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }
    /// The estimated time of an exact solve at `empties` empties.
    pub fn solve_time(&self, empties: u8) -> Duration {
        let speedup = (self.threads as f64).powf(0.8);
        let exponent = f64::from(empties) - f64::from(SOLVE_EMPTIES);
        let seconds = SOLVE_SECONDS_ONE_THREAD * SOLVE_GROWTH.powf(exponent) / speedup;
        Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
    }
    /// The time for the move at a position with `empties` empties.
    pub fn move_time(&self, empties: u8) -> MoveTime {
        let own_moves = u32::from(empties).div_ceil(2).max(1);
        let usable = self
            .remaining
            .saturating_sub(self.overhead.saturating_mul(own_moves + 1));
        let time = if usable.is_zero() {
            // Out of time: spread what is left rather than giving up.
            let share = self.remaining / (own_moves + 1);
            MoveTime {
                target: share / 2,
                limit: share,
            }
        } else if self.solve_time(empties) <= usable.mul_f64(SOLVE_NOW_SHARE) {
            // The game can be solved now; later moves will be free.
            MoveTime {
                target: usable.mul_f64(SOLVE_NOW_SHARE),
                limit: usable.mul_f64(1.5 * SOLVE_NOW_SHARE),
            }
        } else {
            // Spread the clock over the moves until the empties where the
            // reserve solves the game. Arriving there with the reserve, an
            // exact solve takes `SOLVE_NOW_SHARE` of it.
            let reserve_cap = usable.mul_f64(RESERVE_SHARE);
            let solve_at = (0..empties)
                .rev()
                .find(|&e| self.solve_time(e).div_f64(SOLVE_NOW_SHARE) <= reserve_cap)
                .unwrap_or(0);
            let reserve = self.solve_time(solve_at).div_f64(SOLVE_NOW_SHARE);
            let pool = usable.saturating_sub(reserve);
            let moves = u32::from(empties - solve_at).div_ceil(2).max(1);
            let target = pool / moves;
            MoveTime {
                target,
                limit: (target * LIMIT_FACTOR).min(pool / 2).max(target),
            }
        };
        match self.move_cap {
            Some(cap) => MoveTime {
                target: time.target.min(cap),
                limit: time.limit.min(cap),
            },
            None => time,
        }
    }
}

/// Whether to start the next iteration of a search with a target time.
pub(super) struct Pacer {
    base: Duration,
    target: Duration,
    limit: Option<Duration>,
    /// The duration of the last completed iteration and its kind.
    last: Option<(Iteration, Duration)>,
    /// The growth between the last two midgame iterations.
    midgame_growth: Option<f64>,
    previous: Option<(Option<Action>, i32, bool)>,
}

/// Default growth from one midgame iteration to the next, two plies deeper.
const MIDGAME_GROWTH: f64 = 2.5;
/// Growth from the 99% endgame stage to the exact solve; it varies a lot,
/// and an overestimate only postpones the solve to the next move.
const SOLVE_STAGE_GROWTH: f64 = 20.0;
/// Growth between selective endgame stages.
const STAGE_GROWTH: f64 = 2.0;
/// Starts an iteration when the target leaves this share of its expected time.
const TARGET_OVERRUN: f64 = 0.5;
/// Extension of the target when the best move changes or the score drops.
const INSTABILITY: f64 = 1.4;
const MAX_EXTENSION: u32 = 2;
/// A score drop, in discs, that counts as instability.
const SCORE_DROP: i32 = 3;

impl Pacer {
    pub(super) fn new(target: Duration, limit: Option<Duration>) -> Self {
        Self {
            base: target,
            target,
            limit,
            last: None,
            midgame_growth: None,
            previous: None,
        }
    }

    /// Records an iteration that took `duration` and found `best` and `score`.
    pub(super) fn completed(
        &mut self,
        iteration: Iteration,
        duration: Duration,
        best: Option<Action>,
        score: i32,
    ) {
        if let Some((last, last_duration)) = self.last
            && !last.exact
            && !iteration.exact
            && last_duration >= Duration::from_millis(2)
        {
            let growth = duration.as_secs_f64() / last_duration.as_secs_f64();
            // Average with the previous growth: a single step is noisy.
            let growth = match self.midgame_growth {
                Some(previous) => (previous * growth).sqrt(),
                None => growth,
            };
            self.midgame_growth = Some(growth);
        }
        if let Some((previous_best, previous_score, previous_exact)) = self.previous {
            let changed = previous_best != best;
            // Scores of a midgame search and an endgame search are not comparable.
            let dropped = previous_exact == iteration.exact && score <= previous_score - SCORE_DROP;
            if changed || dropped {
                let mut target = self
                    .target
                    .mul_f64(INSTABILITY)
                    .min(self.base * MAX_EXTENSION);
                if let Some(limit) = self.limit {
                    target = target.min(limit);
                }
                self.target = self.target.max(target);
            }
        }
        self.previous = Some((best, score, iteration.exact));
        self.last = Some((iteration, duration));
    }

    /// The expected duration of `next`.
    fn expected(&self, next: Iteration, empties: u8) -> Option<Duration> {
        let (last, duration) = self.last?;
        let growth = match (last.exact, next.exact) {
            (false, false) => self
                .midgame_growth
                .unwrap_or(MIDGAME_GROWTH)
                .clamp(1.5, 5.0)
                .powf(f64::from(next.depth.saturating_sub(last.depth)) / 2.0),
            // The first endgame stage costs more relative to the midgame
            // with more empties: about 4 times at 28 empties, 13 at 32.
            (false, true) => 4.0 * 1.35f64.powi(i32::from(empties) - 28).max(1.0),
            _ if next.selectivity == Selectivity::Full => SOLVE_STAGE_GROWTH,
            _ => STAGE_GROWTH,
        };
        Some(duration.mul_f64(growth))
    }

    /// Whether to start `next` at `elapsed` into the search.
    pub(super) fn should_start(&self, next: Iteration, empties: u8, elapsed: Duration) -> bool {
        let Some(expected) = self.expected(next, empties) else {
            return true;
        };
        let fits_target = elapsed + expected.mul_f64(TARGET_OVERRUN) <= self.target;
        let fits_limit = self.limit.is_none_or(|limit| elapsed + expected <= limit);
        fits_target && fits_limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mid(depth: u8) -> Iteration {
        Iteration {
            depth,
            exact: false,
            selectivity: Selectivity::Percent83,
        }
    }

    #[test]
    fn move_time_reserves_the_endgame_and_respects_caps() {
        let clock = GameClock::new(Duration::from_secs(60)).threads(4);
        let opening = clock.move_time(56);
        assert!(opening.target > Duration::from_millis(1500));
        assert!(opening.target < Duration::from_secs(4));
        assert!(opening.target <= opening.limit);
        assert!(opening.limit <= opening.target * LIMIT_FACTOR);
        // Solvable now: spend a large share, later moves are free.
        let end = GameClock::new(Duration::from_secs(20))
            .threads(4)
            .move_time(20);
        assert_eq!(
            end.target,
            Duration::from_millis(20_000 - 110).mul_f64(SOLVE_NOW_SHARE)
        );
        let capped = clock.move_cap(Duration::from_millis(500)).move_time(40);
        assert!(capped.limit <= Duration::from_millis(500));
        assert!(capped.target <= capped.limit);
        // Nearly out of time: still some time for each move.
        let short = GameClock::new(Duration::from_millis(50)).move_time(30);
        assert!(short.limit > Duration::ZERO && short.limit < Duration::from_millis(50));
    }

    #[test]
    fn move_time_grows_with_the_clock() {
        let mut last = Duration::ZERO;
        for seconds in [10, 30, 60, 300, 1800] {
            let time = GameClock::new(Duration::from_secs(seconds)).move_time(40);
            assert!(time.target > last);
            last = time.target;
        }
    }

    #[test]
    fn pacer_skips_iterations_expected_to_overrun() {
        let mut pacer = Pacer::new(Duration::from_secs(1), Some(Duration::from_secs(3)));
        pacer.completed(mid(10), Duration::from_millis(100), None, 0);
        pacer.completed(mid(12), Duration::from_millis(200), None, 0);
        // Next is about 400 ms: started at 0.5 s, it ends well before the limit.
        assert!(pacer.should_start(mid(14), 40, Duration::from_millis(500)));
        // At 0.9 s, half of it would pass the target.
        assert!(!pacer.should_start(mid(14), 40, Duration::from_millis(900)));
    }

    #[test]
    fn pacer_extends_the_target_when_the_best_move_changes() {
        let a = Some(Action::Place(Square::new(19).unwrap()));
        let b = Some(Action::Place(Square::new(26).unwrap()));
        let mut pacer = Pacer::new(Duration::from_secs(1), Some(Duration::from_secs(3)));
        pacer.completed(mid(10), Duration::from_millis(100), a, 0);
        pacer.completed(mid(12), Duration::from_millis(200), a, 0);
        assert!(!pacer.should_start(mid(14), 40, Duration::from_millis(900)));
        pacer.completed(mid(14), Duration::from_millis(400), b, 0);
        assert!(pacer.target > Duration::from_secs(1));
        for _ in 0..10 {
            pacer.completed(mid(14), Duration::from_millis(400), a, -10);
            pacer.completed(mid(14), Duration::from_millis(400), b, -20);
        }
        assert_eq!(pacer.target, Duration::from_secs(2));
    }
}
