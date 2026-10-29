use crate::{
    Action, Board, Model, ModelMetadata, Square,
    evaluation::{EvalState, PatternModel},
    mpc::MpcConfig,
    parallel::{self, Helper, Padded, Pool, Workers},
    transposition::{Entry, Table},
};
use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

mod api;
pub use api::*;
mod session;
use session::*;
mod endgame;
use endgame::{EndgameCache, new_cache_epoch};
mod level;
mod ordering;
mod pv;
mod root;
mod tree;
mod ybwc;

/// Single-threaded search engine for a Web Worker.
pub struct Engine {
    model: Arc<PatternModel>,
    mpc: Option<MpcConfig>,
    metadata: Option<ModelMetadata>,
    table: Arc<Table>,
    pv_table: Arc<Table>,
    workers: Workers,
    options: EngineOptions,
    generation: u8,
    calibration: Arc<[Option<ProbCutRule>]>,
    cache_sigma: Option<u64>,
    end_cache: EndgameCache,
    /// Tags the local endgame caches that are valid for the current state.
    cache_epoch: u64,
}
impl Engine {
    pub fn new(model: Model, options: EngineOptions) -> Result<Self, EngineError> {
        if options.threads != 1 {
            return Err(EngineError::InvalidOptions(
                "WASM engine requires one thread",
            ));
        }
        let (table, pv_table) = new_tables(options.tt_mib)?;
        let workers = Workers::new(0).map_err(EngineError::Thread)?;
        let Model {
            metadata,
            pattern,
            mpc,
        } = model;
        Ok(Self {
            model: Arc::new(pattern),
            calibration: indexed_rules(mpc.rules()),
            mpc: Some(mpc),
            metadata,
            table,
            pv_table,
            workers,
            options,
            generation: 0,
            cache_sigma: None,
            end_cache: EndgameCache::default(),
            cache_epoch: new_cache_epoch(),
        })
    }
    pub fn options(&self) -> EngineOptions {
        self.options
    }
    pub fn metadata(&self) -> Option<&ModelMetadata> {
        self.metadata.as_ref()
    }
    /// Replaces the model and its MPC configuration, and clears every table.
    pub fn set_model(&mut self, model: Model) -> Result<(), EngineError> {
        let Model {
            metadata,
            pattern,
            mpc,
        } = model;
        self.set_rules(mpc.rules(), Some(mpc))?;
        self.model = Arc::new(pattern);
        self.metadata = metadata;
        Ok(())
    }
    /// Forgets every searched position, for example before a new game.
    pub fn clear_tt(&mut self) -> Result<(), EngineError> {
        (self.table, self.pv_table) = new_tables(self.options.tt_mib)?;
        self.invalidate_end_caches();
        Ok(())
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
        self.run(job, &mut on_progress)
    }
    #[cfg(feature = "tuning")]
    pub fn mpc_config(&self) -> Option<&MpcConfig> {
        self.mpc.as_ref()
    }
    #[cfg(feature = "tuning")]
    pub fn set_mpc_config(&mut self, config: MpcConfig) -> Result<(), EngineError> {
        config.validate().map_err(EngineError::Model)?;
        self.set_rules(config.rules(), Some(config))
    }
    /// Use the MPC table from the legacy main-branch evaluator. This is an
    /// explicit opt-in for models converted from main's `data/eval/eval.json`.
    #[cfg(feature = "tuning")]
    pub fn set_legacy_main_mpc(&mut self) -> Result<(), EngineError> {
        self.set_rules(crate::mpc::legacy_main_rules(), None)
    }
    #[cfg(feature = "tuning")]
    pub fn set_probcut_rules(&mut self, rules: Vec<ProbCutRule>) -> Result<(), EngineError> {
        for (i, r) in rules.iter().enumerate() {
            if r.empties > 60
                || r.depth > r.empties
                || (r.exact && r.depth != r.empties)
                || r.depth > 60
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
            if rules[..i]
                .iter()
                .any(|p| p.exact == r.exact && p.depth == r.depth && p.empties == r.empties)
            {
                return Err(EngineError::InvalidCalibration(
                    "duplicate ProbCut regression",
                ));
            }
        }
        self.set_rules(rules, None)
    }
    /// Installs new ProbCut rules. Every cached result depends on them, so
    /// the tables are replaced first; on failure nothing changes.
    fn set_rules(
        &mut self,
        rules: Vec<ProbCutRule>,
        mpc: Option<MpcConfig>,
    ) -> Result<(), EngineError> {
        self.clear_tt()?;
        self.calibration = indexed_rules(rules);
        self.mpc = mpc;
        self.cache_sigma = None;
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
            Goal::Level(level) => {
                if !(1..=60).contains(&level) {
                    return Err(EngineError::InvalidRequest("level must be 1..=60"));
                }
                if request.selectivity.is_some() {
                    return Err(EngineError::InvalidRequest(
                        "a level chooses its own selectivity",
                    ));
                }
                let (mode, selectivity) = level::level_plan(board.empty_count(), level);
                (mode, selectivity, Some(level))
            }
        };
        if selectivity != Selectivity::Full && self.calibration.is_empty() {
            return Err(EngineError::InvalidCalibration(
                "ProbCut requires model-specific calibration",
            ));
        }
        Ok(Job {
            board,
            mode,
            selectivity,
            level,
            time_limit: request.time_limit,
            node_limit: request.node_limit,
            all_moves: request.all_moves,
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
            let passed = Job {
                board: job.board.board_after_pass(),
                all_moves: false,
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
            }
            result.elapsed = pass_start.elapsed();
            return Ok(result);
        }
        let target_sigma = job.selectivity.sigma();
        // Reuse only caches with the same calibration and sigma.
        let sigma_key = target_sigma.map(f64::to_bits);
        if self.cache_sigma != sigma_key {
            self.clear_tt()?;
            self.cache_sigma = sigma_key;
        }
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
        let mut context = Context::new(runtime.clone(), Vec::new(), Vec::new());
        // Owned by the active context; retained by this Engine between requests.
        context.end_cache = std::mem::take(&mut self.end_cache);
        let mut last = None;
        for iteration in self.plan(&job) {
            runtime.drain();
            runtime.set_stage(iteration.sigma, iteration.quality);
            let prediction = result.score.unwrap_or_else(|| self.model.evaluate(&board));
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                root.iteration(&mut context, iteration.spec(), prediction)
            }));
            let completed = match attempt {
                Ok(Ok(line)) if !context.aborted() => Some(line),
                Ok(_) => None,
                Err(_) => {
                    runtime.control.fail();
                    None
                }
            };
            let Some(line) = completed else { break };
            iteration.record(&mut result, &line);
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
            });
        }
        runtime.drain();
        // Full TT cutoffs return scores without a recursion-built PV. Recover
        // the line separately, verifying each suggested move against the value.
        if !context.aborted()
            && let Some(last) = last
            && last.sigma.is_none()
            && last.depth > 0
            && let Some(score) = result.score
            && let Ok(pv) =
                context.extract_pv(board, score, last.depth, last.exact, result.best_move)
        {
            result.pv = pv;
        }
        runtime.drain();
        if job.all_moves
            && !context.aborted()
            && let Some(last) = last
            && last.depth > 0
            && let Ok(moves) = root.move_scores(&mut context, last.spec())
            && !context.aborted()
        {
            result.moves = moves;
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
    fn runtime(&self, job: &Job, deadline: Option<Instant>) -> Arc<Runtime> {
        let target_sigma = job.selectivity.sigma();
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
            parallel: false,
            // Each iteration sets its own stage before it starts.
            sigma: AtomicU64::new(target_sigma.map_or(0, f64::to_bits)),
            quality: AtomicU8::new(0),
            rules: self.calibration.clone(),
            // Exact requests use selective stages only to seed the tables;
            // results below this bound are exact and reused by the final stage.
            cache_epoch: self.cache_epoch,
            endgame_probcut_min: if job.mode == SearchMode::Exact {
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
        let target_sigma = job.selectivity.sigma();
        let target = match job.mode {
            SearchMode::Depth(d) => d.min(board.empty_count()),
            SearchMode::Exact | SearchMode::SelectiveEndgame => board.empty_count(),
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
        let calibrated = !self.calibration.is_empty();
        let target_stage = job.selectivity as u8;
        let target_quality = if target_sigma.is_none() {
            0
        } else {
            target_stage + 1
        };
        // Eval iterations all use the requested selectivity. Only endgame
        // preparation fixes it to the reference's level 2.
        let prepare = calibrated && exact;
        let mut iterations: Vec<Iteration> = depths
            .into_iter()
            .map(|depth| Iteration {
                depth,
                exact: false,
                sigma: if prepare {
                    Selectivity::Percent85.sigma()
                } else {
                    target_sigma
                },
                quality: if prepare { 3 } else { target_quality },
            })
            .collect();
        if prepare {
            let mut stage = target_stage.min(2);
            while stage < target_stage {
                iterations.push(Iteration {
                    depth: target,
                    exact: true,
                    sigma: Selectivity::from_level(stage).sigma(),
                    quality: stage + 1,
                });
                stage = next_selectivity(stage, target_stage, target);
            }
        }
        if exact {
            iterations.push(Iteration {
                depth: target,
                exact: true,
                sigma: target_sigma,
                quality: target_quality,
            });
        }
        iterations
    }
}

/// A request resolved against its root position.
struct Job {
    board: Board,
    mode: SearchMode,
    selectivity: Selectivity,
    /// Set for `Goal::Level`; it also caps the endgame preparation depth.
    level: Option<u8>,
    time_limit: Option<Duration>,
    node_limit: Option<u64>,
    all_moves: bool,
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
        kind: None,
        pv: Vec::new(),
        moves: Vec::new(),
        stop_reason: StopReason::Completed,
        stats: SearchStats::default(),
        elapsed: Duration::ZERO,
    }
}

/// Whether an iteration searched moves; a depth-0 one only evaluates.
fn searched(kind: ResultKind) -> bool {
    !matches!(kind, ResultKind::Depth { depth: 0, .. })
}

/// One root iteration of `Engine::search`.
#[derive(Clone, Copy)]
struct Iteration {
    depth: u8,
    /// Searches to the end of the game rather than to an evaluation.
    exact: bool,
    sigma: Option<f64>,
    /// TT selectivity tag: 0 exact, otherwise the reference level plus one.
    quality: u8,
}
impl Iteration {
    fn spec(self) -> SearchSpec {
        SearchSpec {
            depth: self.depth,
            exact: self.exact,
            probcut: true,
        }
    }
    /// Makes `line`, the completed iteration's result, the search result.
    fn record(self, result: &mut SearchResult, line: &Line) {
        let selectivity = if self.sigma.is_none() {
            Selectivity::Full
        } else {
            Selectivity::from_level(self.quality - 1)
        };
        result.kind = Some(if self.exact {
            ResultKind::Endgame { selectivity }
        } else {
            ResultKind::Depth {
                depth: self.depth,
                selectivity,
            }
        });
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

/// Reference solver: begin at 5/6 when possible, retaining target parity.
fn iterative_depths(target: u8) -> Vec<u8> {
    if target <= 1 {
        return vec![target];
    }
    let start = (6 - (target & 1)).min(target - 2);
    let start = if start == 0 { 2 - (target & 1) } else { start };
    (start..=target).step_by(2).collect()
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

fn next_selectivity(current: u8, target: u8, empties: u8) -> u8 {
    let next = current + 1;
    if (empties < 21 && next >= 1)
        || (empties < 24 && next >= 2)
        || (empties < 27 && next >= 3)
        || (empties < 30 && next >= 4)
    {
        target
    } else {
        next.min(target)
    }
}

impl Engine {
    /// Writes the evaluation weights in the DPR2 format. With
    /// `write_analysis_settings`, this identifies analyses that are comparable,
    /// for example to tag the values stored in an opening book.
    pub fn write_evaluation_model(&self, out: impl std::io::Write) -> std::io::Result<()> {
        self.model.write_to(out)
    }

    /// Serialize the effective analysis settings in a stable order.
    /// Includes the compiled evaluation variant and expanded calibration rules.
    pub fn write_analysis_settings(&self, mut out: impl std::io::Write) -> std::io::Result<()> {
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
        Ok(())
    }
}
