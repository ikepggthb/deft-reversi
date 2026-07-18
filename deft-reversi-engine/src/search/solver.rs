//! 探索ドライバ。トップレベルの探索 API を提供する。
//!
//! ## 機能
//!
//! - [`Solver::solve_eval`]: 中盤(指定 depth まで)の単発 PVS 探索
//! - [`Solver::solve_final`]: 終盤完全読み (game end まで) の単発 PVS
//! - [`Solver::solve`]: レベル指定で iterative deepening + アスピレーション窓を
//!   組む高レベル API。レベルに応じて中盤探索/終盤完全読み/MPC 強度を自動選択する。

use crate::board::board::Board;
use crate::board::constant::NO_COORD;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::eval::Evaluator;
use crate::file::EngineFile;
use crate::search::eval_search::{nws_eval, pvs_eval};
use crate::search::final_search::nws::{collect_ybwc_tasks, make_ybwc_job};
use crate::search::final_search::{nws_final, pvs_final, solve_score};
use crate::search::mpc::{MpcConfig, SELECTIVITY_LV_MAX};
use crate::search::search::{SearchContext, SearchStats};
use crate::search::thread_pool::ThreadPool;
use crate::t_table::TranspositionTable;
use crate::EngineError;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// solve() 内で使う中盤探索の MPC selectivity。
const EVAL_SOLVER_SELECTIVITY: i32 = 1;

/// solve() に指定できる最大レベル。
pub const SOLVE_LEVEL_MAX: i32 = 60;

/// solve() 内部で選択される具体的な探索構成。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverType {
    /// 中盤探索: (depth, selectivity_lv)
    Eval(i32, i32),
    /// 終盤完全読み: (selectivity_lv)
    Final(i32),
}

/// 単発探索の結果。
#[derive(Debug, Clone)]
pub struct SolverResult {
    /// 最善手の盤面座標(0-63)。パスまたは終局では None。
    pub best_move: Option<u8>,
    /// 現プレイヤー視点のスコア。
    pub score: i32,
    /// 実際に使った探索構成。
    pub solver_type: SolverType,
    /// 探索したノード数。
    pub nodes: u64,
    /// 葉ノード数。
    pub leaf_nodes: u64,
    /// TT から復元した読み筋。少なくとも root best move を含む。
    pub pv: Vec<u8>,
    /// stop フラグで中断されたか。
    pub aborted: bool,
}

pub struct SolverOptions {
    pub tt_capacity: Option<usize>,
    pub stop: Option<Arc<AtomicBool>>,
    /// 探索ノードを処理する総スレッド数。メイン探索スレッドを含む。
    pub search_threads: NonZeroUsize,
}

impl Default for SolverOptions {
    fn default() -> Self {
        Self {
            tt_capacity: None,
            stop: None,
            search_threads: NonZeroUsize::MIN,
        }
    }
}

/// 探索ドライバ。`evaluator` / `mpc` / `tt` を Arc で保持し、複数回の探索で
/// TT を再利用できる。
pub struct Solver {
    evaluator: Arc<Evaluator>,
    ordering_evaluator: Arc<Evaluator>,
    mpc: Arc<MpcConfig>,
    tt: Arc<TranspositionTable>,
    stop: Option<Arc<AtomicBool>>,
    thread_pool: Option<Arc<ThreadPool>>,
}

impl Solver {
    pub fn new(evaluator: Arc<Evaluator>) -> Self {
        Self::with_options(evaluator, SolverOptions::default())
    }

    pub fn with_options(evaluator: Arc<Evaluator>, opts: SolverOptions) -> Self {
        Self::with_mpc(evaluator, Arc::new(MpcConfig::default()), opts)
    }

    fn with_mpc(evaluator: Arc<Evaluator>, mpc: Arc<MpcConfig>, opts: SolverOptions) -> Self {
        let tt = match opts.tt_capacity {
            Some(mb) => TranspositionTable::with_mb_size(mb),
            None => TranspositionTable::new(),
        };
        Self {
            ordering_evaluator: evaluator.clone(),
            evaluator,
            mpc,
            tt: Arc::new(tt),
            stop: opts.stop,
            thread_pool: (opts.search_threads.get() > 1)
                .then(|| Arc::new(ThreadPool::new(opts.search_threads.get() - 1))),
        }
    }

    pub fn from_file(path: &str, opts: SolverOptions) -> Result<Self, EngineError> {
        let bytes = std::fs::read(path)?;
        match std::str::from_utf8(&bytes) {
            Ok(input) => Self::from_str_data(input, opts),
            Err(_) => {
                let file = EngineFile::read_file(path)
                    .map_err(|e| EngineError::InvalidData(e.to_string()))?;
                Self::from_engine_file(file, opts)
            }
        }
    }

    pub fn from_str_data(input: &str, opts: SolverOptions) -> Result<Self, EngineError> {
        match EngineFile::read_string(input) {
            Ok(file) => Self::from_engine_file(file, opts),
            Err(v3_error) => {
                let evaluator = Evaluator::from_str_data(input)
                    .map_err(|_| EngineError::InvalidData(v3_error.to_string()))?;
                Ok(Self::with_options(Arc::new(evaluator), opts))
            }
        }
    }

