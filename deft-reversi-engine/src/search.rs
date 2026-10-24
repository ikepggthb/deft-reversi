use crate::{
    Action, Board, EngineModel, ModelMetadata, MpcConfig, PatternModel, Square,
    evaluation::EvalState,
    parallel::{self, Helper, Pool, Workers},
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
mod last_four;
mod level;
mod ordering;
mod pv;
mod root;
mod small;
mod tail;
mod tree;
mod ybwc;

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
    end_cache: Vec<Vec<EndEntry>>,
}
impl Engine {
    pub fn new(model: PatternModel, options: EngineOptions) -> Result<Self, EngineError> {
        if options.threads == 0 || options.threads > 256 {
            return Err(EngineError::InvalidOptions("threads must be 1..=256"));
        }
        let table = Arc::new(Table::new(options.tt_mib).map_err(EngineError::Memory)?);
        let pv_table =
            Arc::new(Table::new((options.tt_mib / 16).max(1)).map_err(EngineError::Memory)?);
        let workers = Workers::new(options.threads - 1).map_err(EngineError::Thread)?;
        let mpc = MpcConfig::default();
        let calibration = indexed_rules(mpc.rules());
        Ok(Self {
            model: Arc::new(model),
            mpc: Some(mpc),
            metadata: None,
            table,
            pv_table,
            workers,
            options,
            generation: 0,
            calibration,
            cache_sigma: None,
            end_cache: Vec::new(),
        })
    }
    pub fn from_file(
        path: impl AsRef<std::path::Path>,
        options: EngineOptions,
    ) -> Result<Self, EngineError> {
        Self::from_model(
            EngineModel::from_file(path).map_err(EngineError::Model)?,
            options,
        )
    }
    pub fn from_bytes(bytes: &[u8], options: EngineOptions) -> Result<Self, EngineError> {
        Self::from_model(
            EngineModel::from_bytes(bytes).map_err(EngineError::Model)?,
            options,
        )
    }
    fn from_model(model: EngineModel, options: EngineOptions) -> Result<Self, EngineError> {
        let mut engine = Self::new(model.pattern, options)?;
        engine.calibration = indexed_rules(model.mpc.rules());
        engine.mpc = Some(model.mpc);
        engine.metadata = model.metadata;
        Ok(engine)
    }
    pub fn mpc_config(&self) -> Option<&MpcConfig> {
        self.mpc.as_ref()
    }
    pub fn metadata(&self) -> Option<&ModelMetadata> {
        self.metadata.as_ref()
    }
    pub fn set_mpc_config(&mut self, config: MpcConfig) -> Result<(), EngineError> {
        config.validate().map_err(EngineError::Model)?;
        let table = Arc::new(Table::new(self.options.tt_mib).map_err(EngineError::Memory)?);
        let pv_table =
            Arc::new(Table::new((self.options.tt_mib / 16).max(1)).map_err(EngineError::Memory)?);
        self.calibration = indexed_rules(config.rules());
        self.table = table;
        self.pv_table = pv_table;
        self.end_cache.clear();
        self.cache_sigma = None;
        self.mpc = Some(config);
        Ok(())
    }
    pub fn clear_tt(&mut self) -> Result<(), EngineError> {
        let table = Arc::new(Table::new(self.options.tt_mib).map_err(EngineError::Memory)?);
        let pv_table =
            Arc::new(Table::new((self.options.tt_mib / 16).max(1)).map_err(EngineError::Memory)?);
        self.table = table;
        self.pv_table = pv_table;
        self.end_cache.clear();
        Ok(())
    }
    /// Replaces the model and invalidates all cached evaluations.
    pub fn set_model(&mut self, model: PatternModel) -> Result<(), EngineError> {
        let table = Arc::new(Table::new(self.options.tt_mib).map_err(EngineError::Memory)?);
        let pv_table =
            Arc::new(Table::new((self.options.tt_mib / 16).max(1)).map_err(EngineError::Memory)?);
        self.model = Arc::new(model);
        self.table = table;
        self.pv_table = pv_table;
        self.mpc = Some(MpcConfig::default());
        self.metadata = None;
        self.calibration = indexed_rules(self.mpc.as_ref().unwrap().rules());
        self.end_cache.clear();
        self.cache_sigma = None;
        Ok(())
    }
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
        let table = Arc::new(Table::new(self.options.tt_mib).map_err(EngineError::Memory)?);
        let pv_table =
            Arc::new(Table::new((self.options.tt_mib / 16).max(1)).map_err(EngineError::Memory)?);
        self.calibration = indexed_rules(rules);
        self.mpc = None;
        self.end_cache.clear();
        self.cache_sigma = None;
        self.table = table;
        self.pv_table = pv_table;
        Ok(())
    }
    pub fn search(&mut self, request: SearchRequest) -> Result<SearchResult, EngineError> {
        if matches!(request.mode,SearchMode::Depth(d) if d>60) {
            return Err(EngineError::InvalidRequest("depth must be 0..=60"));
        }
        let target_sigma = request
            .probcut_sigma
            .or_else(|| request.selectivity.sigma());
        if request.mode == SearchMode::Exact && target_sigma.is_some() {
            return Err(EngineError::InvalidRequest(
                "Exact forbids probabilistic cuts; use SelectiveEndgame",
            ));
        }
        if request.mode == SearchMode::SelectiveEndgame && target_sigma.is_none() {
            return Err(EngineError::InvalidRequest(
                "SelectiveEndgame requires calibrated ProbCut",
            ));
        }
        if let Some(sigma) = target_sigma
            && (!sigma.is_finite() || sigma <= 0.0 || self.calibration.is_empty())
        {
            return Err(EngineError::InvalidCalibration(
                "ProbCut requires positive sigma and model-specific calibration",
            ));
        }
        // The reference handles a forced root pass before starting its driver.
        if request.board.get_moves() == 0 && request.board.get_opponent_moves() != 0 {
            let pass_start = Instant::now();
            let mut passed = request;
            passed.board = passed.board.board_after_pass();
            let mut result = self.search(passed)?;
            result.score = result.score.map(|score| -score);
            result.action = Some(Action::Pass);
            if result.completed_depth > 0 {
                result.pv.insert(0, Action::Pass);
            }
            result.elapsed = pass_start.elapsed();
            return Ok(result);
        }
        // Reuse only caches with the same calibration and sigma.
        let sigma_key = target_sigma.map(f64::to_bits);
        if self.cache_sigma != sigma_key {
            self.clear_tt()?;
            self.cache_sigma = sigma_key;
        }
        self.generation = self.generation.wrapping_add(1);
        let start = Instant::now();
        let deadline = match request.time_limit {
            Some(d) => Some(
                start
                    .checked_add(d)
                    .ok_or(EngineError::InvalidRequest("time limit overflow"))?,
            ),
            None => None,
        };
        let runtime = Arc::new(Runtime {
            model: self.model.clone(),
            table: self.table.clone(),
            pv_table: self.pv_table.clone(),
            pool: self.workers.pool.clone(),
            control: Control {
                reason: AtomicU8::new(0),
                stop: request.stop,
                deadline,
                node_limit: request.node_limit,
                issued: AtomicU64::new(0),
            },
            generation: self.generation,
            stats: Mutex::new(SearchStats::default()),
            pending: AtomicUsize::new(0),
            pending_lock: Mutex::new(()),
            pending_ready: std::sync::Condvar::new(),
            parallel: self.options.threads > 1,
            sigma: AtomicU64::new(target_sigma.map_or(0, f64::to_bits)),
            quality: AtomicU8::new(if target_sigma.is_some() { 7 } else { 0 }),
            rules: self.calibration.clone(),
        });
        let board = request.board;
        let moves = board.get_moves();
        let fallback = if moves != 0 {
            Some(Action::Place(
                Square::new(moves.trailing_zeros() as u8).unwrap(),
            ))
        } else if board.get_opponent_moves() != 0 {
            Some(Action::Pass)
        } else {
            None
        };
        let mut result = SearchResult {
            action: fallback,
            score: None,
            bound: None,
            requested_mode: request.mode,
            mode: SearchMode::Depth(0),
            selective: target_sigma.is_some(),
            selectivity: if request.probcut_sigma.is_some() {
                None
            } else {
                Some(request.selectivity)
            },
            probcut_sigma: target_sigma,
            completed_depth: 0,
            pv: Vec::new(),
            stop_reason: StopReason::Completed,
            stats: SearchStats::default(),
            elapsed: Duration::ZERO,
        };
        if fallback.is_none() {
            result.mode = SearchMode::Exact;
            result.selective = false;
            result.selectivity = Some(Selectivity::Full);
            result.probcut_sigma = None;
            result.score = Some(board.final_score());
            result.bound = Some(Bound::Exact);
            result.elapsed = start.elapsed();
            return Ok(result);
        }
        let state = EvalState::from_board(board);
        let mut root = root::RootSearch::new(Position { board, state });
        let mut context = Context::new(runtime.clone(), Vec::new(), Vec::new());
        // Owned by the active context; retained by this Engine between requests.
        context.end_cache = std::mem::take(&mut self.end_cache);
        let target = match request.mode {
            SearchMode::Depth(d) => d.min(board.empty_count()),
            SearchMode::Exact | SearchMode::SelectiveEndgame => board.empty_count(),
        };
        let exact = matches!(
            request.mode,
            SearchMode::Exact | SearchMode::SelectiveEndgame
        );
        let depths = if exact {
            // Match the reference's endgame preparation depth and two-ply steps.
            let warmup = target
                .saturating_sub(8)
                .clamp(2, 24)
                .min(target)
                .min(request.level.unwrap_or(60));
            iterative_depths(warmup)
        } else {
            iterative_depths(target)
        };
        let calibrated = !self.calibration.is_empty();
        let target_stage = request.selectivity as u8;
        let target_quality = if target_sigma.is_none() {
            0
        } else if request.probcut_sigma.is_some() {
            7
        } else {
            target_stage + 1
        };
        let mut iterations: Vec<(u8, bool, Option<f64>, u8)> = depths
            .into_iter()
            .map(|depth| {
                // Eval iterations all use the requested selectivity. Only
                // endgame preparation fixes it to the reference's level 2.
                let prepare = calibrated && exact;
                (
                    depth,
                    false,
                    if prepare {
                        Selectivity::Percent85.sigma()
                    } else {
                        target_sigma
                    },
                    if prepare { 3 } else { target_quality },
                )
            })
            .collect();
        if calibrated && exact && request.probcut_sigma.is_none() {
            let mut stage = target_stage.min(2);
            while stage < target_stage {
                iterations.push((
                    target,
                    true,
                    Selectivity::from_level(stage).sigma(),
                    stage + 1,
                ));
                stage = next_selectivity(stage, target_stage, target);
            }
        }
        if exact {
            iterations.push((target, true, target_sigma, target_quality));
        }
        for (depth, final_iteration, sigma, quality) in iterations {
            runtime.drain();
            runtime.set_stage(sigma, quality);
            let prediction = result.score.unwrap_or_else(|| self.model.evaluate(&board));
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                root.iteration(
                    &mut context,
                    SearchSpec {
                        depth,
                        exact: final_iteration,
                        probcut: true,
                    },
                    prediction,
                )
            }));
            let completed = match attempt {
                Ok(Ok(line)) if !context.aborted() => Some(line),
                Ok(_) => None,
                Err(_) => {
                    runtime.control.reason.store(4, Ordering::Release);
                    None
                }
            };
            let Some(line) = completed else { break };
            result.selective = sigma.is_some();
            result.selectivity = if sigma.is_none() {
                Some(Selectivity::Full)
            } else if quality == 7 {
                None
            } else {
                Some(Selectivity::from_level(quality - 1))
            };
            result.probcut_sigma = sigma;
            result.mode = if final_iteration && sigma.is_some() {
                SearchMode::SelectiveEndgame
            } else if final_iteration {
                request.mode
            } else {
                SearchMode::Depth(depth)
            };
            result.score = Some(line.score);
            result.bound = Some(Bound::Exact);
            result.completed_depth = depth;
            result.pv = line.moves();
            if let Some(action) = result.pv.first() {
                result.action = Some(*action)
            }
            // Depth-0 evaluates the position without claiming a searched best move.
            if depth == 0 {
                result.pv.clear();
            }
        }
        runtime.drain();
        // Full TT cutoffs return scores without a recursion-built PV. Recover
        // the line separately, verifying each suggested move against the value.
        if !context.aborted()
            && !result.selective
            && result.completed_depth > 0
            && let Some(score) = result.score
            && let Ok(pv) = context.extract_pv(
                board,
                score,
                result.completed_depth,
                result.mode == SearchMode::Exact,
                result.action,
            )
        {
            result.pv = pv;
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
