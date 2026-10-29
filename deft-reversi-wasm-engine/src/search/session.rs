use super::*;

pub(super) struct Control {
    pub(super) reason: AtomicU8,
    pub(super) stop: StopHandle,
    pub(super) deadline: Option<Instant>,
    pub(super) node_limit: Option<u64>,
    pub(super) issued: Padded<AtomicU64>,
}
impl Control {
    /// Records the first reason to stop; later ones keep it.
    pub(super) fn set(&self, reason: StopReason) {
        let _ = self
            .reason
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Relaxed);
    }
    /// A failed worker overrides any earlier reason: its results are lost.
    pub(super) fn fail(&self) {
        self.reason
            .store(StopReason::WorkerFailed as u8, Ordering::Release);
    }
    pub(super) fn check(&self) -> bool {
        if self.stop.is_stopped() {
            self.set(StopReason::Requested)
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.set(StopReason::TimeLimit)
        }
        self.reason.load(Ordering::Acquire) != 0
    }
    pub(super) fn reason(&self) -> StopReason {
        [
            StopReason::Completed,
            StopReason::Requested,
            StopReason::TimeLimit,
            StopReason::NodeLimit,
            StopReason::WorkerFailed,
        ][usize::from(self.reason.load(Ordering::Acquire))]
    }
}
/// Read on every node by every thread; the fields written during a search
/// are padded so that they do not share its cache lines.
#[repr(align(128))]
pub(super) struct Runtime {
    pub(super) model: Arc<PatternModel>,
    pub(super) table: Arc<Table>,
    pub(super) pv_table: Arc<Table>,
    pub(super) pool: Arc<Pool>,
    pub(super) control: Control,
    pub(super) generation: u8,
    pub(super) stats: Padded<Mutex<SearchStats>>,
    pub(super) pending: Padded<AtomicUsize>,
    pub(super) pending_lock: Mutex<()>,
    pub(super) pending_ready: Padded<std::sync::Condvar>,
    pub(super) parallel: bool,
    pub(super) sigma: AtomicU64,
    pub(super) quality: AtomicU8,
    pub(super) rules: Arc<[Option<ProbCutRule>]>,
    /// Fewest empties at which the local endgame tries ProbCut.
    pub(super) endgame_probcut_min: u8,
    /// Threads reuse the local endgame cache they keep only under this epoch.
    pub(super) cache_epoch: u64,
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

    /// Ends one helper job counted in `pending`. The last one notifies under
    /// the lock `drain` holds between its check and its wait, so the wakeup
    /// cannot fall between the two.
    pub(super) fn finish_pending(&self) {
        if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _guard = parallel::lock(&self.pending_lock);
            self.pending_ready.notify_all();
        }
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
/// A search stopped by a stop request, a limit, a failed worker or a cut
/// at an ancestor split. Its partial result must not be used.
#[derive(Debug)]
pub(super) struct Aborted;
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
    pub(super) cuts: Vec<Arc<Padded<AtomicBool>>>,
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
            issued: Padded(AtomicU64::new(0)),
        },
        generation: 1,
        stats: Padded(Mutex::new(SearchStats::default())),
        pending: Padded(AtomicUsize::new(0)),
        pending_lock: Mutex::new(()),
        pending_ready: Padded(std::sync::Condvar::new()),
        parallel: false,
        sigma: AtomicU64::new(0),
        quality: AtomicU8::new(0),
        rules: Arc::from([]),
        endgame_probcut_min: super::endgame::PROBCUT_MIN_EMPTIES,
        cache_epoch: 0,
    });
    Context::new(runtime, Vec::new(), Vec::new())
}
