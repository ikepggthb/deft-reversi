use crate::{
    Action, Board, Model, Square,
    evaluation::{EvalState, PatternModel},
    parallel::{self, Helper, Padded, Pool, Workers},
    transposition::{Entry, Table},
};
use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

mod api;
pub use api::*;
mod session;
use session::*;
mod endgame;
use endgame::{EndgameCache, new_cache_epoch};
mod level;
pub use level::level_reading;
mod ordering;
mod pv;
mod root;
mod tree;
mod ybwc;

/// Owns the model, the transposition tables and the worker threads. One
/// search runs at a time; `EngineWorker` runs them on a thread of its own.
pub struct Engine {
    model: Arc<PatternModel>,
    table: Arc<Table>,
    pv_table: Arc<Table>,
    workers: Workers,
    options: EngineOptions,
    generation: u8,
    calibration: Arc<[Option<ProbCutRule>]>,
    sigmas: ProbCutSigmas,
    end_cache: EndgameCache,
    /// Tags the local endgame caches that are valid for the current state.
    cache_epoch: u64,
}
impl Engine {
    pub fn new(model: Model, options: EngineOptions) -> Result<Self, EngineError> {
        if options.threads == 0 || options.threads > 256 {
            return Err(EngineError::InvalidOptions("threads must be 1..=256"));
        }
        let (table, pv_table) = new_tables(options.tt_mib)?;
        let workers = Workers::new(options.threads - 1).map_err(EngineError::Thread)?;
        let (rules, sigmas) = split_calibration(model.calibration);
        Ok(Self {
            model: Arc::new(model.pattern),
            calibration: indexed_rules(rules),
            sigmas,
            table,
            pv_table,
            workers,
            options,
            generation: 0,
            end_cache: EndgameCache::default(),
            cache_epoch: new_cache_epoch(),
        })
    }
    /// Independent search state sharing immutable model/calibration weights.
    /// Tables and workers obey the supplied budgets and are not duplicated
    /// from the source. Useful for a bounded pool of independent batch jobs.
    pub fn fork(&self, options: EngineOptions) -> Result<Self, EngineError> {
        if options.threads == 0 || options.threads > 256 {
            return Err(EngineError::InvalidOptions("threads must be 1..=256"));
        }
        let (table, pv_table) = new_tables(options.tt_mib)?;
        Ok(Self {
            model: self.model.clone(),
            calibration: self.calibration.clone(),
            sigmas: self.sigmas,
            table,
            pv_table,
            workers: Workers::new(options.threads - 1).map_err(EngineError::Thread)?,
            options,
            generation: 0,
            end_cache: EndgameCache::default(),
            cache_epoch: new_cache_epoch(),
        })
    }
    /// Reconfigure idle search resources. Existing tables survive unchanged
    /// sizes; changing worker count joins old workers before creating new ones.
    pub fn set_resources(&mut self, options: EngineOptions) -> Result<(), EngineError> {
        if options.threads == 0 || options.threads > 256 {
            return Err(EngineError::InvalidOptions("threads must be 1..=256"));
        }
        if options.tt_mib != self.options.tt_mib {
            (self.table, self.pv_table) = new_tables(options.tt_mib)?;
            self.invalidate_end_caches();
            self.options.tt_mib = options.tt_mib;
        }
        if options.threads != self.options.threads {
            self.workers = Workers::new(0).map_err(EngineError::Thread)?;
            self.options.threads = 1;
            self.workers = Workers::new(options.threads - 1).map_err(EngineError::Thread)?;
        }
        self.options = options;
        Ok(())
    }
    pub fn options(&self) -> EngineOptions {
        self.options
    }
    /// Replaces the model and its ProbCut calibration, and clears every table.
    pub fn set_model(&mut self, model: Model) -> Result<(), EngineError> {
        let (rules, sigmas) = split_calibration(model.calibration);
        self.set_rules(rules, sigmas)?;
        self.model = Arc::new(model.pattern);
        Ok(())
    }
    /// Forgets every searched position, for example before a new game.
    pub fn clear_tt(&mut self) -> Result<(), EngineError> {
        (self.table, self.pv_table) = new_tables(self.options.tt_mib)?;
        self.invalidate_end_caches();
        Ok(())
    }
    /// Resolves the requested depth/mode and selectivity without searching.
    /// A plan is a target, not evidence that a search completed or solved a game.
    /// This validates the goal but does not require MPC calibration; calibration
    /// is checked when an actual selective search is requested.
    pub fn search_plan(
        &self,
        position: impl Into<Board>,
        request: &SearchRequest,
    ) -> Result<ResultKind, EngineError> {
        let job = self.resolve(position.into(), request)?;
        Ok(match job.mode {
            SearchMode::Depth(depth) => ResultKind::Depth {
                depth: depth.min(job.board.empty_count()),
                selectivity: job.selectivity,
            },
            SearchMode::Exact | SearchMode::SelectiveEndgame => ResultKind::Endgame {
                selectivity: job.selectivity,
            },
        })
    }
    /// Searches `position`, blocking the calling thread.
    pub fn search(
        &mut self,
        position: impl Into<Board>,
        request: &SearchRequest,
    ) -> Result<SearchResult, EngineError> {
        self.search_with(position, request, |_| {})
    }
    /// Like `search`, calling `on_progress` after every completed iteration
    /// on the calling thread.
    pub fn search_with(
        &mut self,
        position: impl Into<Board>,
        request: &SearchRequest,
        mut on_progress: impl FnMut(&Progress),
    ) -> Result<SearchResult, EngineError> {
        let job = self.resolve(position.into(), request)?;
        if job.selectivity != Selectivity::Full && self.calibration.is_empty() {
            return Err(EngineError::InvalidCalibration(
                "ProbCut requires model-specific calibration",
            ));
        }
        self.run(job, &mut on_progress)
    }
    /// Use the MPC table from the legacy main-branch evaluator. This is an
    /// explicit opt-in for models converted from main's `data/eval/eval.json`.
    #[cfg(feature = "tuning")]
    pub fn set_legacy_main_mpc(&mut self) -> Result<(), EngineError> {
        self.set_rules(crate::mpc::legacy_main_rules(), ProbCutSigmas::NORMAL)
    }
    /// Installs ProbCut rules with the normal deviation multiples.
    #[cfg(feature = "tuning")]
    pub fn set_probcut_rules(&mut self, rules: Vec<ProbCutRule>) -> Result<(), EngineError> {
        self.set_probcut_calibration(rules, ProbCutSigmas::NORMAL)
    }
    /// Installs ProbCut rules with measured deviation multiples per stage.
    #[cfg(feature = "tuning")]
    pub fn set_probcut_calibration(
        &mut self,
        rules: Vec<ProbCutRule>,
        sigmas: ProbCutSigmas,
    ) -> Result<(), EngineError> {
        validate_calibration(&rules, &sigmas)?;
        self.set_rules(rules, sigmas)
    }
    /// Installs new ProbCut rules. Every cached result depends on them, so
    /// the tables are replaced first; on failure nothing changes.
    fn set_rules(
        &mut self,
        rules: Vec<ProbCutRule>,
        sigmas: ProbCutSigmas,
    ) -> Result<(), EngineError> {
        self.clear_tt()?;
        self.calibration = indexed_rules(rules);
        self.sigmas = sigmas;
        Ok(())
    }
    /// Drops the main thread's local endgame cache and, through a new epoch,
    /// every worker's.
    fn invalidate_end_caches(&mut self) {
        self.end_cache.clear();
        self.cache_epoch = new_cache_epoch();
    }
    /// Checks a request and resolves its goal against the root position.
    fn resolve(&self, board: Board, request: &SearchRequest) -> Result<Job, EngineError> {
        let (mode, selectivity, level) = match request.goal {
            Goal::Depth(depth) if depth > 60 => {
                return Err(EngineError::InvalidRequest("depth must be 0..=60"));
            }
            Goal::Depth(depth) => (
                SearchMode::Depth(depth),
                request.selectivity.unwrap_or_default(),
                None,
            ),
            Goal::Endgame => {
                let selectivity = request.selectivity.unwrap_or_default();
                let mode = if selectivity == Selectivity::Full {
                    SearchMode::Exact
                } else {
                    SearchMode::SelectiveEndgame
                };
                (mode, selectivity, None)
            }
            Goal::Level(_) | Goal::Plan(_) => {
                let plan = match request.goal {
                    Goal::Plan(plan) => plan,
                    Goal::Level(level) => SearchPlan::level(level)?,
                    _ => unreachable!(),
                };
                plan.validate()?;
                if request.selectivity.is_some() {
                    return Err(EngineError::InvalidRequest(
                        "a level or plan chooses its own selectivity",
                    ));
                }
                let (mode, selectivity) = plan.at(board.empty_count());
                (mode, selectivity, Some(plan.midgame_depth()))
            }
        };
        if let Some(mask) = request.root_moves {
            let legal = board.get_moves();
            if mask & !legal != 0 || (legal != 0 && mask == 0) {
                return Err(EngineError::InvalidRequest(
                    "root move mask must be a nonempty legal subset",
                ));
            }
        }
        if request.review_move.is_some_and(|square| {
            request.root_moves.unwrap_or_else(|| board.get_moves()) & square.bit() == 0
        }) {
            return Err(EngineError::InvalidRequest(
                "review move must be in the legal root mask",
            ));
        }
        if request.minimum_score.is_some_and(|score| {
            !(-63..=64).contains(&score)
                || request.all_moves
                || request.review_move.is_some()
                || board.get_moves() == 0
                || !matches!(mode, SearchMode::Depth(d) if d > 0 && d < board.empty_count())
        }) {
            return Err(EngineError::InvalidRequest(
                "minimum score requires a positive midgame depth and legal root moves",
            ));
        }
        Ok(Job {
            minimum_score: request.minimum_score,
            reuse_pv: request.reuse_pv,
            recover_endgame_pv: request.recover_endgame_pv,
            recover_pv: request.recover_pv,
            skip_pv_recovery: request.skip_pv_recovery,
            root_moves: request.root_moves,
            preferred_move: request.preferred_move,
            board,
            mode,
            selectivity,
            level,
            time_limit: request.time_limit,
            timed_levels: request.timed_levels && !self.calibration.is_empty(),
            node_limit: request.node_limit,
            all_moves: request.all_moves,
            move_pvs: request.move_pvs,
            review_move: request.review_move,
            stop: request.stop.clone(),
        })
    }
    fn run(
        &mut self,
        job: Job,
        on_progress: &mut dyn FnMut(&Progress),
    ) -> Result<SearchResult, EngineError> {
        // The reference handles a forced root pass before starting its driver.
        if job.board.must_pass() {
            let pass_start = Instant::now();
            let all_moves = job.all_moves;
            let move_pvs = job.move_pvs;
            let passed = Job {
                board: job.board.board_after_pass(),
                all_moves: false,
                move_pvs: false,
                root_moves: None,
                preferred_move: None,
                ..job
            };
            let mut forward = |progress: &Progress| {
                let mut progress = progress.clone();
                progress.score = -progress.score;
                progress.best_move = Some(Action::Pass);
                if searched(progress.kind) {
                    progress.pv.insert(0, Action::Pass);
                }
                on_progress(&progress);
            };
            let mut result = self.run(passed, &mut forward)?;
            result.score = result.score.map(|score| -score);
            result.best_move = Some(Action::Pass);
            if result.kind.is_some_and(searched) {
                result.pv.insert(0, Action::Pass);
            }
            if all_moves
                && result.stop_reason == StopReason::Completed
                && let Some(score) = result.score
            {
                result.moves = vec![MoveScore {
                    action: Action::Pass,
                    score,
                }];
                if move_pvs {
                    let pv = if result.pv.is_empty() {
                        vec![Action::Pass]
                    } else {
                        result.pv.clone()
                    };
                    result.move_pvs.push(pv);
                }
            }
            result.root_moves = 0;
            result.root_restricted = false;
            result.elapsed = pass_start.elapsed();
            return Ok(result);
        }
        // Entries carry their stage (`quality`) and exact mode, and a cut
        // uses only an entry from the same stage, so searches at other
        // stages keep the tables; a calibration change clears them.
        self.generation = self.generation.wrapping_add(1);
        let start = Instant::now();
        let deadline = match job.time_limit {
            Some(d) => Some(
                start
                    .checked_add(d)
                    .ok_or(EngineError::InvalidRequest("time limit overflow"))?,
            ),
            None => None,
        };
        let board = job.board;
        let mut result = initial_result(board);
        let mask = job.root_moves.unwrap_or_else(|| board.get_moves());
        result.root_moves = mask;
        result.root_restricted = mask != board.get_moves();
        if mask != 0 {
            result.best_move = Some(Action::Place(Square::lowest(mask)));
        }
        if result.best_move.is_none() {
            result.score = Some(board.final_score());
            result.kind = Some(ResultKind::Endgame {
                selectivity: Selectivity::Full,
            });
            result.elapsed = start.elapsed();
            return Ok(result);
        }
        let runtime = self.runtime(&job, deadline);
        let state = EvalState::from_board(board);
        let mut root = root::RootSearch::new(Position { board, state });
        root.restrict(mask, job.preferred_move);
        let mut context = Context::new(runtime.clone(), Vec::new(), Vec::new());
        // Owned by the active context; retained by this Engine between requests.
        context.end_cache = std::mem::take(&mut self.end_cache);
        let mut last = None;
        let plan = self.plan(&job);
        let warm = self.warm_start(&job, mask, &plan);
        let (skip, mut prediction) = if let Some((skip, square, score)) = warm {
            // If time expires in the first uncached iteration, the compatible
            // cached move is a better fallback than the lowest legal square.
            result.best_move = Some(Action::Place(square));
            root.restrict(mask, Some(square));
            context.stats.reused_iterations += skip as u64;
            (skip, score)
        } else {
            (0, self.model.evaluate(&board))
        };
        for (index, &iteration) in plan.iter().enumerate().skip(skip) {
            runtime.drain();
            runtime.set_stage(iteration.selectivity);
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if index + 1 == plan.len()
                    && let Some(minimum) = job.minimum_score
                    && root.below_minimum(&mut context, iteration.spec(), minimum)?
                {
                    return Ok((Line::leaf(minimum - 1), true));
                }
                root.iteration(&mut context, iteration.spec(), prediction)
                    .map(|line| (line, false))
            }));
            let completed = match attempt {
                Ok(Ok(line)) if !context.aborted() => Some(line),
                Ok(_) => None,
                Err(_) => {
                    runtime.control.fail();
                    None
                }
            };
            let Some((line, upper)) = completed else {
                break;
            };
            iteration.record(&mut result, &line);
            if upper {
                result.score_is_upper_bound = true;
                result.best_move = Some(Action::Place(Square::lowest(mask)));
                break;
            }
            prediction = line.score;
            last = Some(iteration);
            // Helpers merge their counters when they finish.
            runtime.drain();
            on_progress(&Progress {
                kind: result.kind.unwrap(),
                score: line.score,
                best_move: result.best_move,
                pv: result.pv.clone(),
                nodes: context.stats.nodes + parallel::lock(&runtime.stats).nodes,
                elapsed: start.elapsed(),
                next: plan.get(index + 1).map(|next| next.kind()),
                moves: Vec::new(),
                root: root.stats,
            });
        }
        runtime.drain();
        // Full TT cutoffs return scores without a recursion-built PV. Recover
        // the line separately, verifying each suggested move against the value.
        if !job.skip_pv_recovery
            && !result.score_is_upper_bound
            && !context.aborted()
            && let Some(last) = last
            && (job.review_move.is_none() || job.all_moves)
            && last.depth > 0
            && let Some(score) = result.score
        {
            let full = last.selectivity == Selectivity::Full;
            let selective_recovery = if last.exact {
                (job.recover_endgame_pv || job.recover_pv)
                    && result.pv_final_score(board) != Some(score)
            } else {
                job.recover_pv && result.pv.len() < usize::from(last.depth)
            };
            if (full || selective_recovery)
                && let Ok(pv) =
                    context.extract_pv(board, score, last.depth, last.exact, result.best_move)
                && (full
                    || pv::terminal_score(board, &pv) == Some(score)
                    || (result.pv_final_score(board).is_none() && pv.len() > result.pv.len()))
            {
                result.pv = pv;
            }
        }
        runtime.drain();
        let score_mask = if job.all_moves {
            mask
        } else if let (Some(played), Some(Action::Place(best))) =
            (job.review_move, result.best_move)
        {
            played.bit() | best.bit()
        } else {
            0
        };
        if score_mask != 0
            && !context.aborted()
            && let Some(last) = last
            && last.depth > 0
            && let Ok(moves) = root.move_scores(
                &mut context,
                last.spec(),
                score_mask,
                job.move_pvs,
                &mut |context, moves| {
                    on_progress(&Progress {
                        kind: result.kind.unwrap(),
                        score: result.score.unwrap_or_default(),
                        best_move: result.best_move,
                        pv: result.pv.clone(),
                        nodes: context.stats.nodes + parallel::lock(&runtime.stats).nodes,
                        elapsed: start.elapsed(),
                        next: None,
                        moves: moves.to_vec(),
                        root: RootStats::default(),
                    })
                },
            )
            && !context.aborted()
        {
            for (score, pv) in moves {
                result.moves.push(score);
                if job.move_pvs {
                    let pv = if result.best_move == Some(score.action)
                        && result.score == Some(score.score)
                        && result.pv.len() > pv.len()
                    {
                        result.pv.clone()
                    } else {
                        pv
                    };
                    result.move_pvs.push(pv);
                }
            }
        }
        runtime.drain();
        if runtime.control.reason() != StopReason::WorkerFailed {
            self.end_cache = std::mem::take(&mut context.end_cache);
        }
        drop(context);
        result.stop_reason = runtime.control.reason();
        result.stats = parallel::lock(&runtime.stats).clone();
        result.elapsed = start.elapsed();
        Ok(result)
    }
    fn warm_start(&self, job: &Job, mask: u64, plan: &[Iteration]) -> Option<(usize, Square, i32)> {
        if !job.reuse_pv || plan.len() < 2 {
            return None;
        }
        [self.pv_table.get(job.board), self.table.get(job.board)]
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let best = Square::new(entry.best)?;
                if mask & best.bit() == 0 || entry.lower != entry.upper {
                    return None;
                }
                // Only skip iterations of the same search mode and MPC stage.
                // Retain the target iteration even for a matching deeper entry.
                let index = plan[..plan.len() - 1].iter().rposition(|iteration| {
                    entry.exact_mode == iteration.exact
                        && entry.quality == iteration.selectivity.quality()
                        && entry.depth >= iteration.depth
                })?;
                Some((index + 1, best, i32::from(entry.lower)))
            })
            .max_by_key(|(skip, _, _)| *skip)
    }
    fn runtime(&self, job: &Job, deadline: Option<Instant>) -> Arc<Runtime> {
        Arc::new(Runtime {
            model: self.model.clone(),
            table: self.table.clone(),
            pv_table: self.pv_table.clone(),
            pool: self.workers.pool.clone(),
            control: Control {
                reason: AtomicU8::new(0),
                stop: job.stop.clone(),
                deadline,
                node_limit: job.node_limit,
                issued: Padded(AtomicU64::new(0)),
            },
            generation: self.generation,
            stats: Padded(Mutex::new(SearchStats::default())),
            pending: Padded(AtomicUsize::new(0)),
            pending_lock: Mutex::new(()),
            pending_ready: Padded(std::sync::Condvar::new()),
            parallel: self.options.threads > 1,
            split_empties: tree::split_empties(self.options.threads),
            // Each iteration sets its own stage before it starts.
            quality: AtomicU8::new(0),
            rules: self.calibration.clone(),
            sigmas: self.sigmas,
            // The timed ladder treats selective stages as level goals, using
            // their own ProbCut range. A fixed exact search uses them as preparation.
            cache_epoch: self.cache_epoch,
            endgame_probcut_min: if job.mode == SearchMode::Exact && !job.timed_levels {
                endgame::EXACT_PREPARATION_PROBCUT_MIN
            } else {
                endgame::PROBCUT_MIN_EMPTIES
            },
        })
    }
    /// The iterative deepening schedule: evaluation iterations, then for
    /// endgame requests the selective stages and the final solve.
    fn plan(&self, job: &Job) -> Vec<Iteration> {
        let board = job.board;
        if job.timed_levels {
            let max_level = job.level.expect("timed levels use a level goal");
            return timed_level_iterations(max_level, board.empty_count());
        }
        let target = match job.mode {
            SearchMode::Depth(d) => d.min(board.empty_count()),
            SearchMode::Exact | SearchMode::SelectiveEndgame => board.empty_count(),
        };
        let midgame = |depth, selectivity| Iteration {
            depth,
            exact: false,
            selectivity,
        };
        let endgame = |selectivity| Iteration {
            depth: board.empty_count(),
            exact: true,
            selectivity,
        };
        let exact = matches!(job.mode, SearchMode::Exact | SearchMode::SelectiveEndgame);
        let depths = if exact {
            // Match the reference's endgame preparation depth and two-ply steps.
            let warmup = target
                .saturating_sub(8)
                .clamp(2, 24)
                .min(target)
                .min(job.level.unwrap_or(60));
            iterative_depths(warmup)
        } else {
            iterative_depths(target)
        };
        // Evaluation iterations use the requested selectivity. Before an
        // endgame search they only prepare the tables and the move order,
        // so they run at the least accurate stage, and the endgame then
        // steps from 85% up to the requested stage.
        let prepare = exact && !self.calibration.is_empty();
        let warmup = if prepare {
            Selectivity::Percent83
        } else {
            job.selectivity
        };
        let mut iterations: Vec<Iteration> = depths
            .into_iter()
            .map(|depth| midgame(depth, warmup))
            .collect();
        if prepare {
            let mut stage = job.selectivity.min(Selectivity::Percent85);
            while stage < job.selectivity {
                iterations.push(endgame(stage));
                stage = next_stage(stage, job.selectivity, target);
            }
        }
        if exact {
            iterations.push(endgame(job.selectivity));
        }
        iterations
    }
}

