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
use crate::search::eval_search::{nws_eval, pvs_eval};
use crate::search::final_search::{nws_final, pvs_final, solve_score};
use crate::search::mpc::{MpcConfig, SELECTIVITY_LV_MAX};
use crate::search::search::{SearchContext, SearchStats};
use crate::t_table::TranspositionTable;
use std::sync::Arc;

/// solve() 内で使う中盤探索の MPC selectivity。
const EVAL_SOLVER_SELECTIVITY: i32 = 1;

/// solve() に指定できる最大レベル。
pub const SOLVE_LEVEL_MAX: i32 = 60;

/// solve() 内部で選択される具体的な探索構成。
#[derive(Debug, Clone, Copy)]
pub enum SolverType {
    /// 中盤探索: (depth, selectivity_lv)
    Eval(i32, i32),
    /// 終盤完全読み: (selectivity_lv)
    Final(i32),
}

/// 単発探索の結果。
#[derive(Debug, Clone, Copy)]
pub struct SolverResult {
    /// 最善手の盤面座標(0-63)。合法手が無いときは [`NO_COORD`]。
    pub best_move: u8,
    /// 現プレイヤー視点のスコア。
    pub score: i32,
    /// 探索したノード数。
    pub nodes: u64,
}

/// 探索ドライバ。`evaluator` / `mpc` / `tt` を Arc で保持し、複数回の探索で
/// TT を再利用できる。
pub struct Solver {
    pub evaluator: Arc<Evaluator>,
    pub mpc: Arc<MpcConfig>,
    pub tt: Arc<TranspositionTable>,
}

impl Solver {
    pub fn new(
        evaluator: Arc<Evaluator>,
        mpc: Arc<MpcConfig>,
        tt: Arc<TranspositionTable>,
    ) -> Self {
        Self { evaluator, mpc, tt }
    }

    /// 中盤の PVS 探索を `depth` まで実行する。
    ///
    /// `final_search_empties` 以下の空きマスになると `pvs_final` に切り替わる。
    pub fn solve_eval(
        &self,
        board: &Board,
        depth: i32,
        selectivity_lv: i32,
        final_search_empties: i32,
    ) -> SolverResult {
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            self.evaluator.clone(), self.mpc.clone(), self.tt.clone(), &mut stats,
        );
        search.selectivity_lv = selectivity_lv;
        search.final_search_empties = final_search_empties;

