//! 探索ドライバ。トップレベルの探索 API を提供する。
//!
//! ## 機能
//!
//! - [`Solver::solve_eval`]: 中盤（指定深さまで）の単発 PVS 探索
//! - [`Solver::solve_final`]: 終盤完全読み（終局まで）の単発 PVS 探索
//! - [`Solver::solve`]: レベル指定で反復深化 + アスピレーション窓を
//!   組む高レベル API。レベルに応じて中盤探索/終盤完全読み/MPC 強度を自動選択する。

use crate::board::board::Board;
use crate::board::constant::NO_COORD;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::eval::Evaluator;
use crate::file::EngineFile;
use crate::search::eval_search::{nws_eval, pvs_eval};
use crate::search::final_search::{nws_final, pvs_final, solve_score};
use crate::search::mpc::{MpcConfig, SELECTIVITY_LV_MAX};
use crate::search::search::{SearchContext, SearchStats};
use crate::t_table::TranspositionTable;
use crate::EngineError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 反復深化で使う MPC 選択率レベルの下限。レベル 2 は 85%。
const ITERATIVE_SELECTIVITY_MIN: i32 = 2;
/// この深さ（終盤では空きマス数）以下では、狭い窓で再探索せず最初から全幅探索する。
const ASPIRATION_FULL_WINDOW_DEPTH: i32 = 10;
/// 予測スコアが変化し続けた場合に、アスピレーション探索を安定化させる試行回数の上限。
const ASPIRATION_STABILIZE_RETRIES: i32 = 10;

/// solve() に指定できる最大レベル。
pub const SOLVE_LEVEL_MAX: i32 = 60;

/// solve() 内部で選択される具体的な探索構成。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverType {
    /// 中盤探索。第1要素が探索深さ、第2要素が MPC 選択率レベル。
    Eval(i32, i32),
    /// 終盤探索。要素は MPC 選択率レベルで、最大値なら完全読み。
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
    /// 置換表に割り当てる容量（MiB）。`None` の場合は既定値を使う。
    pub tt_capacity: Option<usize>,
    /// 呼び出し側から探索を中断するための共有フラグ。
    pub stop: Option<Arc<AtomicBool>>,
}

impl Default for SolverOptions {
    fn default() -> Self {
        Self {
            tt_capacity: None,
            stop: None,
        }
    }
}

/// `DEFT_PHASE_TIME` 用の段階別計時。
///
/// レベル指定の探索は次の順に進む。
///
/// 1. 中盤の反復深化 (`iterative_deepening_eval`)
/// 2. selectivity を上げながらの終盤探索
/// 3. 完全読み (exact)
struct PhaseTimer {
    /// 全段階に共通する計測開始時刻。
    start: Instant,
    /// 中盤反復深化が完了するまでの累積時間とノード数。
    iterative_deepening: Duration,
    iterative_deepening_nodes: u64,
    /// 中盤終了後から、最後の完全読み直前までの累積時間とノード数。
    selective_final: Duration,
    selective_final_nodes: u64,
}