/// A request resolved against its root position.
struct Job {
    minimum_score: Option<i32>,
    reuse_pv: bool,
    recover_endgame_pv: bool,
    recover_pv: bool,
    skip_pv_recovery: bool,
    board: Board,
    mode: SearchMode,
    selectivity: Selectivity,
    /// The midgame depth of a `Goal::Level` or `Goal::Plan`; it also caps the
    /// endgame preparation depth.
    level: Option<u8>,
    time_limit: Option<Duration>,
    timed_levels: bool,
    node_limit: Option<u64>,
    all_moves: bool,
    move_pvs: bool,
    review_move: Option<Square>,
    root_moves: Option<u64>,
    preferred_move: Option<Square>,
    stop: StopHandle,
}

fn new_tables(tt_mib: usize) -> Result<(Arc<Table>, Arc<Table>), EngineError> {
    let table = Table::new(tt_mib).map_err(EngineError::Memory)?;
    let pv_table = Table::new((tt_mib / 16).max(1)).map_err(EngineError::Memory)?;
    Ok((Arc::new(table), Arc::new(pv_table)))
}

/// The result before any iteration completes: the first legal action, if any.
fn initial_result(board: Board) -> SearchResult {
    let moves = board.get_moves();
    let best_move = if moves != 0 {
        Some(Action::Place(Square::lowest(moves)))
    } else if board.get_opponent_moves() != 0 {
        Some(Action::Pass)
    } else {
        None
    };
    SearchResult {
        best_move,
        score: None,
        score_is_upper_bound: false,
        kind: None,
        pv: Vec::new(),
        moves: Vec::new(),
        move_pvs: Vec::new(),
        stop_reason: StopReason::Completed,
        stats: SearchStats::default(),
        elapsed: Duration::ZERO,
        root_moves: moves,
        root_restricted: false,
    }
}

