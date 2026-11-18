use super::*;

/// Compatibility epoch for persisted estimated search values. Increment when
/// evaluation/search/MPC semantics change incompatibly, including level plans.
/// Thread counts, time limits and performance-only changes do not require a bump.
pub const SEARCH_SEMANTICS_REVISION: u32 = 2;

/// The internal search driver mode a request resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchMode {
    Depth(u8),
    /// Exact final result. Calibrated selective preparation may precede it.
    Exact,
    /// Terminal-depth search with calibrated probabilistic cuts.
    SelectiveEndgame,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    // Stored in an `AtomicU8` during the search; zero means still running.
    Completed = 0,
    Requested = 1,
    TimeLimit = 2,
    NodeLimit = 3,
    WorkerFailed = 4,
}
/// Stops a search from another thread. Clones share the same flag, and a
/// stopped handle stays stopped, so each search needs a new request.
#[derive(Clone, Default, Debug)]
pub struct StopHandle(pub(super) Arc<AtomicBool>);
impl StopHandle {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
/// Build with `EngineOptions::default().threads(n).tt_mib(m)`.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct EngineOptions {
    /// Search threads, including the calling thread: 1..=256.
    pub threads: usize,
    /// Size of the main transposition table.
    pub tt_mib: usize,
}
impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            threads: 1,
            tt_mib: 256,
        }
    }
}
impl EngineOptions {
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }
    pub fn tt_mib(mut self, tt_mib: usize) -> Self {
        self.tt_mib = tt_mib;
        self
    }
}
/// ProbCut confidence, ordered from least accurate to full search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Selectivity {
    Percent83,
    Percent85,
    Percent95,
    Percent98,
    Percent99,
    #[default]
    Full,
}
impl Selectivity {
    /// Every stage, least accurate first.
    pub const ALL: [Self; 6] = [
        Self::Percent83,
        Self::Percent85,
        Self::Percent95,
        Self::Percent98,
        Self::Percent99,
        Self::Full,
    ];
    pub fn percent(self) -> u8 {
        match self {
            Self::Percent83 => 83,
            Self::Percent85 => 85,
            Self::Percent95 => 95,
            Self::Percent98 => 98,
            Self::Percent99 => 99,
            Self::Full => 100,
        }
    }
    /// The stage with this probability: 83, 85, 95, 98, 99 or 100.
    pub fn from_percent(percent: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.percent() == percent)
    }
    /// The next more accurate stage; `Full` stays `Full`.
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Percent83 => Self::Percent85,
            Self::Percent85 => Self::Percent95,
            Self::Percent95 => Self::Percent98,
            Self::Percent98 => Self::Percent99,
            Self::Percent99 | Self::Full => Self::Full,
        }
    }
    /// The tag of the stage in the transposition tables: zero for `Full`.
    pub(crate) fn quality(self) -> u8 {
        match self {
            Self::Full => 0,
            Self::Percent83 => 1,
            Self::Percent85 => 2,
            Self::Percent95 => 3,
            Self::Percent98 => 4,
            Self::Percent99 => 5,
        }
    }
}

/// What a search reads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// That many moves ahead, 0..=60. Passes are not counted.
    Depth(u8),
    /// To the end of the game; an exact solve at `Selectivity::Full`.
    Endgame,
    /// A level, 1..=60, which chooses the depth, the switch to the endgame
    /// and the selectivity: `Plan(SearchPlan::level(n)?)`.
    Level(u8),
    /// A midgame depth and the empties from which the endgame is searched,
    /// chosen freely or changed from a level's.
    Plan(SearchPlan),
}