    fn from_engine_file(file: EngineFile, opts: SolverOptions) -> Result<Self, EngineError> {
        let (_, evaluator, mpc) = file
            .into_parts()
            .map_err(|e| EngineError::InvalidData(e.to_string()))?;
        Ok(Self::with_mpc(Arc::new(evaluator), Arc::new(mpc), opts))
    }

    pub fn mpc_config(&self) -> &MpcConfig {
        &self.mpc
    }

    pub fn set_ordering_evaluator(&mut self, ev: Evaluator) {
        self.ordering_evaluator = Arc::new(ev);
    }

    pub fn clear_tt(&self) {
        self.tt.advance_generation();
    }

    /// 中盤の PVS 探索を `depth` まで実行する。
    ///
    // Single-shot eval search entry point used by internal tests and debugging.
    #[allow(dead_code)]
    pub fn solve_eval(&self, board: &Board, depth: i32, selectivity_lv: i32) -> SolverResult {
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            self.evaluator.clone(),
            self.mpc.clone(),
            self.tt.clone(),
            &mut stats,
        )
        .with_ordering_evaluator(self.ordering_evaluator.clone())
        .with_stop(self.stop.clone())
        .with_thread_pool(self.thread_pool.clone());
        search.selectivity_lv = selectivity_lv;