impl PhaseTimer {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            iterative_deepening: Duration::ZERO,
            iterative_deepening_nodes: 0,
            selective_final: Duration::ZERO,
            selective_final_nodes: 0,
        }
    }

    fn searched_nodes(stats: &SearchStats) -> u64 {
        stats.eval_search_nodes + stats.final_search_nodes
    }

    /// 反復深化が終わった時点で呼ぶ。
    fn mark_iterative_deepening_done(&mut self, stats: &SearchStats) {
        self.iterative_deepening = self.start.elapsed();
        self.iterative_deepening_nodes = Self::searched_nodes(stats);
    }

    /// selectivity 付きの終盤探索が 1 段終わるたびに呼ぶ。
    /// 最後の exact 段では呼ばないため、累計は exact を含まない。
    fn mark_selective_final_done(&mut self, stats: &SearchStats) {
        self.selective_final = self.start.elapsed() - self.iterative_deepening;
        self.selective_final_nodes =
            Self::searched_nodes(stats) - self.iterative_deepening_nodes;
    }

    fn report(&self, stats: &SearchStats) -> String {
        let total = self.start.elapsed();
        let exact = total
            .saturating_sub(self.iterative_deepening)
            .saturating_sub(self.selective_final);
        let exact_nodes = Self::searched_nodes(stats)
            - self.iterative_deepening_nodes
            - self.selective_final_nodes;
        let share = |d: Duration| {
            if total.is_zero() {
                0.0
            } else {
                100.0 * d.as_secs_f64() / total.as_secs_f64()
            }
        };
        format!(
            "total={:.3}s | iterative_deepening={:.3}s ({:.1}%) nodes={} | selective_final={:.3}s ({:.1}%) nodes={} | exact_final={:.3}s ({:.1}%) nodes={}",
            total.as_secs_f64(),
            self.iterative_deepening.as_secs_f64(),
            share(self.iterative_deepening),
            self.iterative_deepening_nodes,
            self.selective_final.as_secs_f64(),
            share(self.selective_final),
            self.selective_final_nodes,
            exact.as_secs_f64(),
            share(exact),
            exact_nodes,
        )
    }
}

/// 探索ドライバ。`evaluator` / `mpc` / `tt` を Arc で保持し、複数回の探索で
/// TT を再利用できる。
pub struct Solver {
    /// 探索値の計算に使う本評価器。
    evaluator: Arc<Evaluator>,
    /// 候補手の並べ替えに使う評価器。本評価器とは別に差し替えられる。
    ordering_evaluator: Arc<Evaluator>,
    /// MPC の深さ・誤差分布・選択率設定。
    mpc: Arc<MpcConfig>,
    /// 通常探索用と PV 復元優先用の置換表。
    tt: Arc<TranspositionTable>,
    pv_tt: Arc<TranspositionTable>,
    /// 外部中断フラグ。`None` なら時間制限なしで探索する。
    stop: Option<Arc<AtomicBool>>,
}

impl Solver {
    /// 既定の MPC、置換表容量、1 スレッド構成で探索器を生成する。
    pub fn new(evaluator: Arc<Evaluator>) -> Self {
        Self::with_options(evaluator, SolverOptions::default())
    }

    /// 評価器と実行時オプションを指定して探索器を生成する。
    pub fn with_options(evaluator: Arc<Evaluator>, opts: SolverOptions) -> Self {
        Self::with_mpc(evaluator, Arc::new(MpcConfig::default()), opts)
    }

    fn with_mpc(evaluator: Arc<Evaluator>, mpc: Arc<MpcConfig>, opts: SolverOptions) -> Self {
        let tt = match opts.tt_capacity {
            Some(mb) => TranspositionTable::with_mb_size(mb),
            None => TranspositionTable::new(),
        };
        // PV 復元専用の表は通常 TT より小さくてよいが、最低 1 MiB は確保する。
        let pv_tt = TranspositionTable::with_mb_size((tt.actual_mb_size() / 16).max(1));
        Self {
            ordering_evaluator: evaluator.clone(),
            evaluator,
            mpc,
            tt: Arc::new(tt),
            pv_tt: Arc::new(pv_tt),
            stop: opts.stop,
        }
    }

    /// ファイルから評価器と MPC 設定を読み込む。
    /// UTF-8 テキストとして解釈できなければバイナリ形式として読み直す。
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

    /// 文字列を現行のエンジン形式、失敗した場合は旧評価器形式として読み込む。
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

    /// 現在使用中の MPC 設定を返す。
    pub fn mpc_config(&self) -> &MpcConfig {
        &self.mpc
    }

    /// 着手順序付け専用の評価器を差し替える。
    pub fn set_ordering_evaluator(&mut self, ev: Evaluator) {
        self.ordering_evaluator = Arc::new(ev);
    }