/// What a search does by the number of empties: with few enough empties it
/// searches to the end of the game at one of the endgame stages, otherwise
/// the midgame to a depth.
///
/// ```
/// # use deft_reversi_engine::{SearchPlan, Selectivity};
/// // Level 24, but solving exactly from 26 empties.
/// let plan = SearchPlan::level(24)?.endgame(Selectivity::Full, 26);
/// // Depth 12 at 83%; 95% endgame from 20 empties, exact from 16.
/// let own = SearchPlan::midgame(12, Selectivity::Percent83)
///     .endgame(Selectivity::Percent95, 20)
///     .endgame(Selectivity::Full, 16);
/// # Ok::<(), deft_reversi_engine::EngineError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchPlan {
    midgame_depth: u8,
    midgame_selectivity: Selectivity,
    /// By `Selectivity`: the most empties searched to the end at that stage;
    /// zero if the stage is not used.
    endgame: [u8; 6],
}
impl SearchPlan {
    /// The plan of a level, 1..=60.
    pub fn level(level: u8) -> Result<Self, EngineError> {
        if !(1..=60).contains(&level) {
            return Err(EngineError::InvalidRequest("level must be 1..=60"));
        }
        Ok(level::plan(level))
    }
    /// A midgame search to `depth` (0..=60) at every number of empties.
    pub fn midgame(depth: u8, selectivity: Selectivity) -> Self {
        Self {
            midgame_depth: depth,
            midgame_selectivity: selectivity,
            endgame: [0; 6],
        }
    }
    /// Changes the midgame search, keeping the endgame stages.
    pub fn with_midgame(mut self, depth: u8, selectivity: Selectivity) -> Self {
        self.midgame_depth = depth;
        self.midgame_selectivity = selectivity;
        self
    }
    /// Searches to the end of the game at `selectivity` (`Full` solves
    /// exactly) with `up_to_empties` or fewer empties; zero drops the stage.
    /// Where several stages apply, the most accurate one is used.
    pub fn endgame(mut self, selectivity: Selectivity, up_to_empties: u8) -> Self {
        self.endgame[selectivity as usize] = up_to_empties;
        self
    }
    pub fn midgame_depth(&self) -> u8 {
        self.midgame_depth
    }
    pub fn midgame_selectivity(&self) -> Selectivity {
        self.midgame_selectivity
    }
    /// The most empties searched to the end at `selectivity`; zero if unused.
    pub fn endgame_limit(&self, selectivity: Selectivity) -> u8 {
        self.endgame[selectivity as usize]
    }
    pub(crate) fn validate(&self) -> Result<(), EngineError> {
        if self.midgame_depth > 60 || self.endgame.iter().any(|&e| e > 60) {
            return Err(EngineError::InvalidRequest(
                "a plan's depth and empties must be 0..=60",
            ));
        }
        Ok(())
    }
    /// The search at a position with `empties` empties.
    pub(crate) fn at(&self, empties: u8) -> (SearchMode, Selectivity) {
        let stage = Selectivity::ALL
            .into_iter()
            .rev()
            .find(|&stage| empties <= self.endgame[stage as usize]);
        match stage {
            Some(Selectivity::Full) => (SearchMode::Exact, Selectivity::Full),
            Some(stage) => (SearchMode::SelectiveEndgame, stage),
            None => (
                SearchMode::Depth(self.midgame_depth),
                self.midgame_selectivity,
            ),
        }
    }
}