        let solver_type = SolverType::Eval(depth, selectivity_lv);
        let (best_move, score) = search_root_eval(board, depth, &mut search);
        let aborted = search.is_aborted();
        drop(search);
        self.result_from_parts(best_move, score, solver_type, &stats, aborted, board)
    }

    /// 終盤完全読み(game end まで)を実行する。
    // Single-shot final search entry point used by internal tests and debugging.
    #[allow(dead_code)]
    pub fn solve_final(&self, board: &Board, selectivity_lv: i32) -> SolverResult {
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            self.evaluator.clone(),
            self.mpc.clone(),
            self.tt.clone(),
            &mut stats,
        )
        .with_ordering_evaluator(self.ordering_evaluator.clone())
        .with_stop(self.stop.clone())
        .with_thread_pool(self.thread_pool.clone());
        search.selectivity_lv = selectivity_lv;

        let solver_type = SolverType::Final(selectivity_lv);
        let (best_move, score) = search_root_final(board, &mut search);
        let aborted = search.is_aborted();
        drop(search);
        self.result_from_parts(best_move, score, solver_type, &stats, aborted, board)
    }

    /// レベル指定の高レベル探索。`level` (1..=60) に応じて中盤/終盤の構成を
    /// 自動選択し、step=4 の iterative deepening + アスピレーション窓で TT を
    /// 暖めながら最終探索を行う。
    pub fn solve(&self, board: &Board, level: i32) -> SolverResult {
        let level = level.clamp(1, SOLVE_LEVEL_MAX);

        let legal = board.moves();
        let n_empties = (board.player | board.opponent).count_zeros() as i32;
        let mut solver_type = level_to_solver_type(n_empties, level);
        if self.is_stopped() {
            let (best_move, score) = self.fallback_move(board);
            return self.result_from_parts(
                best_move,
                score,
                solver_type,
                &SearchStats::default(),
                true,
                board,
            );
        }
        if legal == 0 {
            let passed = board.passed();
            if passed.moves() == 0 {
                return SolverResult {
                    best_move: None,
                    score: solve_score(board),
                    solver_type,
                    nodes: 0,
                    leaf_nodes: 0,
                    pv: Vec::new(),
                    aborted: false,
                };
            }
            let mut r = self.solve(&passed, level);
            r.score = -r.score;
            r.best_move = None;
            return r;
        }

        // A Solver is reused across positions by the CLI. Start a fresh TT
        // generation so entries from earlier positions are replacement
        // candidates instead of competing with the current search.
        self.tt.advance_generation();

        // root 候補手リスト(再順序の効率化のため Vec で保持)。
        let mut candidates: Vec<(u8, Board)> = Vec::with_capacity(legal.count_ones() as usize);
        let mut bits = legal;
        while bits != 0 {
            let mb = bits & bits.wrapping_neg();
            bits &= bits - 1;
            let pos = mb.trailing_zeros() as u8;
            candidates.push((pos, board.make_move(mb)));
        }

        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            self.evaluator.clone(),
            self.mpc.clone(),
            self.tt.clone(),
            &mut stats,
        )
        .with_ordering_evaluator(self.ordering_evaluator.clone())
        .with_stop(self.stop.clone())
        .with_thread_pool(self.thread_pool.clone());
        // 開幕の評価は粗いので最大 MPC を使うが、深いレベルでは MPC を弱める。
        search.selectivity_lv = if level > 10 {
            EVAL_SOLVER_SELECTIVITY
        } else {
            SELECTIVITY_LV_MAX
        };

        let mut predict_score = self.evaluator.evaluate_board_slow(board);

        match &mut solver_type {
            SolverType::Eval(target_depth, selectivity) => {
                // 序盤の評価関数の精度が低いので深いレベルでは緩める。
                if board.move_count() < 20 && *target_depth > 14 {
                    *target_depth -= 4;
                    if *selectivity != SELECTIVITY_LV_MAX {
                        *selectivity = 3;
                    }
                }
                predict_score = iterative_deepening_eval(
                    *target_depth,
                    *selectivity,
                    &mut candidates,
                    predict_score,
                    &mut search,
                );
            }
            SolverType::Final(selectivity) => {
                let selectivity = *selectivity;
                // Final solver 前の中盤反復深化レベル(短く)。
                let eval_solver_lv = (n_empties - 7 - (2 - selectivity / 2))
                    .clamp(2, 24)
                    .min(level);
                predict_score = iterative_deepening_eval(
                    eval_solver_lv,
                    EVAL_SOLVER_SELECTIVITY,
                    &mut candidates,
                    predict_score,
                    &mut search,
                );
                trace_search_stage("eval", predict_score, &search);
                if search.is_aborted() {
                    drop(search);
                    return self.result_from_parts(
                        candidates[0].0,
                        predict_score,
                        solver_type,
                        &stats,
                        true,
                        board,
                    );
                }

                if eval_solver_lv >= 18 && selectivity > 5 {
                    let init_w = (10 - n_empties).max(2 + predict_score.rem_euclid(2));
                    predict_score = aspiration_search_final(
                        selectivity - 4,
                        init_w,
                        predict_score,
                        &mut candidates,
                        &mut search,
                    );
                    trace_search_stage("selective-final", predict_score, &search);
                    if search.is_aborted() {
                        drop(search);
                        return self.result_from_parts(
                            candidates[0].0,
                            predict_score,
                            solver_type,
                            &stats,
                            true,
                            board,
                        );
                    }
                }
                let init_w = (10 - n_empties).max(2 + predict_score.rem_euclid(2));
                predict_score = aspiration_search_final(
                    selectivity,
                    init_w,
                    predict_score,
                    &mut candidates,
                    &mut search,
                );
                trace_search_stage("exact-final", predict_score, &search);
            }
        }

        let aborted = search.is_aborted();
        drop(search);
        self.result_from_parts(
            candidates[0].0,
            predict_score,
            solver_type,
            &stats,
            aborted,
            board,
        )
    }

    fn is_stopped(&self) -> bool {
        self.stop
            .as_ref()
            .is_some_and(|stop| stop.load(Ordering::Relaxed))
    }

    fn fallback_move(&self, board: &Board) -> (u8, i32) {
        let legal = board.moves();
        if legal == 0 {
            return (NO_COORD, self.evaluator.evaluate_board_slow(board));
        }
        let mut best_move = NO_COORD;
        let mut best_score = -SCORE_MAX;
        let mut bits = legal;
        while bits != 0 {
            let mb = bits & bits.wrapping_neg();
            bits &= bits - 1;
            let pos = mb.trailing_zeros() as u8;
            let score = -self.evaluator.evaluate_board_slow(&board.make_move(mb));
            if score > best_score {
                best_score = score;
                best_move = pos;
            }
        }
        (best_move, best_score)
    }

    fn result_from_parts(
        &self,
        best_move: u8,
        score: i32,
        solver_type: SolverType,
        stats: &SearchStats,
        aborted: bool,
        board: &Board,
    ) -> SolverResult {
        if std::env::var_os("DEFT_MPC_STATS").is_some() {
            eprintln!(
                "MPCSTATS tries={} cuts={} high={} low={}",
                stats.mpc_tries, stats.mpc_cuts, stats.mpc_high_cuts, stats.mpc_low_cuts
            );
        }
        if std::env::var_os("DEFT_STABILITY_STATS").is_some() {
            eprintln!(
                "STABILITYSTATS tries={} cuts={}",
                stats.stability_tries, stats.stability_cuts
            );
        }
        if std::env::var_os("DEFT_YBWC_STATS").is_some() {
            eprintln!(
                "YBWCSTATS splits={} aborts={}",
                stats.ybwc_splits, stats.ybwc_split_aborts
            );
        }
        let best_move_opt = (best_move != NO_COORD).then_some(best_move);
        let pv = best_move_opt
            .filter(|pos| board.moves() & (1u64 << pos) != 0)
            .map_or_else(Vec::new, |best_move| {
                let depth = match solver_type {
                    SolverType::Eval(depth, _) => depth,
                    SolverType::Final(_) => (board.player | board.opponent).count_zeros() as i32,
                };
                self.restore_pv(board, best_move, depth)
            });
        SolverResult {
            best_move: best_move_opt,
            score,
            solver_type,
            nodes: stats.eval_search_leaf_nodes + stats.final_search_nodes,
            leaf_nodes: stats.eval_search_leaf_nodes + stats.final_search_leaf_nodes,
            pv,
            aborted,
        }
    }

    fn restore_pv(&self, board: &Board, best_move: u8, depth: i32) -> Vec<u8> {
        let max_len = depth
            .max(0)
            .min((board.player | board.opponent).count_zeros() as i32)
            as usize;
        let mut pv = Vec::with_capacity(max_len.min(64));
        let mut current = *board;
        let mut next_move = Some(best_move);

        while pv.len() < max_len {
            let moves = current.moves();
            if moves == 0 {
                if current.opponent_moves() == 0 {
                    break;
                }
                current = current.passed();
                next_move = self.tt.get(&current).map(|value| value.move0);
                continue;
            }

            let mv = match next_move.or_else(|| self.tt.get(&current).map(|value| value.move0)) {
                Some(mv) if mv != NO_COORD && (moves & (1u64 << mv)) != 0 => mv,
                _ => break,
            };
            pv.push(mv);
            current = current.make_move(1u64 << mv);
            next_move = self.tt.get(&current).map(|value| value.move0);
        }

        pv
    }
}