    /// 置換表の世代を進め、過去のエントリを優先的な置換対象にする。
    pub fn clear_tt(&self) {
        self.tt.advance_generation();
        self.pv_tt.advance_generation();
    }

    /// 中盤の PVS 探索を `depth` まで実行する。
    ///
    // 内部テストやデバッグから反復深化を介さず呼ぶための単発探索 API。
    #[allow(dead_code)]
    pub fn solve_eval(&self, board: &Board, depth: i32, selectivity_lv: i32) -> SolverResult {
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            self.evaluator.clone(),
            self.mpc.clone(),
            self.tt.clone(),
            &mut stats,
        )
        .with_pv_tt(self.pv_tt.clone(), board.empties_count() as i32)
        .with_ordering_evaluator(self.ordering_evaluator.clone())
        .with_stop(self.stop.clone());
        search.selectivity_lv = selectivity_lv;

        let solver_type = SolverType::Eval(depth, selectivity_lv);
        let (best_move, score) = search_root_eval(board, depth, &mut search);
        let aborted = search.is_aborted();
        drop(search);
        self.result_from_parts(best_move, score, solver_type, &stats, aborted, board)
    }

    /// 終盤完全読み（終局まで）を実行する。
    // 内部テストやデバッグから反復深化を介さず呼ぶための単発探索 API。
    #[allow(dead_code)]
    pub fn solve_final(&self, board: &Board, selectivity_lv: i32) -> SolverResult {
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            self.evaluator.clone(),
            self.mpc.clone(),
            self.tt.clone(),
            &mut stats,
        )
        .with_pv_tt(self.pv_tt.clone(), board.empties_count() as i32)
        .with_ordering_evaluator(self.ordering_evaluator.clone())
        .with_stop(self.stop.clone());
        search.selectivity_lv = selectivity_lv;

        let solver_type = SolverType::Final(selectivity_lv);
        let (best_move, score) = search_root_final(board, &mut search);
        let aborted = search.is_aborted();
        drop(search);
        self.result_from_parts(best_move, score, solver_type, &stats, aborted, board)
    }

    /// レベル指定の高レベル探索。`level` (1..=60) に応じて中盤/終盤の構成を
    /// 自動選択し、2 手刻みの反復深化 + アスピレーション窓で TT を
    /// 暖めながら最終探索を行う。
    pub fn solve(&self, board: &Board, level: i32) -> SolverResult {
        let level = level.clamp(1, SOLVE_LEVEL_MAX);

        let legal = board.moves();
        let n_empties = (board.player | board.opponent).count_zeros() as i32;
        let mut solver_type = level_to_solver_type(n_empties, level);
        // 開始前から停止済みでも、呼び出し側が着手できるよう静的評価で合法手を返す。
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
        // 合法手がなければパスする。相手にも合法手がなければ終局である。
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

        // CLI は同じ Solver を複数局面で再利用する。探索開始時に世代を進め、
        // 以前の局面のエントリを現在の探索結果より置換されやすくする。
        self.tt.advance_generation();
        self.pv_tt.advance_generation();

        // (着手座標, 着手後の子局面) の一覧。candidates[0] を現在の最善手とし、
        // 反復のたびに先頭を入れ替えて次の探索順へ引き継ぐ。
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
        .with_pv_tt(self.pv_tt.clone(), n_empties)
        .with_ordering_evaluator(self.ordering_evaluator.clone())
        .with_stop(self.stop.clone());
        // 開幕の評価は粗いので最大 MPC を使うが、深いレベルでは MPC を弱める。
        search.selectivity_lv = if level > 10 {
            ITERATIVE_SELECTIVITY_MIN
        } else {
            SELECTIVITY_LV_MAX
        };

        let mut predict_score = self.evaluator.evaluate_board_slow(board);

        let mut phase = PhaseTimer::new();

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
                phase.mark_iterative_deepening_done(search.stats);
            }
            SolverType::Final(selectivity) => {
                let selectivity = *selectivity;
                // Edax同様、完全読みの約8手前まで中盤反復深化を行う。
                let eval_solver_lv = (n_empties - 8).clamp(2, 24).min(level);
                predict_score = iterative_deepening_eval(
                    eval_solver_lv,
                    ITERATIVE_SELECTIVITY_MIN,
                    &mut candidates,
                    predict_score,
                    &mut search,
                );
                phase.mark_iterative_deepening_done(search.stats);
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

                // 安価な選択的探索から目標の選択率へ段階的に近づける。前段階の
                // 予測スコアと root の着手順は、次段階の初期値として再利用する。
                let mut final_selectivity = first_final_selectivity(selectivity);
                loop {
                    let init_w = (10 - n_empties).max(2 + predict_score.rem_euclid(2));
                    predict_score = aspiration_search_final(
                        n_empties,
                        final_selectivity,
                        init_w,
                        predict_score,
                        &mut candidates,
                        &mut search,
                    );
                    trace_search_stage(
                        if final_selectivity == selectivity {
                            "exact-final"
                        } else {
                            "selective-final"
                        },
                        predict_score,
                        &search,
                    );
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
                    if final_selectivity == selectivity {
                        break;
                    }
                    phase.mark_selective_final_done(search.stats);
                    final_selectivity =
                        next_final_selectivity(final_selectivity, selectivity, n_empties);
                }
            }
        }

        if std::env::var_os("DEFT_PHASE_TIME").is_some() {
            eprintln!("PHASETIME {}", phase.report(search.stats));
        }
        let aborted = search.is_aborted();
        // SearchContext が stats を可変借用しているため、結果構築前に借用を解放する。
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

    /// 停止済みの場合に返す手を、1 手先の静的評価だけで選ぶ。
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

    /// 探索内部の値を公開用の結果へ変換し、可能なら PV も復元する。
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
        if crate::t_table::tt_contention_stats_enabled() {
            eprintln!(
                "TTSTATS {}",
                crate::t_table::take_tt_contention_stats().summary_line()
            );
        }
        let best_move_opt = (best_move != NO_COORD).then_some(best_move);
        // 中断直後などで root 手が不正なら、壊れた PV を返さず空にする。
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

    /// root の最善手を起点に、PV 用 TT（なければ通常 TT）をたどる。
    /// パスは着手列に含めず、TT の手が不正または終局なら復元を打ち切る。
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
                next_move = self.pv_move(&current);
                continue;
            }

            let mv = match next_move.or_else(|| self.pv_move(&current)) {
                Some(mv) if mv != NO_COORD && (moves & (1u64 << mv)) != 0 => mv,
                _ => break,
            };
            pv.push(mv);
            current = current.make_move(1u64 << mv);
            next_move = self.pv_move(&current);
        }

        pv
    }

    fn pv_move(&self, board: &Board) -> Option<u8> {
        self.pv_tt
            .get(board)
            .or_else(|| self.tt.get(board))
            .map(|value| value.move0)
    }
}