/// Built with `SearchRequest::new(goal)` and the setters below.
#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub(super) goal: Goal,
    pub(super) selectivity: Option<Selectivity>,
    pub(super) time_limit: Option<Duration>,
    pub(super) timed_levels: bool,
    pub(super) node_limit: Option<u64>,
    pub(super) all_moves: bool,
    pub(super) move_pvs: bool,
    pub(super) review_move: Option<crate::Square>,
    pub(super) root_moves: Option<u64>,
    pub(super) preferred_move: Option<crate::Square>,
    pub(super) minimum_score: Option<i32>,
    pub(super) reuse_pv: bool,
    pub(super) recover_endgame_pv: bool,
    pub(super) skip_pv_recovery: bool,
    pub(super) stop: StopHandle,
}
impl SearchRequest {
    pub fn new(goal: Goal) -> Self {
        Self {
            goal,
            selectivity: None,
            time_limit: None,
            timed_levels: false,
            node_limit: None,
            all_moves: false,
            move_pvs: false,
            review_move: None,
            root_moves: None,
            preferred_move: None,
            minimum_score: None,
            reuse_pv: false,
            recover_endgame_pv: false,
            skip_pv_recovery: false,
            stop: StopHandle::new(),
        }
    }
    /// Iteratively search up to `level` under one clock, returning the best
    /// move from the last completed iteration. With a calibrated model, the
    /// root's empty-square count determines the last evaluation depth and
    /// endgame stages from the level table. Evaluation depths advance by two.
    pub fn timed_levels(level: u8, limit: Duration) -> Self {
        Self {
            timed_levels: true,
            ..Self::new(Goal::Level(level)).time_limit(limit)
        }
    }
    /// Pruning for `Depth` and `Endgame`; `Full` when not set. A `Level`
    /// chooses its own, so setting one there is an invalid request.
    pub fn selectivity(mut self, selectivity: Selectivity) -> Self {
        self.selectivity = Some(selectivity);
        self
    }
    /// Search only these legal root squares. Descendants are unrestricted.
    /// A subset result proves at most the selected moves, not the whole root.
    /// For a forced pass or terminal root the only valid mask is zero.
    pub fn root_moves(mut self, mask: u64) -> Self {
        self.root_moves = Some(mask);
        self
    }
    /// A legal ordering hint; ignored if outside the selected root moves.
    pub fn preferred_move(mut self, square: crate::Square) -> Self {
        self.preferred_move = Some(square);
        self
    }
    /// At the final midgame depth, first test whether any selected move
    /// reaches this score. A failed test returns an upper bound for the
    /// entire mask; a successful test is followed by a normal point search.
    pub fn minimum_score(mut self, score: i32) -> Self {
        self.minimum_score = Some(score);
        self
    }
    /// Reuse a compatible transposition-table result as a warm start. The
    /// target iteration is still searched with the requested root mask.
    pub fn reuse_pv(mut self) -> Self {
        self.reuse_pv = true;
        self
    }
    /// Try to extend a selective endgame PV at the completed search stage.
    /// Recovery shares the search limits and may leave a partial line.
    pub fn recover_endgame_pv(mut self) -> Self {
        self.recover_endgame_pv = true;
        self
    }
    /// Return the line found during search without a separate PV recovery
    /// pass. The best move and score still come from the completed search.
    pub fn skip_pv_recovery(mut self) -> Self {
        self.skip_pv_recovery = true;
        self
    }
    /// Stops after this much time, returning the last completed iteration.
    pub fn time_limit(mut self, limit: Duration) -> Self {
        self.time_limit = Some(limit);
        self
    }
    /// Stops before visiting more nodes than this, across all threads.
    pub fn node_limit(mut self, limit: u64) -> Self {
        self.node_limit = Some(limit);
        self
    }
    /// Also scores every selected legal root move with a full window (`SearchResult::moves`).
    /// This searches each move again, so it takes several times longer.
    pub fn all_moves(mut self) -> Self {
        self.all_moves = true;
        self
    }
    /// Score every selected root move and retain its principal variation from
    /// the scoring search. TT cutoffs may leave a partial line.
    pub fn all_moves_with_pv(mut self) -> Self {
        self.all_moves = true;
        self.move_pvs = true;
        self
    }
    /// Score the principal move and the specified legal move with full windows
    /// at the final search stage. Intended for reviewing a played move.
    /// This skips full PV recovery. If `all_moves` is set, score every root move.
    pub fn review_move(mut self, square: crate::Square) -> Self {
        self.review_move = Some(square);
        self
    }
    /// Stops on `stop` instead of a handle of its own, so that one handle
    /// can stop several searches, such as all those of a batch job.
    pub fn with_stop(mut self, stop: StopHandle) -> Self {
        self.stop = stop;
        self
    }
    pub fn stop_handle(&self) -> StopHandle {
        self.stop.clone()
    }
    pub fn goal(&self) -> Goal {
        self.goal
    }
}
/// Counters of one search. The parallel ones (`splits` to `join_time`) are
/// diagnostics and may change between versions.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct SearchStats {
    pub nodes: u64,
    pub leaves: u64,
    pub evaluations: u64,
    pub tt_hits: u64,
    pub tt_cuts: u64,
    pub beta_cuts: u64,
    pub splits: u64,
    pub helper_jobs: u64,
    pub ancestor_handoffs: u64,
    pub worker_dispatches: u64,
    /// Sum of join time across masters; includes time spent helping descendants.
    pub join_time: Duration,
    pub probcut_tries: u64,
    pub probcut_cuts: u64,
    pub stability_cuts: u64,
    /// Number of initial iterations skipped using a compatible TT entry.
    pub reused_iterations: u64,
}
impl SearchStats {
    pub(super) fn merge(&mut self, other: &Self) {
        self.nodes += other.nodes;
        self.leaves += other.leaves;
        self.evaluations += other.evaluations;
        self.tt_hits += other.tt_hits;
        self.tt_cuts += other.tt_cuts;
        self.beta_cuts += other.beta_cuts;
        self.splits += other.splits;
        self.helper_jobs += other.helper_jobs;
        self.ancestor_handoffs += other.ancestor_handoffs;
        self.worker_dispatches += other.worker_dispatches;
        self.join_time += other.join_time;
        self.probcut_tries += other.probcut_tries;
        self.probcut_cuts += other.probcut_cuts;
        self.stability_cuts += other.stability_cuts;
        self.reused_iterations += other.reused_iterations;
    }
}
/// What a completed iteration searched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Depth { depth: u8, selectivity: Selectivity },
    Endgame { selectivity: Selectivity },
}
impl ResultKind {
    pub fn selectivity(self) -> Selectivity {
        match self {
            Self::Depth { selectivity, .. } | Self::Endgame { selectivity } => selectivity,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveScore {
    pub action: Action,
    pub score: i32,
}

/// Scores are disc differences for the side to move.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SearchResult {
    /// None only at the end of the game. A search stopped before any
    /// iteration completed still returns a legal action. For a mask upper
    /// bound it is a legal representative, without any optimality claim.
    pub best_move: Option<Action>,
    /// None when no iteration completed.
    pub score: Option<i32>,
    /// True when the last iteration only proved an upper bound for the mask.
    pub score_is_upper_bound: bool,
    /// The last completed iteration; None with `score`.
    pub kind: Option<ResultKind>,
    pub pv: Vec<Action>,
    /// Scores best first for the root moves requested by `all_moves` or
    /// `review_move`, after the final iteration completed.
    pub moves: Vec<MoveScore>,
    /// Lines corresponding to `moves`, when requested by `all_moves_with_pv`.
    pub move_pvs: Vec<Vec<Action>>,
    pub stop_reason: StopReason,
    pub stats: SearchStats,
    pub elapsed: Duration,
    /// Root squares included in this result (zero at a forced pass/terminal).
    pub root_moves: u64,
    /// True when legal root moves were deliberately excluded.
    pub root_restricted: bool,
}
impl SearchResult {
    /// The terminal disc difference reached by replaying the PV, from the
    /// root side's perspective. Returns None for a partial or illegal PV.
    pub fn pv_final_score(&self, position: impl Into<crate::Board>) -> Option<i32> {
        super::pv::terminal_score(position.into(), &self.pv)
    }
    /// Whether `score` is the exact game value: a full-selectivity endgame
    /// iteration completed.
    pub fn is_solved(&self) -> bool {
        !self.root_restricted && self.is_subset_solved()
    }
    /// Exact maximum over the selected root moves. Does not certify excluded
    /// moves; callers must retain the mask when backing this result up.
    pub fn is_subset_solved(&self) -> bool {
        !self.score_is_upper_bound
            && self.kind
                == Some(ResultKind::Endgame {
                    selectivity: Selectivity::Full,
                })
    }
}

/// Reported after every completed iteration.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Progress {
    pub kind: ResultKind,
    pub score: i32,
    pub best_move: Option<Action>,
    pub pv: Vec<Action>,
    /// Nodes so far, across all threads.
    pub nodes: u64,
    pub elapsed: Duration,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum EngineError {
    InvalidOptions(&'static str),
    InvalidRequest(&'static str),
    InvalidCalibration(&'static str),
    Memory(&'static str),
    Thread(std::io::Error),
    Model(std::io::Error),
    /// `EngineWorker::start` while a search is running.
    Busy,
}
impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions(s)
            | Self::InvalidRequest(s)
            | Self::InvalidCalibration(s)
            | Self::Memory(s) => f.write_str(s),
            Self::Thread(e) => write!(f, "search worker could not start: {e}"),
            Self::Model(e) => write!(f, "engine model: {e}"),
            Self::Busy => f.write_str("a search is already running"),
        }
    }
}
impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Thread(e) | Self::Model(e) => Some(e),
            _ => None,
        }
    }
}