        let (best_move, score) = search_root_eval(board, depth, &mut search);
        SolverResult { best_move, score, nodes: stats.eval_search_leaf_nodes + stats.final_search_nodes }
    }

    /// 終盤完全読み(game end まで)を実行する。
    pub fn solve_final(&self, board: &Board, selectivity_lv: i32) -> SolverResult {
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            self.evaluator.clone(), self.mpc.clone(), self.tt.clone(), &mut stats,
        );
        search.selectivity_lv = selectivity_lv;

        let (best_move, score) = search_root_final(board, &mut search);
        SolverResult { best_move, score, nodes:  stats.eval_search_leaf_nodes + stats.final_search_nodes }
    }

    /// レベル指定の高レベル探索。`level` (1..=60) に応じて中盤/終盤の構成を
    /// 自動選択し、step=4 の iterative deepening + アスピレーション窓で TT を
    /// 暖めながら最終探索を行う。
    pub fn solve(&self, board: &Board, level: i32) -> SolverResult {
        let level = level.clamp(1, SOLVE_LEVEL_MAX);

        let legal = board.moves();
        if legal == 0 {
            let passed = board.passed();
            if passed.moves() == 0 {
                return SolverResult { best_move: NO_COORD, score: solve_score(board), nodes: 0 };
            }
            let mut r = self.solve(&passed, level);
            r.score = -r.score;
            r.best_move = NO_COORD;
            return r;
        }

        // root 候補手リスト(再順序の効率化のため Vec で保持)。
        let mut candidates: Vec<(u8, Board)> = Vec::with_capacity(legal.count_ones() as usize);
        let mut bits = legal;
        while bits != 0 {
            let mb = bits & bits.wrapping_neg();
            bits &= bits - 1;
            let pos = mb.trailing_zeros() as u8;
            candidates.push((pos, board.make_move(mb)));
        }

        let n_empties = (board.player | board.opponent).count_zeros() as i32;
        let mut solver_type = level_to_solver_type(n_empties, level);

        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            self.evaluator.clone(), self.mpc.clone(), self.tt.clone(), &mut stats,
        );
        // 開幕の評価は粗いので最大 MPC を使うが、深いレベルでは MPC を弱める。
        search.selectivity_lv = if level > 10 { EVAL_SOLVER_SELECTIVITY } else { SELECTIVITY_LV_MAX };

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
                    *target_depth, *selectivity, &mut candidates, predict_score, &mut search,
                );
            }
            SolverType::Final(selectivity) => {
                let selectivity = *selectivity;
                // Final solver 前の中盤反復深化レベル(短く)。
                let eval_solver_lv = (n_empties - 7 - (2 - selectivity / 2))
                    .clamp(2, 24)
                    .min(level);
                predict_score = iterative_deepening_eval(
                    eval_solver_lv, EVAL_SOLVER_SELECTIVITY, &mut candidates, predict_score, &mut search,
                );

                if eval_solver_lv >= 18 && selectivity > 5 {
                    let init_w = (10 - n_empties).max(2 + predict_score.rem_euclid(2));
                    predict_score = aspiration_search_final(
                        selectivity - 4, init_w, predict_score, &mut candidates, &mut search,
                    );
                }
                let init_w = (10 - n_empties).max(2 + predict_score.rem_euclid(2));
                predict_score = aspiration_search_final(
                    selectivity, init_w, predict_score, &mut candidates, &mut search,
                );
            }
        }

        SolverResult {
            best_move: candidates[0].0,
            score: predict_score,
            nodes:  stats.eval_search_leaf_nodes + stats.final_search_nodes,
        }
    }
}