/// 環境変数で有効化されたときだけ、探索段階ごとの統計を標準エラーへ出す。
fn trace_search_stage(stage: &str, score: i32, search: &SearchContext) {
    if std::env::var_os("DEFT_SEARCH_TRACE").is_some() {
        eprintln!(
            "SEARCHTRACE stage={stage} score={score:+} eval_nodes={} final_nodes={} \
             mpc={}/{} stability={}/{}",
            search.stats.eval_search_nodes,
            search.stats.final_search_nodes,
            search.stats.mpc_cuts,
            search.stats.mpc_tries,
            search.stats.stability_cuts,
            search.stats.stability_tries,
        );
    }
}

/// レベルと残り空きマスから具体的な探索構成を決める(旧 solver の get_config 移植)。
fn level_to_solver_type(n_empties: i32, level: i32) -> SolverType {
    // レベルが上がるほど、より空きマスの多い局面から終盤探索へ移る。
    // Final の引数が小さい段階は、MPC を強く使う選択的な終盤探索である。
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
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
        }
    } else if level <= 18 {
        if n_empties <= 21 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 24 {
            Final(4)
        } else if n_empties <= 27 {
            Final(2)
        } else {
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
        }
    } else if level <= 21 {
        if n_empties <= 24 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 27 {
            Final(4)
        } else if n_empties <= 30 {
            Final(2)
        } else {
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
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
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
        }
    } else if level <= 27 {
        if n_empties <= 27 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 30 {
            Final(4)
        } else if n_empties <= 33 {
            Final(2)
        } else {
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
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
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
        }
    } else if level <= 31 {
        if n_empties <= 30 {
            Final(SELECTIVITY_LV_MAX)
        } else if n_empties <= 33 {
            Final(4)
        } else if n_empties <= 36 {
            Final(2)
        } else {
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
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
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
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
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
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
            Eval(level, ITERATIVE_SELECTIVITY_MIN)
        }
    } else {
        Final(SELECTIVITY_LV_MAX)
    }
}