/// The multiples of a rule's deviation that each selective stage cuts at
/// (`ProbCutSigmas::STAGES`), for midgame rules and for exact rules. A stage
/// at probability p should cut where a fraction 1 - p of the standardized
/// prediction errors |deep - predicted| / deviation lie beyond. The default
/// assumes normal errors; measured errors have heavier tails, so a
/// calibration supplies the empirical quantiles instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbCutSigmas {
    pub midgame: [f64; 5],
    pub exact: [f64; 5],
}
impl ProbCutSigmas {
    /// The stage of each multiple, in the order of the arrays.
    pub const STAGES: [Selectivity; 5] = [
        Selectivity::Percent83,
        Selectivity::Percent85,
        Selectivity::Percent95,
        Selectivity::Percent98,
        Selectivity::Percent99,
    ];
    /// Two-sided normal quantiles of the stage probabilities.
    pub const NORMAL: Self = Self {
        midgame: NORMAL_SIGMAS,
        exact: NORMAL_SIGMAS,
    };
    /// The multiple of the stage with the table tag `quality`, 1..=5.
    pub(crate) fn get(&self, exact: bool, quality: u8) -> f64 {
        let stage = usize::from(quality) - 1;
        if exact {
            self.exact[stage]
        } else {
            self.midgame[stage]
        }
    }
    pub(crate) fn is_valid(&self) -> bool {
        [self.midgame, self.exact].iter().all(|s| {
            s.iter().all(|v| v.is_finite() && *v > 0.0) && s.windows(2).all(|w| w[0] <= w[1])
        })
    }
}
impl Default for ProbCutSigmas {
    fn default() -> Self {
        Self::NORMAL
    }
}
const NORMAL_SIGMAS: [f64; 5] = [1.37220, 1.43953, 1.960, 2.326, 2.576];

