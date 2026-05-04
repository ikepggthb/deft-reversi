//! 末端側の単純な NWS 評価探索。
//!
//! TT や move ordering は使わず、MPC のみ差し込む。
//! 上位の `nws_eval` / `pvs_eval` から depth が浅くなったときに呼ばれる。

use crate::board::board::Board;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::final_search::solve_score;
use crate::search::mpc::{eval_search_mpc, ProbCutResult};
use crate::search::search::SearchContext;

/// MPC ありの NWS 葉探索。
pub fn nws_eval_leaf(board: &Board, alpha: i32, depth: i32, search: &mut SearchContext) -> i32 {
    nws_eval_leaf_impl(board, alpha, depth, true, search)
}

/// MPC なしの NWS 葉探索。move ordering の事前評価などに使う。
pub(crate) fn nws_eval_leaf_no_mpc(
    board: &Board,
    alpha: i32,
    depth: i32,
    search: &mut SearchContext,
) -> i32 {
    nws_eval_leaf_impl(board, alpha, depth, false, search)
}

fn nws_eval_leaf_impl(
    board: &Board,
    alpha: i32,
    depth: i32,
    use_mpc: bool,
    search: &mut SearchContext,
) -> i32 {
    debug_assert!(alpha < SCORE_MAX);

    search.stats.eval_search_nodes += 1;

    if depth <= 0 {
        search.stats.eval_search_leaf_nodes += 1;
        return search.evaluator.evaluate_board_slow(board);
    }

    if board.player | board.opponent == u64::MAX {
        return solve_score(board);
    }

    let beta = alpha + 1;
    let mut moves_bit = board.moves();

    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.eval_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -nws_eval_leaf_impl(&passed, -beta, depth, use_mpc, search);
    }

    if use_mpc {
        match eval_search_mpc(board, alpha, beta, depth, search) {
            ProbCutResult::Cut(score) => return score,
            ProbCutResult::Fail => {}
        }
    }

    let mut best_score = -SCORE_MAX;

    while moves_bit != 0 {
        let move_bit = moves_bit & moves_bit.wrapping_neg();
        moves_bit &= moves_bit - 1;

        let child = board.make_move(move_bit);
        let score = -nws_eval_leaf_impl(&child, -beta, depth - 1, use_mpc, search);
        if score >= beta {
            return score;
        }
        if score > best_score {
            best_score = score;
        }
    }

    best_score
}

/// 全幅(NegaAlpha)の葉評価探索。`pvs_eval` の基底として使う。
///
/// TT は使わないが MPC は適用する。
pub fn negaalpha_eval_leaf(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    depth: i32,
    search: &mut SearchContext,
) -> i32 {
    debug_assert!(alpha < beta);

    search.stats.eval_search_nodes += 1;

    if depth <= 0 {
        search.stats.eval_search_leaf_nodes += 1;
        return search.evaluator.evaluate_board_slow(board);
    }

    if board.player | board.opponent == u64::MAX {
        return solve_score(board);
    }

    let mut moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.eval_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -negaalpha_eval_leaf(&passed, -beta, -alpha, depth, search);
    }

    match eval_search_mpc(board, alpha, beta, depth, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    let mut best_score = -SCORE_MAX;
    while moves_bit != 0 {
        let move_bit = moves_bit & moves_bit.wrapping_neg();
        moves_bit &= moves_bit - 1;

        let child = board.make_move(move_bit);
        let score = -negaalpha_eval_leaf(&child, -beta, -alpha, depth - 1, search);
        if score >= beta {
            return score;
        }
        if score > alpha {
            alpha = score;
        }
        if score > best_score {
            best_score = score;
        }
    }

    best_score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::mpc::MpcConfig;
    use crate::search::search::SearchStats;
    use crate::t_table::TranspositionTable;
    use std::sync::Arc;

    const D3: u64 = 1u64 << 19;

    fn played_board() -> Board {
        let mut board = Board::new();
        let flip = board.flip_bit(D3);
        board.make_move_from_flip_bit(D3, flip);
        board
    }

    #[test]
    fn nws_eval_leaf_depth_zero_matches_static_eval() {
        let board = played_board();
        let evaluator = Arc::new(Evaluator::default());
        let mpc = Arc::new(MpcConfig::default());
        let tt = Arc::new(TranspositionTable::new());
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(evaluator.clone(), mpc, tt.clone(), &mut stats);

        assert_eq!(
            nws_eval_leaf(&board, -SCORE_MAX, 0, &mut search),
            evaluator.evaluate_board_slow(&board)
        );
        assert_eq!(search.stats.eval_search_nodes, 1);
        assert_eq!(search.stats.eval_search_leaf_nodes, 1);
    }
}