/// 中盤の iterative deepening。Edax同様step=2で`target_depth`まで反復し、各反復で
/// アスピレーション窓を使う。最終的な fail-soft スコアを返す。
fn iterative_deepening_eval(
    target_depth: i32,
    selectivity: i32,
    candidates: &mut [(u8, Board)],
    init_score: i32,
    search: &mut SearchContext,
) -> i32 {
    if target_depth <= 1 {
        return aspiration_search_eval(
            target_depth,
            selectivity,
            6,
            init_score,
            candidates,
            search,
        );
    }

    // 目標深さと同じ偶奇を保ち、直前の探索結果を予測値として使いながら深くする。
    let step = 2;
    let start = first_iterative_depth(target_depth);
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

#[inline(always)]
fn first_iterative_depth(target_depth: i32) -> i32 {
    debug_assert!(target_depth >= 2);
    // 可能なら深さ 5/6 から開始し、目標深さと同じ偶奇にそろえる。
    let mut start = 6 - (target_depth & 1);
    if start > target_depth - 2 {
        start = target_depth - 2;
    }
    if start <= 0 {
        start = 2 - (target_depth & 1);
    }
    start
}

#[inline(always)]
fn first_final_selectivity(target: i32) -> i32 {
    // 完全読みが目標でも、最初は安価な選択的探索で着手順を整える。
    target.min(ITERATIVE_SELECTIVITY_MIN)
}

/// Edaxの完全読み反復と同じ空き数閾値で、高価なselectivity段階を省略する。
fn next_final_selectivity(current: i32, target: i32, n_empties: i32) -> i32 {
    debug_assert!(current < target);
    let next = current + 1;
    let jump_to_target = (n_empties < 21 && next >= 1)
        || (n_empties < 24 && next >= 2)
        || (n_empties < 27 && next >= 3)
        || (n_empties < 30 && next >= 4);
    if jump_to_target {
        target
    } else {
        next.min(target)
    }
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
    let previous_best = candidates[0];
    let previous_score = predict;

    if depth <= ASPIRATION_FULL_WINDOW_DEPTH {
        // 浅い探索では窓を広げ直す費用が利点を上回るため、最初から全幅で読む。
        let score = search_root_eval_window(depth, -SCORE_MAX, SCORE_MAX, candidates, search);
        if search.is_aborted() {
            restore_candidate_front(candidates, previous_best);
            return previous_score;
        }
        return score;
    }

    let base_width = init_width.max(1);
    let mut score = predict;
    // 窓内に収まってもスコアが予測値から動いた場合は、その値を中心に再度安定させる。
    for retry in 0..ASPIRATION_STABILIZE_RETRIES {
        let old_score = score;
        let width = retry.max(1) * base_width;
        let mut left = width;
        let mut right = width;

        loop {
            let alpha = (score - left).max(-SCORE_MAX);
            let beta = (score + right).min(SCORE_MAX);
            if alpha >= beta {
                break;
            }
            score = search_root_eval_window(depth, alpha, beta, candidates, search);
            if search.is_aborted() {
                // 未完了探索のスコアと着手順を、確定結果として上位へ返さない。
                restore_candidate_front(candidates, previous_best);
                return previous_score;
            }

            if (score <= -SCORE_MAX && alpha <= -SCORE_MAX)
                || (score >= SCORE_MAX && beta >= SCORE_MAX)
            {
                break;
            }
            if score <= alpha && grow_aspiration_side(&mut left, &mut right) {
                continue;
            }
            if score >= beta && grow_aspiration_side(&mut right, &mut left) {
                continue;
            }
            break;
        }

        if score == old_score {
            break;
        }
    }
    score
}

/// 終盤探索のアスピレーション窓ループ。
fn aspiration_search_final(
    n_empties: i32,
    selectivity: i32,
    init_width: i32,
    predict: i32,
    candidates: &mut [(u8, Board)],
    search: &mut SearchContext,
) -> i32 {
    search.selectivity_lv = selectivity;
    let mut root_bounds = [RootBound::UNBOUNDED; 64];
    let previous_best = candidates[0];
    let previous_score = predict;

    if n_empties <= ASPIRATION_FULL_WINDOW_DEPTH {
        // 残り手数が少なければ、狭い窓の再探索より全幅探索の方が安い。
        let score =
            search_root_final_window(-SCORE_MAX, SCORE_MAX, candidates, &mut root_bounds, search);
        if search.is_aborted() {
            restore_candidate_front(candidates, previous_best);
            return previous_score;
        }
        return score;
    }

    let base_width = init_width.max(1);
    let mut score = predict;
    for retry in 0..ASPIRATION_STABILIZE_RETRIES {
        let old_score = score;
        let width = retry.max(1) * base_width;
        let mut left = width;
        let mut right = width;

        loop {
            // 終局スコアは偶数なので、窓端を外側の偶数へ丸める。
            // 発生しない奇数スコアだけを区別する再探索を避けられる。
            let alpha = even_floor((score - left).max(-SCORE_MAX));
            let beta = even_ceil((score + right).min(SCORE_MAX));
            if alpha >= beta {
                break;
            }
            score = search_root_final_window(alpha, beta, candidates, &mut root_bounds, search);
            if search.is_aborted() {
                restore_candidate_front(candidates, previous_best);
                return previous_score;
            }

            if (score <= -SCORE_MAX && alpha <= -SCORE_MAX)
                || (score >= SCORE_MAX && beta >= SCORE_MAX)
            {
                break;
            }
            if score <= alpha && grow_aspiration_side(&mut left, &mut right) {
                continue;
            }
            if score >= beta && grow_aspiration_side(&mut right, &mut left) {
                continue;
            }
            break;
        }

        if score == old_score {
            break;
        }
    }
    score
}

/// root の各着手について、完全読みで判明したスコアの上下限を保持する。
/// アスピレーション窓を広げた際、既知の範囲で探索を省略・縮小するために使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RootBound {
    /// この着手のスコアが少なくともこの値以上、という既知の下限。
    lower: i32,
    /// この着手のスコアがこの値以下、という既知の上限。
    upper: i32,
}