/// Advanced per-position calibration overriding the regression configuration.
#[derive(Clone, Copy, Debug)]
pub struct ProbCutRule {
    pub exact: bool,
    pub depth: u8,
    pub empties: u8,
    pub shallow_depth: u8,
    pub slope: f64,
    pub intercept: f64,
    pub deviation: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_are_ordered_and_tagged_consistently() {
        // `SearchPlan` keeps one limit per stage, indexed by the stage.
        for (index, stage) in Selectivity::ALL.into_iter().enumerate() {
            assert_eq!(stage as usize, index);
        }
        assert!(Selectivity::ALL.windows(2).all(|w| w[0] < w[1]));
        assert!(
            Selectivity::ALL
                .windows(2)
                .all(|w| w[0].percent() < w[1].percent())
        );
        assert_eq!(Selectivity::ALL[..5], ProbCutSigmas::STAGES);
        for stage in Selectivity::ALL {
            assert_eq!(Selectivity::from_percent(stage.percent()), Some(stage));
        }
        assert_eq!(Selectivity::from_percent(68), None);
        assert_eq!(Selectivity::from_percent(77), None);
        // Table tags: zero for a full search, otherwise the multiple's slot + 1.
        assert_eq!(Selectivity::Full.quality(), 0);
        for (slot, stage) in ProbCutSigmas::STAGES.into_iter().enumerate() {
            assert_eq!(usize::from(stage.quality()), slot + 1);
            let mut sigmas = ProbCutSigmas::NORMAL;
            sigmas.midgame[slot] = 0.5;
            sigmas.exact[slot] = 0.25;
            assert_eq!(sigmas.get(false, stage.quality()), 0.5);
            assert_eq!(sigmas.get(true, stage.quality()), 0.25);
        }
        assert!(ProbCutSigmas::NORMAL.is_valid());
        for pair in Selectivity::ALL.windows(2) {
            assert_eq!(pair[0].next(), pair[1]);
        }
        assert_eq!(Selectivity::Full.next(), Selectivity::Full);
    }
}