/// Whether an iteration searched moves; a depth-0 one only evaluates.
fn searched(kind: ResultKind) -> bool {
    !matches!(kind, ResultKind::Depth { depth: 0, .. })
}

/// One root iteration of `Engine::search`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Iteration {
    depth: u8,
    /// Searches to the end of the game rather than to an evaluation.
    exact: bool,
    selectivity: Selectivity,
}
impl Iteration {
    fn spec(self) -> SearchSpec {
        SearchSpec {
            depth: self.depth,
            exact: self.exact,
            probcut: true,
        }
    }
    fn kind(self) -> ResultKind {
        let selectivity = self.selectivity;
        if self.exact {
            ResultKind::Endgame { selectivity }
        } else {
            ResultKind::Depth {
                depth: self.depth,
                selectivity,
            }
        }
    }
    /// Makes `line`, the completed iteration's result, the search result.
    fn record(self, result: &mut SearchResult, line: &Line) {
        result.kind = Some(self.kind());
        result.score = Some(line.score);
        result.pv = line.moves();
        if let Some(action) = result.pv.first() {
            result.best_move = Some(*action)
        }
        // Depth-0 evaluates the position without claiming a searched best move.
        if self.depth == 0 {
            result.pv.clear();
        }
    }
}

fn timed_level_iterations(max_level: u8, empties: u8) -> Vec<Iteration> {
    let mut steps: Vec<_> = (1..=max_level)
        .map(|level| {
            let (mode, selectivity) = level::plan(level).at(empties);
            let (depth, exact) = match mode {
                SearchMode::Depth(depth) => (depth, false),
                SearchMode::Exact | SearchMode::SelectiveEndgame => (empties, true),
            };
            Iteration {
                depth,
                exact,
                selectivity,
            }
        })
        .collect();
    steps.dedup();
    let endgame_start = steps.partition_point(|step| !step.exact);
    if let Some(&target) = steps[..endgame_start].last() {
        // Keep the depth at the table's transition, including its parity.
        steps.splice(
            ..endgame_start,
            iterative_depths(target.depth)
                .into_iter()
                .map(|depth| Iteration { depth, ..target }),
        );
    }
    steps
}

