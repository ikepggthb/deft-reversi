use super::tree::pick_best;
use super::*;

impl Context {
    pub(super) fn split(
        &mut self,
        position: Position,
        candidates: Vec<Candidate>,
        window: (i32, i32),
        spec: SearchSpec,
        best: Line,
        root_bounds: Option<Arc<Mutex<[root::RootBound; 64]>>>,
    ) -> Result<Line, ()> {
        let (alpha, beta) = window;
        let SearchSpec {
            depth,
            exact,
            probcut: allow_probcut,
        } = spec;
        let point = Arc::new(Split {
            position,
            root_bounds,
            state: Mutex::new(SplitState {
                candidates,
                next: 0,
                alpha,
                best,
            }),
            beta,
            depth,
            exact,
            allow_probcut,
            cut: Arc::new(Padded(AtomicBool::new(false))),
            helper: Arc::new(Helper::new()),
            active: AtomicUsize::new(0),
        });
        // Unwind must join descendants too: a queued handoff can otherwise retain
        // its own ancestor slot with nobody left to execute it.
        let _join_on_unwind = JoinOnUnwind {
            point: point.clone(),
            runtime: self.runtime.clone(),
        };
        let mut ancestors = self.ancestors.clone();
        ancestors.push(point.helper.clone());
        let mut cuts = self.cuts.clone();
        cuts.push(point.cut.clone());
        let mut spawned = 0;
        // Offer every remaining move but the master's own; only idle threads
        // accept, so a split takes as many helpers as are free and useful.
        let wanted = parallel::lock(&point.state).candidates.len() - 1;
        for _ in 0..wanted {
            let runtime = self.runtime.clone();
            let task_point = point.clone();
            let task_ancestors = ancestors.clone();
            let task_cuts = cuts.clone();
            point.active.fetch_add(1, Ordering::AcqRel);
            self.runtime.pending.fetch_add(1, Ordering::AcqRel);
            let job: parallel::Job = Box::new(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut context = Context::new(runtime.clone(), task_ancestors, task_cuts);
                    context.end_cache = EndgameCache::take_thread(runtime.cache_epoch);
                    context.stats.helper_jobs += 1;
                    context.split_loop(&task_point);
                    std::mem::take(&mut context.end_cache).put_thread(runtime.cache_epoch);
                }));
                if result.is_err() {
                    runtime.control.reason.store(4, Ordering::Release);
                }
                task_point.active.fetch_sub(1, Ordering::AcqRel);
                task_point.helper.wake();
                runtime.finish_pending();
            });
            match self.runtime.pool.offer(job, &self.ancestors) {
                Ok(dispatch) => {
                    spawned += 1;
                    match dispatch {
                        parallel::Dispatch::Ancestor => self.stats.ancestor_handoffs += 1,
                        parallel::Dispatch::Worker => self.stats.worker_dispatches += 1,
                    }
                }
                Err(_) => {
                    point.active.fetch_sub(1, Ordering::AcqRel);
                    self.runtime.pending.fetch_sub(1, Ordering::AcqRel);
                    break;
                }
            }
        }
        if spawned != 0 {
            self.stats.splits += 1;
        }
        // The master searches too, then only accepts descendant work while joining.
        let old_ancestors = std::mem::replace(&mut self.ancestors, ancestors);
        let old_cuts = std::mem::replace(&mut self.cuts, cuts);
        self.split_loop(&point);
        let join_start = Instant::now();
        // Jobs this thread runs while joining share its cache.
        let epoch = self.runtime.cache_epoch;
        std::mem::take(&mut self.end_cache).put_thread(epoch);
        point
            .helper
            .wait(|| point.active.load(Ordering::Acquire) == 0);
        self.end_cache = EndgameCache::take_thread(epoch);
        self.stats.join_time += join_start.elapsed();
        self.ancestors = old_ancestors;
        self.cuts = old_cuts;
        if self.aborted() {
            return Err(());
        }
        let result = parallel::lock(&point.state).best.clone();
        Ok(result)
    }
    fn split_loop(&mut self, point: &Split) {
        loop {
            if self.aborted() {
                break;
            }
            let next = {
                let mut state = parallel::lock(&point.state);
                if state.next == state.candidates.len() || state.alpha >= point.beta {
                    None
                } else {
                    let index = state.next;
                    pick_best(&mut state.candidates, index);
                    let candidate = state.candidates[index];
                    state.next += 1;
                    Some((candidate, state.alpha))
                }
            };
            let Some((candidate, alpha)) = next else {
                break;
            };
            let Ok(Some(line)) = self.sibling(
                point.position,
                candidate,
                alpha,
                point.beta,
                SearchSpec {
                    depth: point.depth,
                    exact: point.exact,
                    probcut: point.allow_probcut,
                },
            ) else {
                break;
            };
            if let Some(bounds) = &point.root_bounds {
                parallel::lock(bounds)[candidate.square.index() as usize]
                    .update(line.score, alpha, point.beta);
            }
            let mut state = parallel::lock(&point.state);
            if line.score > state.best.score {
                state.best = line;
            }
            state.alpha = state.alpha.max(state.best.score);
            if state.alpha >= point.beta {
                point.cut.store(true, Ordering::Release);
                self.stats.beta_cuts += 1;
                point.helper.wake();
                break;
            }
        }
    }
}
struct SplitState {
    candidates: Vec<Candidate>,
    next: usize,
    alpha: i32,
    best: Line,
}
// Cloned for every helper job; keep the reference counts off the shared state.
#[repr(align(128))]
struct Split {
    position: Position,
    root_bounds: Option<Arc<Mutex<[root::RootBound; 64]>>>,
    state: Mutex<SplitState>,
    beta: i32,
    depth: u8,
    exact: bool,
    allow_probcut: bool,
    cut: Arc<Padded<AtomicBool>>,
    helper: Arc<Helper>,
    active: AtomicUsize,
}

struct JoinOnUnwind {
    point: Arc<Split>,
    runtime: Arc<Runtime>,
}
impl Drop for JoinOnUnwind {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.runtime.control.reason.store(4, Ordering::Release);
            self.point.cut.store(true, Ordering::Release);
            self.point
                .helper
                .wait(|| self.point.active.load(Ordering::Acquire) == 0);
        }
    }
}