impl RootBound {
    const UNBOUNDED: Self = Self {
        lower: -SCORE_MAX,
        upper: SCORE_MAX,
    };

    #[inline(always)]
    fn exact(self) -> Option<i32> {
        (self.lower == self.upper).then_some(self.lower)
    }

    #[inline(always)]
    fn update(&mut self, score: i32, alpha: i32, beta: i32) -> i32 {
        // fail-low は上限、fail-high は下限、窓内の値は確定値として記録する。
        if score <= alpha {
            self.upper = self.upper.min(score);
        } else if score >= beta {
            self.lower = self.lower.max(score);
        } else {
            self.lower = score;
            self.upper = score;
        }
        debug_assert!(self.lower <= self.upper);
        self.exact().unwrap_or(score)
    }
}

#[inline(always)]
fn search_root_final_candidate(
    board: &Board,
    alpha: i32,
    beta: i32,
    bound: &mut RootBound,
    search: &mut SearchContext,
) -> i32 {
    let exact_selectivity = search.selectivity_lv == SELECTIVITY_LV_MAX;
    if exact_selectivity {
        // 選択的探索の値は厳密な上下限ではないため、完全読み時だけ再利用する。
        if let Some(score) = bound.exact() {
            return score;
        }
        if bound.lower >= beta {
            return bound.lower;
        }
        if bound.upper <= alpha {
            return bound.upper;
        }
    }

    let search_alpha = if exact_selectivity {
        alpha.max(bound.lower)
    } else {
        alpha
    };
    let search_beta = if exact_selectivity {
        beta.min(bound.upper)
    } else {
        beta
    };
    debug_assert!(search_alpha < search_beta);
    let score = -pvs_final(board, -search_beta, -search_alpha, search);
    if search.is_aborted() {
        return alpha;
    }
    if exact_selectivity {
        bound.update(score, search_alpha, search_beta)
    } else {
        score
    }
}