/// Reference solver: begin at 5/6 when possible, retaining target parity.
fn iterative_depths(target: u8) -> Vec<u8> {
    if target <= 1 {
        return vec![target];
    }
    let start = (6 - (target & 1)).min(target - 2);
    let start = if start == 0 { 2 - (target & 1) } else { start };
    (start..=target).step_by(2).collect()
}

fn split_calibration(c: Option<crate::model::Calibration>) -> (Vec<ProbCutRule>, ProbCutSigmas) {
    c.map_or((Vec::new(), ProbCutSigmas::NORMAL), |c| (c.rules, c.sigmas))
}

/// Checks ProbCut rules and multiples the way every installer does: rules
/// within the board, a shallow depth below the deep one, finite positive
/// slopes and deviations, no triple twice, and positive non-decreasing
/// multiples.
pub(crate) fn validate_calibration(
    rules: &[ProbCutRule],
    sigmas: &ProbCutSigmas,
) -> Result<(), EngineError> {
    if !sigmas.is_valid() {
        return Err(EngineError::InvalidCalibration(
            "ProbCut multiples must be positive, finite and non-decreasing",
        ));
    }
    let mut seen = vec![false; 2 * 61 * 61];
    for r in rules {
        if r.empties > 60
            || r.depth > r.empties
            || (r.exact && r.depth != r.empties)
            || r.depth < 2
            || r.shallow_depth >= r.depth
            || !r.slope.is_finite()
            || r.slope <= 0.0
            || !r.intercept.is_finite()
            || !r.deviation.is_finite()
            || r.deviation < 0.0
        {
            return Err(EngineError::InvalidCalibration(
                "invalid ProbCut regression",
            ));
        }
        let i = rule_index(r.exact, r.depth, r.empties);
        if std::mem::replace(&mut seen[i], true) {
            return Err(EngineError::InvalidCalibration(
                "duplicate ProbCut regression",
            ));
        }
    }
    Ok(())
}