/// レベルと残り空きマスから具体的な探索構成を決める(旧 solver の get_config 移植)。
fn level_to_solver_type(n_empties: i32, level: i32) -> SolverType {
    use SolverType::*;
    if level == 0 {
        Eval(0, SELECTIVITY_LV_MAX)
    } else if level <= 10 {
        if n_empties <= 2 * level { Final(SELECTIVITY_LV_MAX) } else { Eval(level, SELECTIVITY_LV_MAX) }
    } else if level <= 12 {
        if n_empties <= 21 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= 24 { Final(4) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level <= 18 {
        if n_empties <= 21 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= 24 { Final(4) }
        else if n_empties <= 27 { Final(2) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level <= 21 {
        if n_empties <= 24 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= 27 { Final(4) }
        else if n_empties <= 30 { Final(2) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level <= 24 {
        if n_empties <= 24 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= 27 { Final(5) }
        else if n_empties <= 30 { Final(3) }
        else if n_empties <= 33 { Final(1) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level <= 27 {
        if n_empties <= 27 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= 30 { Final(4) }
        else if n_empties <= 33 { Final(2) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level < 30 {
        if n_empties <= 27 { Final(6) }
        else if n_empties <= 30 { Final(5) }
        else if n_empties <= 33 { Final(3) }
        else if n_empties <= 36 { Final(1) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level <= 31 {
        if n_empties <= 30 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= 33 { Final(4) }
        else if n_empties <= 36 { Final(2) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level <= 33 {
        if n_empties <= 30 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= 33 { Final(5) }
        else if n_empties <= 36 { Final(3) }
        else if n_empties <= 39 { Final(1) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level <= 35 {
        if n_empties <= 30 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= 33 { Final(5) }
        else if n_empties <= 36 { Final(4) }
        else if n_empties <= 39 { Final(2) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
    } else if level < 60 {
        if n_empties <= level - 6 { Final(SELECTIVITY_LV_MAX) }
        else if n_empties <= level - 3 { Final(5) }
        else if n_empties <= level { Final(4) }
        else if n_empties <= level + 3 { Final(3) }
        else if n_empties <= level + 6 { Final(2) }
        else if n_empties <= level + 9 { Final(1) }
        else { Eval(level, EVAL_SOLVER_SELECTIVITY) }
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
        let init_w = if depth > 16 { 2 } else { 6 };
        score = aspiration_search_eval(depth, selectivity, init_w, score, candidates, search);
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
    loop {
        n += 1;
        let alpha = (predict - left).max(-SCORE_MAX);
        let beta = (predict + right).min(SCORE_MAX);
        debug_assert!(alpha <= beta);
        predict = search_root_eval_window(depth, alpha, beta, candidates, search);

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
    loop {
        n += 1;
        let alpha = (predict - left).max(-SCORE_MAX);
        let beta = (predict + right).min(SCORE_MAX);
        debug_assert!(alpha <= beta);
        predict = search_root_final_window(alpha, beta, candidates, search);

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
    let mut best_score = -pvs_eval(&candidates[0].1, -beta, -alpha, depth - 1, search);
    if best_score >= beta {
        return best_score;
    }
    if best_score > alpha {
        alpha = best_score;
    }
    let mut best_idx = 0;
    for i in 1..candidates.len() {
        let mut s = -nws_eval(&candidates[i].1, -alpha - 1, depth - 1, search);
        if s >= beta {
            candidates.swap(0, i);
            return s;
        }
        if s > alpha {
            s = -pvs_eval(&candidates[i].1, -beta, -alpha, depth - 1, search);
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
    search: &mut SearchContext,
) -> i32 {
    let mut alpha = alpha;
    let mut best_score = -pvs_final(&candidates[0].1, -beta, -alpha, search);
    if best_score >= beta {
        return best_score;
    }
    if best_score > alpha {
        alpha = best_score;
    }
    let mut best_idx = 0;
    for i in 1..candidates.len() {
        let mut s = -nws_final(&candidates[i].1, -alpha - 1, search);
        if s >= beta {
            candidates.swap(0, i);
            return s;
        }
        if s > alpha {
            s = -pvs_final(&candidates[i].1, -beta, -alpha, search);
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

/// 中盤 PVS の root 探索。最善手と fail-soft スコアを返す。
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

    fn make_solver() -> Solver {
        Solver::new(
            Arc::new(Evaluator::default()),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
        )
    }

    /// 初期盤面で深さ 4 の中盤探索が合法手を返すことを確認する。
    #[test]
    fn solve_eval_initial_board_returns_legal_move() {
        let solver = make_solver();
        let board = Board::new();
        let r = solver.solve_eval(&board, 4, NO_MPC_SELECTIVITY_LV, 0);

        // 初期盤面の合法手は d3, c4, f5, e6 (= 19, 26, 37, 44)
        assert!(matches!(r.best_move, 19 | 26 | 37 | 44),
            "unexpected best move: {}", r.best_move);
        assert!(r.nodes > 0);
    }

    /// 合法手なし → pass → 合法手なし の盤面は終局スコアを返す。
    #[test]
    fn solve_eval_terminal_position_returns_solve_score() {
        let solver = make_solver();
        let board = Board { player: u64::MAX, opponent: 0 };
        let r = solver.solve_eval(&board, 4, NO_MPC_SELECTIVITY_LV, 0);

        assert_eq!(r.best_move, NO_COORD);
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
            let board = Board { player, opponent: !player & !empties };

            let r = solver.solve_final(&board, NO_MPC_SELECTIVITY_LV);
            let expected = brute_force(&board);
            assert_eq!(r.score, expected, "score mismatch on board p={:#018x} o={:#018x}",
                board.player, board.opponent);
        }
    }

    /// 高レベル solve(level) は初期盤面で合法手を返す。
    #[test]
    fn solve_initial_board_returns_legal_move() {
        let solver = make_solver();
        let board = Board::new();
        let r = solver.solve(&board, 4);
        assert!(matches!(r.best_move, 19 | 26 | 37 | 44),
            "unexpected best move: {}", r.best_move);
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
            let board = Board { player, opponent: !player & !empties };
            if board.moves() == 0 && board.passed().moves() == 0 { continue; }

            let r = solver.solve(&board, 60);
            let expected = brute_force(&board);
            assert_eq!(r.score, expected,
                "score mismatch p={:#018x} o={:#018x}", board.player, board.opponent);
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
}