/// 中断前に確定していた最善手を候補配列の先頭へ戻す。
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

/// Edax同様、failした側を倍増し、反対側を現在のscoreで閉じる。
fn grow_aspiration_side(failed_side: &mut i32, opposite_side: &mut i32) -> bool {
    if *failed_side == 0 {
        return false;
    }
    *failed_side = failed_side.saturating_mul(2).min(2 * SCORE_MAX);
    *opposite_side = 0;
    true
}

#[inline(always)]
fn even_floor(value: i32) -> i32 {
    value - value.rem_euclid(2)
}

#[inline(always)]
fn even_ceil(value: i32) -> i32 {
    value + value.rem_euclid(2)
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
    // 前回の最善手は現在の [alpha, beta] 窓全体で読み、alpha を先に引き上げる。
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
        // 2 手目以降は null-window で候補を絞り、alpha を超えた手だけ読み直す。
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

/// 終盤の root を [alpha, beta] 窓で 1 回探索し、最善手を `candidates[0]` に
/// スワップする。fail-soft スコアを返す。
fn search_root_final_window(
    alpha: i32,
    beta: i32,
    candidates: &mut [(u8, Board)],
    root_bounds: &mut [RootBound; 64],
    search: &mut SearchContext,
) -> i32 {
    let mut alpha = alpha;
    let exact_selectivity = search.selectivity_lv == SELECTIVITY_LV_MAX;
    if search.is_aborted() {
        return alpha;
    }
    let trace = std::env::var_os("DEFT_SEARCH_TRACE").is_some();
    if trace {
        eprintln!(
            "SEARCHROOT_TT move={} value={:?}",
            candidates[0].0,
            search
                .pv_tt
                .as_ref()
                .and_then(|pv_tt| pv_tt.get(&candidates[0].1))
                .or_else(|| search.tt.get(&candidates[0].1))
        );
    }
    let mut nodes_before = search.stats.eval_search_nodes + search.stats.final_search_nodes;
    let first_move = candidates[0].0 as usize;
    // 前段階で先頭になった手を最初に読み、残りの探索に使う alpha を確定させる。
    let mut best_score = search_root_final_candidate(
        &candidates[0].1,
        alpha,
        beta,
        &mut root_bounds[first_move],
        search,
    );
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
    for i in 1..candidates.len() {
        let move_num = candidates[i].0 as usize;
        // 完全読みでは、過去の窓探索で得た上下限だけでカットできる場合がある。
        if exact_selectivity && root_bounds[move_num].lower >= beta {
            candidates.swap(0, i);
            return root_bounds[move_num].lower;
        }
        if exact_selectivity && root_bounds[move_num].upper <= alpha {
            if root_bounds[move_num].upper > best_score {
                best_score = root_bounds[move_num].upper;
                best_idx = i;
            }
            continue;
        }
        let mut s = -nws_final(&candidates[i].1, -alpha - 1, search);
        if search.is_aborted() {
            return best_score;
        }
        if exact_selectivity {
            s = root_bounds[move_num].update(s, alpha, alpha + 1);
        }
        if s >= beta {
            candidates.swap(0, i);
            return s;
        }
        if s > alpha {
            s = search_root_final_candidate(
                &candidates[i].1,
                alpha,
                beta,
                &mut root_bounds[move_num],
                search,
            );
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
// 単発の中盤探索 API から使う root 用ヘルパー。
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
// 単発の終盤探索 API から使う root 用ヘルパー。
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
        let pv_generation = solver.pv_tt.generation();

        let result = solver.solve(&Board::new(), 1);

        assert!(!result.aborted);
        assert_eq!(solver.tt.generation(), generation.wrapping_add(1));
        assert_eq!(solver.pv_tt.generation(), pv_generation.wrapping_add(1));
    }

    #[test]
    fn root_bound_combines_opposite_bounds_into_exact_score() {
        let mut bound = RootBound::UNBOUNDED;
        assert_eq!(bound.update(-8, -7, -3), -8);
        assert_eq!(
            bound,
            RootBound {
                lower: -64,
                upper: -8
            }
        );

        assert_eq!(bound.update(-8, -12, -8), -8);
        assert_eq!(bound.exact(), Some(-8));
    }

    #[test]
    fn non_exact_root_candidate_ignores_and_preserves_bound() {
        let board = Board {
            player: u64::MAX,
            opponent: 0,
        };
        let mut bound = RootBound {
            lower: 17,
            upper: 17,
        };
        let original_bound = bound;
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            Arc::new(Evaluator::default()),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
            &mut stats,
        );
        search.selectivity_lv = ITERATIVE_SELECTIVITY_MIN;

        let score =
            search_root_final_candidate(&board, -SCORE_MAX, SCORE_MAX, &mut bound, &mut search);

        assert_eq!(score, -SCORE_MAX);
        assert_eq!(bound, original_bound);
    }

    #[test]
    fn aspiration_growth_doubles_failed_side_and_closes_opposite_side() {
        let mut failed = 2;
        let mut opposite = 2;

        assert!(grow_aspiration_side(&mut failed, &mut opposite));
        assert_eq!((failed, opposite), (4, 0));
        assert!(!grow_aspiration_side(&mut opposite, &mut failed));
    }

    #[test]
    fn exact_aspiration_bounds_expand_outward_to_even_scores() {
        assert_eq!(even_floor(3), 2);
        assert_eq!(even_ceil(3), 4);
        assert_eq!(even_floor(-3), -4);
        assert_eq!(even_ceil(-3), -2);
        assert_eq!(even_floor(8), 8);
        assert_eq!(even_ceil(8), 8);
    }

    #[test]
    fn iterative_deepening_starts_with_edax_parity_and_advances_by_two() {
        assert_eq!(first_iterative_depth(2), 2);
        assert_eq!(first_iterative_depth(3), 1);
        assert_eq!(first_iterative_depth(8), 6);
        assert_eq!(first_iterative_depth(9), 5);
        assert_eq!(first_iterative_depth(20), 6);
    }

    #[test]
    fn final_selectivity_uses_edax_endgame_jump_thresholds() {
        assert_eq!(first_final_selectivity(6), 2);
        assert_eq!(first_final_selectivity(2), 2);
        assert_eq!(first_final_selectivity(1), 1);
        assert_eq!(next_final_selectivity(2, 6, 20), 6);
        assert_eq!(next_final_selectivity(2, 6, 23), 6);
        assert_eq!(next_final_selectivity(2, 6, 26), 6);
        assert_eq!(next_final_selectivity(2, 6, 29), 3);
        assert_eq!(next_final_selectivity(3, 6, 29), 6);
        assert_eq!(next_final_selectivity(4, 6, 30), 5);
        assert_eq!(next_final_selectivity(5, 6, 30), 6);
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