fn rule_index(exact: bool, depth: u8, empties: u8) -> usize {
    (usize::from(exact) * 61 + usize::from(depth)) * 61 + usize::from(empties)
}

fn aspiration_window(pivot: i32, left: i32, right: i32, exact: bool) -> (i32, i32) {
    let mut alpha = pivot - left;
    let mut beta = pivot + right;
    if exact {
        alpha -= alpha.rem_euclid(2);
        beta += (-beta).rem_euclid(2);
    }
    (alpha.max(-65), beta.min(65))
}

fn indexed_rules(rules: Vec<ProbCutRule>) -> Arc<[Option<ProbCutRule>]> {
    let mut indexed = if rules.is_empty() {
        Vec::new()
    } else {
        vec![None; 2 * 61 * 61]
    };
    for rule in rules {
        indexed[rule_index(rule.exact, rule.depth, rule.empties)] = Some(rule);
    }
    indexed.into()
}

/// The endgame stage after `current`, which is 85% or above, on the way to
/// `target`: with fewer than 27 empties the search goes straight to the
/// target, and with fewer than 30 it does so after 95%.
fn next_stage(current: Selectivity, target: Selectivity, empties: u8) -> Selectivity {
    let next = current.next();
    if empties < 27 || (empties < 30 && next >= Selectivity::Percent98) {
        target
    } else {
        next.min(target)
    }
}