fn trace_search_stage(stage: &str, score: i32, search: &SearchContext) {
    if std::env::var_os("DEFT_SEARCH_TRACE").is_some() {
        eprintln!(
            "SEARCHTRACE stage={stage} score={score:+} eval_nodes={} final_nodes={} \
             mpc={}/{} stability={}/{} ybwc={}/{}",
            search.stats.eval_search_nodes,
            search.stats.final_search_nodes,
            search.stats.mpc_cuts,
            search.stats.mpc_tries,
            search.stats.stability_cuts,
            search.stats.stability_tries,
            search.stats.ybwc_split_aborts,
            search.stats.ybwc_splits,
        );
    }
}

/// レベルと残り空きマスから具体的な探索構成を決める(旧 solver の get_config 移植)。
fn level_to_solver_type(n_empties: i32, level: i32) -> SolverType {
    use SolverType::*;
    if level == 0 {
        Eval(0, SELECTIVITY_LV_MAX)
    } else if level <= 10 {
        if n_empties <= 2 * level {
            Final(SELECTIVITY_LV_MAX)
        } else {
            Eval(level, SELECTIVITY_LV_MAX)
        }
    } else if level <= 12 {
        if n_empties <= 21 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 24 {
            Final(4)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level <= 18 {
        if n_empties <= 21 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 24 {
            Final(4)
        } else if n_empties <= 27 {
            Final(2)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level <= 21 {
        if n_empties <= 24 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 27 {
            Final(4)
        } else if n_empties <= 30 {
            Final(2)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level <= 24 {
        if n_empties <= 24 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 27 {
            Final(5)
        } else if n_empties <= 30 {
            Final(3)
        } else if n_empties <= 33 {
            Final(1)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level <= 27 {
        if n_empties <= 27 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 30 {
            Final(4)
        } else if n_empties <= 33 {
            Final(2)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level < 30 {
        if n_empties <= 27 {
            Final(6)
        } else if n_empties <= 30 {
            Final(5)
        } else if n_empties <= 33 {
            Final(3)
        } else if n_empties <= 36 {
            Final(1)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level <= 31 {
        if n_empties <= 30 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 33 {
            Final(4)
        } else if n_empties <= 36 {
            Final(2)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level <= 33 {
        if n_empties <= 30 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 33 {
            Final(5)
        } else if n_empties <= 36 {
            Final(3)
        } else if n_empties <= 39 {
            Final(1)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level <= 35 {
        if n_empties <= 30 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 33 {
            Final(5)
        } else if n_empties <= 36 {
            Final(4)
        } else if n_empties <= 39 {
            Final(2)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else if level < 60 {
        if n_empties <= level - 6 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= level - 3 {
            Final(5)
        } else if n_empties <= level {
            Final(4)
        } else if n_empties <= level + 3 {
            Final(3)
        } else if n_empties <= level + 6 {
            Final(2)
        } else if n_empties <= level + 9 {
            Final(1)
        } else {
            Eval(level, EVAL_SOLVER_SELECTIVITY)
        }
    } else {
        Final(SELECTIVITY_LV_MAX)
    }
}

/// 中盤の iterative deepening。step=4 で `target_depth` まで反復し、各反復で
/// アスピレーション窓を使う。最終的な fail-soft スコアを返す。
fn iterative_deepening_eval(
    target_depth: i32,
    selectivity: i32,
    candidates: &mut [(u8, Board)],
    init_score: i32,
    search: &mut SearchContext,
) -> i32 {
    let step = 4;
    let start = target_depth.rem_euclid(step);
    let mut score = init_score;
    let mut depth = start;
    while depth <= target_depth {
        if search.is_aborted() {
            break;
        }
        let init_w = if depth > 16 { 2 } else { 6 };
        score = aspiration_search_eval(depth, selectivity, init_w, score, candidates, search);
        if search.is_aborted() {
            break;
        }
        depth += step;
    }
    score
}

/// 中盤探索のアスピレーション窓ループ。fail-high/low するたびに窓を片側拡張する。
fn aspiration_search_eval(
    depth: i32,
    selectivity: i32,
    init_width: i32,
    predict: i32,
    candidates: &mut [(u8, Board)],
    search: &mut SearchContext,
) -> i32 {
    search.selectivity_lv = selectivity;
    let mut left = init_width;
    let mut right = init_width;
    let mut predict = predict;
    let mut n = 0;
    let previous_best = candidates[0];
    let previous_score = predict;
    loop {
        n += 1;
        let alpha = (predict - left).max(-SCORE_MAX);
        let beta = (predict + right).min(SCORE_MAX);
        debug_assert!(alpha <= beta);
        predict = search_root_eval_window(depth, alpha, beta, candidates, search);
        if search.is_aborted() {
            restore_candidate_front(candidates, previous_best);
            return previous_score;
        }

        if (predict <= -SCORE_MAX && alpha <= -SCORE_MAX)
            || (predict >= SCORE_MAX && beta >= SCORE_MAX)
        {
            break;
        }
        if predict >= beta {
            widen(&mut right, &mut left, n);
        } else if predict <= alpha {
            widen(&mut left, &mut right, n);
        } else {
            break;
        }
    }
    predict
}

/// 終盤探索のアスピレーション窓ループ。
fn aspiration_search_final(
    selectivity: i32,
    init_width: i32,
    predict: i32,
    candidates: &mut [(u8, Board)],
    search: &mut SearchContext,
) -> i32 {
    search.selectivity_lv = selectivity;
    let mut left = init_width;
    let mut right = init_width;
    let mut predict = predict;
    let mut n = 0;
    let previous_best = candidates[0];
    let previous_score = predict;
    loop {
        n += 1;
        let alpha = (predict - left).max(-SCORE_MAX);
        let beta = (predict + right).min(SCORE_MAX);
        debug_assert!(alpha <= beta);
        predict = search_root_final_window(alpha, beta, candidates, search);
        if search.is_aborted() {
            restore_candidate_front(candidates, previous_best);
            return previous_score;
        }

        if (predict <= -SCORE_MAX && alpha <= -SCORE_MAX)
            || (predict >= SCORE_MAX && beta >= SCORE_MAX)
        {
            break;
        }
        if predict >= beta {
            widen(&mut right, &mut left, n);
        } else if predict <= alpha {
            widen(&mut left, &mut right, n);
        } else {
            break;
        }
    }
    predict
}

fn restore_candidate_front(candidates: &mut [(u8, Board)], previous_best: (u8, Board)) {
    if candidates[0].0 == previous_best.0 {
        return;
    }
    if let Some(idx) = candidates
        .iter()
        .position(|candidate| candidate.0 == previous_best.0)
    {
        candidates.swap(0, idx);
    }
}

/// アスピレーション窓を片側に広げる。fail 側を大きく、反対側を小さめに。
fn widen(primary: &mut i32, secondary: &mut i32, n: i32) {
    if n % 2 == 1 {
        *primary += 2;
    } else {
        *primary += n * (n as f64).log2() as i32 + 2;
        *secondary += 2;
    }
}

/// 中盤の root を [alpha, beta] 窓で 1 回探索し、最善手を `candidates[0]` に
/// スワップする。fail-soft スコアを返す。
fn search_root_eval_window(
    depth: i32,
    alpha: i32,
    beta: i32,
    candidates: &mut [(u8, Board)],
    search: &mut SearchContext,
) -> i32 {
    let mut alpha = alpha;
    if search.is_aborted() {
        return alpha;
    }
    let mut best_score = -pvs_eval(&candidates[0].1, -beta, -alpha, depth - 1, search);
    if search.is_aborted() {
        return alpha;
    }
    if best_score >= beta {
        return best_score;
    }
    if best_score > alpha {
        alpha = best_score;
    }
    let mut best_idx = 0;
    for i in 1..candidates.len() {
        let mut s = -nws_eval(&candidates[i].1, -alpha - 1, depth - 1, search);
        if search.is_aborted() {
            return best_score;
        }
        if s >= beta {
            candidates.swap(0, i);
            return s;
        }
        if s > alpha {
            s = -pvs_eval(&candidates[i].1, -beta, -alpha, depth - 1, search);
            if search.is_aborted() {
                return best_score;
            }
            if s >= beta {
                candidates.swap(0, i);
                return s;
            }
            if s > alpha {
                alpha = s;
                best_score = s;
                best_idx = i;
            }
        }
    }
    if best_idx > 0 {
        candidates.swap(0, best_idx);
    }
    best_score
}

fn search_root_final_siblings_ybwc(
    mut alpha: i32,
    beta: i32,
    mut best_score: i32,
    candidates: &mut [(u8, Board)],
    search: &mut SearchContext,
) -> i32 {
    let Some(thread_pool) = search.thread_pool.clone() else {
        return best_score;
    };
    let split_searching = Arc::new(AtomicBool::new(true));
    let mut handles = Vec::with_capacity(candidates.len() - 1);
    let mut results = Vec::new();

    for (move_index, (_, child_board)) in candidates.iter().enumerate().skip(1) {
        if !split_searching.load(Ordering::Relaxed) {
            break;
        }
        let job = make_ybwc_job(
            *child_board,
            -(alpha + 1),
            beta,
            move_index,
            split_searching.clone(),
            search,
        );
        match thread_pool.try_push(job) {
            Ok(handle) => {
                search.stats.ybwc_splits += 1;
                handles.push(handle);
            }
            Err(job) => {
                let result = job();
                search.stats.add_assign(result.stats);
                if result.aborted {
                    search.stats.ybwc_split_aborts += 1;
                }
                let cutoff = !result.aborted && result.score >= beta;
                results.push(result);
                if cutoff {
                    break;
                }
            }
        }
    }

    results.extend(collect_ybwc_tasks(handles, search));
    if search.check_abort_now() {
        split_searching.store(false, Ordering::Relaxed);
        return best_score;
    }

    if let Some(result) = results
        .iter()
        .find(|result| !result.aborted && result.score >= beta)
    {
        candidates.swap(0, result.move_index);
        return result.score;
    }

    results.sort_unstable_by_key(|result| result.move_index);
    let mut best_idx = 0;
    for result in results {
        if result.aborted || result.score <= alpha {
            continue;
        }
        let score = -pvs_final(&candidates[result.move_index].1, -beta, -alpha, search);
        if search.is_aborted() {
            return best_score;
        }
        if score >= beta {
            candidates.swap(0, result.move_index);
            return score;
        }
        if score > alpha {
            alpha = score;
            best_score = score;
            best_idx = result.move_index;
        }
    }
    if best_idx > 0 {
        candidates.swap(0, best_idx);
    }
    best_score
}

/// 終盤の root を [alpha, beta] 窓で 1 回探索し、最善手を `candidates[0]` に
/// スワップする。fail-soft スコアを返す。
fn search_root_final_window(
    alpha: i32,
    beta: i32,
    candidates: &mut [(u8, Board)],
    search: &mut SearchContext,
) -> i32 {
    let mut alpha = alpha;
    if search.is_aborted() {
        return alpha;
    }
    let trace = std::env::var_os("DEFT_SEARCH_TRACE").is_some();
    if trace {
        eprintln!(
            "SEARCHROOT_TT move={} value={:?}",
            candidates[0].0,
            search.tt.get(&candidates[0].1)
        );
    }
    let mut nodes_before = search.stats.eval_search_nodes + search.stats.final_search_nodes;
    let mut best_score = -pvs_final(&candidates[0].1, -beta, -alpha, search);
    if trace {
        let nodes = search.stats.eval_search_nodes + search.stats.final_search_nodes;
        eprintln!(
            "SEARCHROOT move={} score={best_score:+} nodes={}",
            candidates[0].0,
            nodes - nodes_before
        );
        nodes_before = nodes;
    }
    if search.is_aborted() {
        return alpha;
    }
    if best_score >= beta {
        return best_score;
    }
    if best_score > alpha {
        alpha = best_score;
    }
    let mut best_idx = 0;
    if search.selectivity_lv == SELECTIVITY_LV_MAX
        && search.thread_pool.is_some()
        && candidates.len() > 2
    {
        return search_root_final_siblings_ybwc(alpha, beta, best_score, candidates, search);
    }
    for i in 1..candidates.len() {
        let mut s = -nws_final(&candidates[i].1, -alpha - 1, search);
        if search.is_aborted() {
            return best_score;
        }
        if s >= beta {
            candidates.swap(0, i);
            return s;
        }
        if s > alpha {
            s = -pvs_final(&candidates[i].1, -beta, -alpha, search);
            if search.is_aborted() {
                return best_score;
            }
            if s >= beta {
                candidates.swap(0, i);
                return s;
            }
            if s > alpha {
                alpha = s;
                best_score = s;
                best_idx = i;
            }
        }
        if trace {
            let nodes = search.stats.eval_search_nodes + search.stats.final_search_nodes;
            eprintln!(
                "SEARCHROOT move={} score={s:+} nodes={}",
                candidates[i].0,
                nodes - nodes_before
            );
            nodes_before = nodes;
        }
    }
    if best_idx > 0 {
        candidates.swap(0, best_idx);
    }
    best_score
}

/// 中盤 PVS の root 探索。最善手と fail-soft スコアを返す。
// Root helper for the single-shot eval search entry point.
#[allow(dead_code)]
fn search_root_eval(board: &Board, depth: i32, search: &mut SearchContext) -> (u8, i32) {
    let moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            return (NO_COORD, solve_score(board));
        }
        let (_, score) = search_root_eval(&passed, depth, search);
        return (NO_COORD, -score);
    }

    let mut alpha = -SCORE_MAX;
    let beta = SCORE_MAX;
    let mut best_move = NO_COORD;
    let mut best_score = -SCORE_MAX;
    let mut is_first = true;

    let mut bits = moves_bit;
    while bits != 0 {
        let move_bit = bits & bits.wrapping_neg();
        bits &= bits - 1;
        let pos = move_bit.trailing_zeros() as u8;
        let child = board.make_move(move_bit);

        let score = if is_first {
            is_first = false;
            -pvs_eval(&child, -beta, -alpha, depth - 1, search)
        } else {
            let s = -nws_eval(&child, -(alpha + 1), depth - 1, search);
            if s > alpha && s < beta {
                -pvs_eval(&child, -beta, -alpha, depth - 1, search)
            } else {
                s
            }
        };

        if score > best_score {
            best_score = score;
            best_move = pos;
        }
        if score > alpha {
            alpha = score;
        }
    }

    (best_move, best_score)
}

/// 終盤 PVS の root 探索。最善手と完全読みスコアを返す。
// Root helper for the single-shot final search entry point.
#[allow(dead_code)]
fn search_root_final(board: &Board, search: &mut SearchContext) -> (u8, i32) {
    let moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            return (NO_COORD, solve_score(board));
        }
        let (_, score) = search_root_final(&passed, search);
        return (NO_COORD, -score);
    }

    let mut best_move = NO_COORD;
    let mut best_score = -SCORE_MAX;

    let mut bits = moves_bit;
    while bits != 0 {
        let move_bit = bits & bits.wrapping_neg();
        bits &= bits - 1;
        let pos = move_bit.trailing_zeros() as u8;
        let child = board.make_move(move_bit);
        let score = -pvs_final(&child, -SCORE_MAX, SCORE_MAX, search);
        if score > best_score {
            best_score = score;
            best_move = pos;
        }
    }

    (best_move, best_score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::search::NO_MPC_SELECTIVITY_LV;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    fn make_solver() -> Solver {
        Solver::new(Arc::new(Evaluator::default()))
    }

    #[test]
    fn solve_advances_tt_generation() {
        let solver = make_solver();
        let generation = solver.tt.generation();

        let result = solver.solve(&Board::new(), 1);

        assert!(!result.aborted);
        assert_eq!(solver.tt.generation(), generation.wrapping_add(1));
    }

    /// 初期盤面で深さ 4 の中盤探索が合法手を返すことを確認する。
    #[test]
    fn solve_eval_initial_board_returns_legal_move() {
        let solver = make_solver();
        let board = Board::new();
        let r = solver.solve_eval(&board, 4, NO_MPC_SELECTIVITY_LV);

        // 初期盤面の合法手は d3, c4, f5, e6 (= 19, 26, 37, 44)
        assert!(
            matches!(r.best_move, Some(19 | 26 | 37 | 44)),
            "unexpected best move: {:?}",
            r.best_move
        );
        assert!(r.nodes > 0);
    }

    /// 合法手なし → pass → 合法手なし の盤面は終局スコアを返す。
    #[test]
    fn solve_eval_terminal_position_returns_solve_score() {
        let solver = make_solver();
        let board = Board {
            player: u64::MAX,
            opponent: 0,
        };
        let r = solver.solve_eval(&board, 4, NO_MPC_SELECTIVITY_LV);

        assert_eq!(r.best_move, None);
        assert_eq!(r.score, 64); // 全マス自分の石
    }

    /// 終盤完全読み: 4〜6 マス空きから完全読みする。
    #[test]
    fn solve_final_matches_brute_force() {
        let solver = make_solver();
        let mut rng = 0xdead_beef_u64;
        for _ in 0..50 {
            rng ^= rng.wrapping_shl(13);
            rng ^= rng.wrapping_shr(7);
            rng ^= rng.wrapping_shl(17);
            let n_empties = 4 + (rng % 3) as u32;
            let mut empties = 0u64;
            while empties.count_ones() < n_empties {
                let pos = (rng % 64) as u32;
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                empties |= 1u64 << pos;
            }
            let player = rng & !empties;
            let board = Board {
                player,
                opponent: !player & !empties,
            };

            let r = solver.solve_final(&board, NO_MPC_SELECTIVITY_LV);
            let expected = brute_force(&board);
            assert_eq!(
                r.score, expected,
                "score mismatch on board p={:#018x} o={:#018x}",
                board.player, board.opponent
            );
        }
    }

    /// 高レベル solve(level) は初期盤面で合法手を返す。
    #[test]
    fn solve_initial_board_returns_legal_move() {
        let solver = make_solver();
        let board = Board::new();
        let r = solver.solve(&board, 4);
        assert!(
            matches!(r.best_move, Some(19 | 26 | 37 | 44)),
            "unexpected best move: {:?}",
            r.best_move
        );
    }

    #[test]
    fn solve_observes_stop_flag_during_search() {
        let stop = Arc::new(AtomicBool::new(false));
        let solver = Solver::with_options(
            Arc::new(Evaluator::default()),
            SolverOptions {
                tt_capacity: Some(16),
                stop: Some(stop.clone()),
                ..SolverOptions::default()
            },
        );
        let stopper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            stop.store(true, Ordering::Relaxed);
        });

        let board = Board::new();
        let r = solver.solve(&board, 21);
        stopper.join().unwrap();

        assert!(r.aborted, "solve finished without observing stop flag");
        assert!(
            r.best_move
                .is_some_and(|pos| board.moves() & (1u64 << pos) != 0),
            "best_move must be legal after abort: {:?}",
            r.best_move
        );
    }

    #[test]
    fn solve_restores_multi_move_pv_from_tt() {
        let solver = make_solver();
        let board = Board::new();
        let r = solver.solve(&board, 10);

        assert!(r.pv.len() >= 3, "pv too short: {:?}", r.pv);
        assert_eq!(r.pv.first().copied(), r.best_move);
        assert_pv_is_legal(&board, &r.pv);
    }

    /// レベル 60 の終盤完全読みが brute_force と一致する(残り 5〜7 マス)。
    #[test]
    fn solve_level60_endgame_matches_brute_force() {
        let solver = make_solver();
        let mut rng = 0x1234_5678_9abc_def0_u64;
        for _ in 0..20 {
            rng ^= rng.wrapping_shl(13);
            rng ^= rng.wrapping_shr(7);
            rng ^= rng.wrapping_shl(17);
            let n_empties = 5 + (rng % 3) as u32;
            let mut empties = 0u64;
            while empties.count_ones() < n_empties {
                let pos = (rng % 64) as u32;
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                empties |= 1u64 << pos;
            }
            let player = rng & !empties;
            let board = Board {
                player,
                opponent: !player & !empties,
            };
            if board.moves() == 0 && board.passed().moves() == 0 {
                continue;
            }

            let r = solver.solve(&board, 60);
            let expected = brute_force(&board);
            assert_eq!(
                r.score, expected,
                "score mismatch p={:#018x} o={:#018x}",
                board.player, board.opponent
            );
        }
    }

    #[test]
    fn parallel_final_search_matches_single_thread() {
        let single = Solver::with_options(
            Arc::new(Evaluator::default()),
            SolverOptions {
                tt_capacity: Some(16),
                ..SolverOptions::default()
            },
        );
        let parallel = Solver::with_options(
            Arc::new(Evaluator::default()),
            SolverOptions {
                tt_capacity: Some(16),
                search_threads: NonZeroUsize::new(8).unwrap(),
                ..SolverOptions::default()
            },
        );
        let mut rng = 0x7b31_d4a9_2f68_c05e_u64;
        let mut empties = 0u64;
        while empties.count_ones() < 14 {
            rng ^= rng.wrapping_shl(13);
            rng ^= rng.wrapping_shr(7);
            rng ^= rng.wrapping_shl(17);
            empties |= 1u64 << (rng % 64);
        }
        let board = Board {
            player: rng & !empties,
            opponent: !rng & !empties,
        };
        let expected = single.solve_final(&board, NO_MPC_SELECTIVITY_LV).score;

        // 並列実行順は非決定的なので、同じ局面を世代を分けて繰り返す。
        for _ in 0..4 {
            parallel.clear_tt();
            let actual = parallel.solve_final(&board, NO_MPC_SELECTIVITY_LV);
            assert!(!actual.aborted);
            assert_eq!(actual.score, expected);
        }
    }

    #[test]
    fn search_thread_count_includes_main_thread() {
        let single = Solver::with_options(
            Arc::new(Evaluator::default()),
            SolverOptions {
                search_threads: NonZeroUsize::MIN,
                ..SolverOptions::default()
            },
        );
        assert!(single.thread_pool.is_none());

        let parallel = Solver::with_options(
            Arc::new(Evaluator::default()),
            SolverOptions {
                search_threads: NonZeroUsize::new(2).unwrap(),
                ..SolverOptions::default()
            },
        );
        assert!(parallel.thread_pool.is_some());
    }

    fn brute_force(board: &Board) -> i32 {
        let moves = board.moves();
        if moves == 0 {
            if board.opponent_moves() == 0 {
                return solve_score(board);
            }
            return -brute_force(&board.passed());
        }
        let mut best = -SCORE_MAX;
        let mut bits = moves;
        while bits != 0 {
            let m = bits & bits.wrapping_neg();
            bits ^= m;
            best = best.max(-brute_force(&board.make_move(m)));
        }
        best
    }

    fn assert_pv_is_legal(board: &Board, pv: &[u8]) {
        let mut board = *board;
        for &mv in pv {
            while board.moves() == 0 {
                assert_ne!(board.opponent_moves(), 0, "PV continued after game end");
                board = board.passed();
            }
            let move_bit = 1u64 << mv;
            assert!(
                board.moves() & move_bit != 0,
                "illegal PV move {mv} on board p={:#018x} o={:#018x}",
                board.player,
                board.opponent
            );
            board = board.make_move(move_bit);
        }
    }
}
