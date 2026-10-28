use super::*;

pub(super) struct Control {
    pub(super) reason: AtomicU8,
    pub(super) stop: StopHandle,
    pub(super) deadline: Option<Instant>,
    pub(super) node_limit: Option<u64>,
    pub(super) issued: AtomicU64,
}
impl Control {
    pub(super) fn set(&self, reason: u8) {
        let _ = self
            .reason
            .compare_exchange(0, reason, Ordering::AcqRel, Ordering::Relaxed);
    }
    pub(super) fn check(&self) -> bool {
        if self.stop.is_stopped() {
            self.set(1)
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.set(2)
        }
        self.reason.load(Ordering::Acquire) != 0
    }
    pub(super) fn reason(&self) -> StopReason {
        match self.reason.load(Ordering::Acquire) {
            1 => StopReason::Requested,
            2 => StopReason::TimeLimit,
            3 => StopReason::NodeLimit,
            4 => StopReason::WorkerFailed,
            _ => StopReason::Completed,
        }
    }
}
pub(super) struct Runtime {
    pub(super) model: Arc<PatternModel>,
    pub(super) table: Arc<Table>,
    pub(super) pv_table: Arc<Table>,
    pub(super) pool: Arc<Pool>,
    pub(super) control: Control,
    pub(super) generation: u8,
    pub(super) stats: Mutex<SearchStats>,
    pub(super) pending: AtomicUsize,
    pub(super) pending_lock: Mutex<()>,
    pub(super) pending_ready: std::sync::Condvar,
    pub(super) parallel: bool,
    pub(super) sigma: AtomicU64,
    pub(super) quality: AtomicU8,
    pub(super) rules: Arc<[Option<ProbCutRule>]>,
    /// Fewest empties at which the local endgame tries ProbCut.
    pub(super) endgame_probcut_min: u8,
}
impl Runtime {
    pub(super) fn sigma(&self) -> Option<f64> {
        let bits = self.sigma.load(Ordering::Relaxed);
        (bits != 0).then(|| f64::from_bits(bits))
    }
    /// Called only after all workers from the preceding iteration have joined.
    pub(super) fn set_stage(&self, sigma: Option<f64>, quality: u8) {
        self.sigma
            .store(sigma.map_or(0, f64::to_bits), Ordering::Relaxed);
        self.quality.store(quality, Ordering::Relaxed);
    }

    pub(super) fn drain(&self) {
        let mut guard = parallel::lock(&self.pending_lock);
        while self.pending.load(Ordering::Acquire) != 0 {
            let (next, _) = self
                .pending_ready
                .wait_timeout(guard, Duration::from_millis(5))
                .unwrap_or_else(|p| p.into_inner());
            guard = next;
        }
    }
}
#[derive(Clone)]
pub(super) struct Line {
    pub(super) score: i32,
    pub(super) actions: [Option<Action>; 128],
    pub(super) len: usize,
}
impl Line {
    pub(super) fn leaf(score: i32) -> Self {
        Self {
            score,
            actions: [None; 128],
            len: 0,
        }
    }
    pub(super) fn prepend(action: Action, child: &Self) -> Self {
        let mut line = Self::leaf(-child.score);
        line.actions[0] = Some(action);
        line.len = (child.len + 1).min(128);
        line.actions[1..line.len].copy_from_slice(&child.actions[..line.len - 1]);
        line
    }
    pub(super) fn moves(&self) -> Vec<Action> {
        self.actions[..self.len].iter().flatten().copied().collect()
    }
}
#[derive(Clone, Copy)]
pub(super) struct Position {
    pub(super) board: Board,
    pub(super) state: EvalState,
}
#[derive(Clone, Copy)]
pub(super) struct Candidate {
    pub(super) square: Square,
    pub(super) flips: u64,
    pub(super) ordering: i32,
}
impl Candidate {
    pub(super) fn child(self, parent: Position, update_eval: bool) -> Position {
        let bit = self.square.bit();
        Position {
            board: parent.board.after_known_move(bit, self.flips),
            state: if update_eval {
                parent.state.played(bit, self.flips)
            } else {
                parent.state
            },
        }
    }
    pub(super) fn action(self) -> Action {
        Action::Place(self.square)
    }
}
pub(super) struct Context {
    pub(super) runtime: Arc<Runtime>,
    pub(super) stats: SearchStats,
    pub(super) ancestors: Vec<Arc<Helper>>,
    pub(super) cuts: Vec<Arc<AtomicBool>>,
    pub(super) end_cache: EndgameCache,
    #[cfg(feature = "endgame-pattern-ordering")]
    pub(super) end_ordering: [Option<(Board, EvalState)>; 3],
}
impl Drop for Context {
    fn drop(&mut self) {
        parallel::lock(&self.runtime.stats).merge(&self.stats)
    }
}

#[derive(Clone, Copy)]
pub(super) struct SearchSpec {
    pub(super) depth: u8,
    pub(super) exact: bool,
    pub(super) probcut: bool,
}

#[cfg(test)]
pub(super) fn test_context() -> Context {
    let workers = Workers::new(0).unwrap();
    let runtime = Arc::new(Runtime {
        model: Arc::new(PatternModel::zero()),
        table: Arc::new(Table::new(1).unwrap()),
        pv_table: Arc::new(Table::new(1).unwrap()),
        pool: workers.pool.clone(),
        control: Control {
            reason: AtomicU8::new(0),
            stop: StopHandle::new(),
            deadline: None,
            node_limit: None,
            issued: AtomicU64::new(0),
        },
        generation: 1,
        stats: Mutex::new(SearchStats::default()),
        pending: AtomicUsize::new(0),
        pending_lock: Mutex::new(()),
        pending_ready: std::sync::Condvar::new(),
        parallel: false,
        sigma: AtomicU64::new(0),
        quality: AtomicU8::new(0),
        rules: Arc::from([]),
        endgame_probcut_min: super::endgame::PROBCUT_MIN_EMPTIES,
    });
    Context::new(runtime, Vec::new(), Vec::new())
}