impl Engine {
    /// Writes the evaluation weights, with the pattern embedding networks if
    /// the model has them, in the DPR2 format (no calibration). With
    /// `write_analysis_settings`, this identifies analyses that are comparable,
    /// for example to tag the values stored in an opening book.
    pub fn write_evaluation_model(&self, out: impl std::io::Write) -> std::io::Result<()> {
        self.model.write_to(out)
    }

    /// Serialize the effective analysis settings in a stable order.
    /// Includes the search compatibility revision, compiled evaluation variant,
    /// and expanded calibration rules.
    pub fn write_analysis_settings(&self, mut out: impl std::io::Write) -> std::io::Result<()> {
        out.write_all(b"deft-search-semantics")?;
        out.write_all(&SEARCH_SEMANTICS_REVISION.to_le_bytes())?;
        out.write_all(&[u8::from(cfg!(feature = "endgame-pattern-ordering"))])?;
        for rule in self.calibration.iter().flatten() {
            out.write_all(&[
                u8::from(rule.exact),
                rule.depth,
                rule.empties,
                rule.shallow_depth,
            ])?;
            for value in [rule.slope, rule.intercept, rule.deviation] {
                out.write_all(&value.to_bits().to_le_bytes())?;
            }
        }
        // Normal multiples write nothing, so earlier settings stay the same.
        if self.sigmas != ProbCutSigmas::NORMAL {
            out.write_all(b"sigmas")?;
            for value in self.sigmas.midgame.iter().chain(&self.sigmas.exact) {
                out.write_all(&value.to_bits().to_le_bytes())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn forks_share_only_immutable_weights_and_stable_resize_keeps_tables() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let fork = engine
            .fork(EngineOptions::default().threads(2).tt_mib(1))
            .unwrap();
        assert!(Arc::ptr_eq(&engine.model, &fork.model));
        assert!(Arc::ptr_eq(&engine.calibration, &fork.calibration));
        assert!(!Arc::ptr_eq(&engine.table, &fork.table));
        assert!(!Arc::ptr_eq(&engine.pv_table, &fork.pv_table));
        let table = engine.table.clone();
        engine.set_resources(engine.options()).unwrap();
        assert!(Arc::ptr_eq(&engine.table, &table));
    }
}

#[cfg(test)]
mod timed_level_tests {
    use super::*;

    #[test]
    fn depth_and_endgame_stages_follow_the_level_table() {
        use Selectivity::*;
        for (empties, level, first_depth, max_depth, endgame) in [
            (7, 60, 1, 0, &[Full][..]),
            (14, 1, 1, 1, &[]),
            (14, 60, 2, 4, &[Percent85, Full]),
            (24, 60, 6, 18, &[Percent85, Percent98, Percent99, Full]),
            (28, 21, 5, 21, &[]),
            (28, 22, 5, 21, &[Percent85]),
            (
                28,
                60,
                5,
                21,
                &[Percent85, Percent95, Percent98, Percent99, Full],
            ),
            (
                40,
                60,
                6,
                36,
                &[Percent85, Percent95, Percent98, Percent99, Full],
            ),
            (60, 60, 6, 56, &[Percent85, Percent98, Percent99, Full]),
        ] {
            let expected: Vec<_> = (first_depth..=max_depth)
                .step_by(2)
                .map(|depth| Iteration {
                    depth,
                    exact: false,
                    selectivity: Percent83,
                })
                .chain(endgame.iter().map(|&selectivity| Iteration {
                    depth: empties,
                    exact: true,
                    selectivity,
                }))
                .collect();
            assert_eq!(timed_level_iterations(level, empties), expected);
        }
    }
}

#[cfg(test)]
mod warm_start_tests {
    use super::*;

    #[test]
    fn warm_start_keeps_the_target_and_requires_the_cached_move_in_the_mask() {
        let mut engine = Engine::new(
            crate::Model::from_pattern(crate::PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let board = Board::new();
        let best = board.legal_moves().last().unwrap();
        engine.pv_table.store(
            board,
            crate::transposition::Entry {
                lower: 0,
                upper: 0,
                depth: 6,
                quality: Selectivity::Full.quality(),
                best: best.index(),
                second: 255,
                generation: 0,
                exact_mode: false,
            },
        );
        let request = SearchRequest::new(Goal::Depth(8)).reuse_pv();
        let stopped = engine
            .search(board, &request.clone().time_limit(Duration::ZERO))
            .unwrap();
        assert_eq!(stopped.best_move, Some(Action::Place(best)));
        assert_eq!(stopped.kind, None);
        assert_eq!(stopped.score, None);

        let excluded = engine
            .search(
                board,
                &request.clone().root_moves(board.get_moves() & !best.bit()),
            )
            .unwrap();
        assert_eq!(excluded.stats.reused_iterations, 0);

        let warmed = engine.search(board, &request).unwrap();
        assert_eq!(warmed.stats.reused_iterations, 1);
        assert_eq!(
            warmed.kind,
            Some(ResultKind::Depth {
                depth: 8,
                selectivity: Selectivity::Full,
            })
        );
        assert_eq!(warmed.score, Some(0));
        assert!(!warmed.score_is_upper_bound);
    }
}
