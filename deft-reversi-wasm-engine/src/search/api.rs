use super::*;

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
    /// Browser search runs on one thread.
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
/// How strongly multi-ProbCut prunes: the confidence that a pruned move is
/// really outside the window. `Full` does not prune.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Selectivity {
    Percent68,
    Percent77,
    Percent85,
    Percent95,
    Percent98,
    Percent99,
    #[default]
    Full,
}
impl Selectivity {
    pub(crate) fn sigma(self) -> Option<f64> {
        [
            Some(1.0),
            Some(1.20),
            Some(1.43953),
            Some(1.960),
            Some(2.326),
            Some(2.576),
            None,
        ][self as usize]
    }
    pub fn percent(self) -> u8 {
        [68, 77, 85, 95, 98, 99, 100][self as usize]
    }
    pub(crate) fn from_level(level: u8) -> Self {
        [
            Self::Percent68,
            Self::Percent77,
            Self::Percent85,
            Self::Percent95,
            Self::Percent98,
            Self::Percent99,
            Self::Full,
        ][level as usize]
    }
}

/// What a search reads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// That many moves ahead, 0..=60. Passes are not counted.
    Depth(u8),
    /// To the end of the game; an exact solve at `Selectivity::Full`.
    Endgame,
    /// A reference-engine level, 1..=60, which also chooses the depth, the
    /// switch to the endgame and the selectivity.
    Level(u8),
}

/// Built with `SearchRequest::new(goal)` and the setters below.
#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub(super) goal: Goal,
    pub(super) selectivity: Option<Selectivity>,
    pub(super) time_limit: Option<Duration>,
    pub(super) node_limit: Option<u64>,
    pub(super) all_moves: bool,
    pub(super) stop: StopHandle,
}
impl SearchRequest {
    pub fn new(goal: Goal) -> Self {
        Self {
            goal,
            selectivity: None,
            time_limit: None,
            node_limit: None,
            all_moves: false,
            stop: StopHandle::new(),
        }
    }
    /// Pruning for `Depth` and `Endgame`; `Full` when not set. A `Level`
    /// chooses its own, so setting one there is an invalid request.
    pub fn selectivity(mut self, selectivity: Selectivity) -> Self {
        self.selectivity = Some(selectivity);
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
    /// Also scores every legal move with a full window (`SearchResult::moves`).
    /// This searches each move again, so it takes several times longer.
    pub fn all_moves(mut self) -> Self {
        self.all_moves = true;
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
    /// iteration completed still returns a legal action.
    pub best_move: Option<Action>,
    /// None when no iteration completed.
    pub score: Option<i32>,
    /// The last completed iteration; None with `score`.
    pub kind: Option<ResultKind>,
    pub pv: Vec<Action>,
    /// Every legal move with its score, best first, when the request asked
    /// for `all_moves` and the search completed.
    pub moves: Vec<MoveScore>,
    pub stop_reason: StopReason,
    pub stats: SearchStats,
    pub elapsed: Duration,
}
impl SearchResult {
    /// Whether `score` is the exact game value: a full-selectivity endgame
    /// iteration completed.
    pub fn is_solved(&self) -> bool {
        self.kind
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
