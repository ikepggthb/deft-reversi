use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    Depth(u8),
    /// Exact final result. Calibrated selective preparation may precede it.
    Exact,
    /// Terminal-depth search with explicitly calibrated probabilistic cuts.
    SelectiveEndgame,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Completed,
    Requested,
    TimeLimit,
    NodeLimit,
    WorkerFailed,
}
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
#[derive(Clone, Copy, Debug)]
pub struct EngineOptions {
    pub threads: usize,
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
/// Reference selectivity levels 0..=6. Full disables MPC in the final pass.
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
    pub fn sigma(self) -> Option<f64> {
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

#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub board: Board,
    pub mode: SearchMode,
    pub time_limit: Option<Duration>,
    pub node_limit: Option<u64>,
    pub stop: StopHandle,
    /// Optional custom sigma override; None uses `selectivity`. Exact requires
    /// Full for the final pass, but may use selective preparation.
    pub probcut_sigma: Option<f64>,
    /// Preferred MPC setting: the same seven levels as the reference engine.
    pub selectivity: Selectivity,
    pub(super) level: Option<u8>,
}
impl SearchRequest {
    pub fn new(board: Board, mode: SearchMode) -> Self {
        Self {
            board,
            mode,
            time_limit: None,
            node_limit: None,
            stop: StopHandle::new(),
            probcut_sigma: None,
            selectivity: if mode == SearchMode::SelectiveEndgame {
                Selectivity::Percent85
            } else {
                Selectivity::Full
            },
            level: None,
        }
    }
}
#[derive(Clone, Debug, Default)]
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
#[derive(Clone, Debug)]
pub struct SearchResult {
    /// None means terminal; Pass is explicit, including on interrupted requests.
    pub action: Option<Action>,
    pub score: Option<i32>,
    pub bound: Option<Bound>,
    pub requested_mode: SearchMode,
    pub mode: SearchMode,
    pub selective: bool,
    /// Actual completed stage. None denotes an explicit custom sigma override.
    pub selectivity: Option<Selectivity>,
    pub probcut_sigma: Option<f64>,
    pub completed_depth: u8,
    pub pv: Vec<Action>,
    pub stop_reason: StopReason,
    pub stats: SearchStats,
    pub elapsed: Duration,
}
#[derive(Debug)]
pub enum EngineError {
    InvalidOptions(&'static str),
    InvalidRequest(&'static str),
    InvalidCalibration(&'static str),
    Memory(&'static str),
    Thread(std::io::Error),
    Model(std::io::Error),
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
