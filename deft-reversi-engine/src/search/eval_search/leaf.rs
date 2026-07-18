//! 末端側の単純な NWS 評価探索。
//!
//! TT や move ordering は使わず、MPC のみ差し込む。
//! 上位の `nws_eval` / `pvs_eval` から depth が浅くなったときに呼ばれる。

use crate::board::board::Board;
use crate::eval::evaluator::Evaluator;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::eval::feature_indexes::FeatureIndexes;
use crate::search::final_search::solve_score;
use crate::search::mpc::{eval_search_mpc, ProbCutResult};
use crate::search::search::SearchContext;

/// MPC ありの NWS 葉探索。
pub fn nws_eval_leaf(board: &Board, alpha: i32, depth: i32, search: &mut SearchContext) -> i32 {
    match search.evaluator.as_ref() {
        Evaluator::Nnue(_) => nws_eval_leaf_slow_impl(board, alpha, depth, true, search),
        Evaluator::Pattern(_) => nws_eval_leaf_slow_impl(board, alpha, depth, true, search),
    }
}

/// MPC なしの NWS 葉探索。move ordering の事前評価などに使う。
pub(crate) fn nws_eval_leaf_no_mpc(
    board: &Board,
    alpha: i32,
    depth: i32,
    search: &mut SearchContext,
) -> i32 {
    match search.evaluator.as_ref() {
        Evaluator::Nnue(_) => nws_eval_leaf_slow_impl(board, alpha, depth, false, search),
        Evaluator::Pattern(_) => nws_eval_leaf_slow_impl(board, alpha, depth, false, search),
    }
}

fn nws_eval_leaf_slow_impl(
    board: &Board,
    alpha: i32,
    depth: i32,
    use_mpc: bool,
    search: &mut SearchContext,
) -> i32 {
    debug_assert!(alpha < SCORE_MAX);

    search.stats.eval_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

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
        return -nws_eval_leaf_slow_impl(&passed, -beta, depth, use_mpc, search);
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
        let score = -nws_eval_leaf_slow_impl(&child, -beta, depth - 1, use_mpc, search);
        if search.is_aborted() {
            return alpha;
        }
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
    alpha: i32,
    beta: i32,
    depth: i32,
    search: &mut SearchContext,
) -> i32 {
    match search.evaluator.as_ref() {
        Evaluator::Nnue(_) => negaalpha_eval_leaf_slow_impl(board, alpha, beta, depth, search),
        Evaluator::Pattern(_) => negaalpha_eval_leaf_slow_impl(board, alpha, beta, depth, search),
    }
}

pub(crate) fn negaalpha_eval_ordering(
    board: &Board,
    alpha: i32,
    beta: i32,
    depth: i32,
    search: &mut SearchContext,
) -> i32 {
    negaalpha_eval_ordering_impl(board, alpha, beta, depth, search)
}

/// `board.passed()` の特徴を受け取り、深さ1の手順評価を差分更新だけで行う。
pub(crate) fn negaalpha_eval_ordering_depth_one(
    board: &Board,
    swapped: &FeatureIndexes,
    mut alpha: i32,
    beta: i32,
    search: &mut SearchContext,
) -> i32 {
    search.stats.eval_search_nodes += 1;
    if search.check_abort() {
        return alpha;
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
        return -negaalpha_eval_ordering_impl(&passed, -beta, -alpha, 1, search);
    }

    let mut best_score = -SCORE_MAX;
    while moves_bit != 0 {
        let move_bit = moves_bit & moves_bit.wrapping_neg();
        moves_bit &= moves_bit - 1;
        let flip = board.flip_bit(move_bit);
        let child = board.make_move_from_flip_bit(move_bit, flip);
        let state = swapped.child_from_swapped(move_bit, flip);

        search.stats.eval_search_nodes += 1;
        if search.check_abort() {
            return alpha;
        }
        search.stats.eval_search_leaf_nodes += 1;
        let score = match search.ordering_evaluator.as_ref() {
            Evaluator::Pattern(evaluator) => -evaluator.evaluate(&child, &state),
            Evaluator::Nnue(_) => unreachable!("pattern fast path selected for NNUE evaluator"),
        };
        if score >= beta {
            return score;
        }
        alpha = alpha.max(score);
        best_score = best_score.max(score);
    }
    best_score
}

fn negaalpha_eval_ordering_impl(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    depth: i32,
    search: &mut SearchContext,
) -> i32 {
    debug_assert!(alpha < beta);

    search.stats.eval_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    if depth <= 0 {
        search.stats.eval_search_leaf_nodes += 1;
        return search.ordering_evaluator.evaluate_board_slow(board);
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
        return -negaalpha_eval_ordering_impl(&passed, -beta, -alpha, depth, search);
    }

    if depth == 1 && matches!(search.ordering_evaluator.as_ref(), Evaluator::Pattern(_)) {
        let swapped = FeatureIndexes::from_board(&board.passed());
        let mut best_score = -SCORE_MAX;
        while moves_bit != 0 {
            let move_bit = moves_bit & moves_bit.wrapping_neg();
            moves_bit &= moves_bit - 1;

            let flip = board.flip_bit(move_bit);
            let child = board.make_move_from_flip_bit(move_bit, flip);
            let state = swapped.child_from_swapped(move_bit, flip);

            // 通常の depth=0 再帰と同じ統計・中断確認を行う。
            search.stats.eval_search_nodes += 1;
            if search.check_abort() {
                return alpha;
            }
            search.stats.eval_search_leaf_nodes += 1;
            let score = match search.ordering_evaluator.as_ref() {
                Evaluator::Pattern(evaluator) => -evaluator.evaluate(&child, &state),
                Evaluator::Nnue(_) => unreachable!("pattern batch selected for NNUE evaluator"),
            };
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
        return best_score;
    }

    let mut best_score = -SCORE_MAX;
    while moves_bit != 0 {
        let move_bit = moves_bit & moves_bit.wrapping_neg();
        moves_bit &= moves_bit - 1;

        let child = board.make_move(move_bit);
        let score = -negaalpha_eval_ordering_impl(&child, -beta, -alpha, depth - 1, search);
        if search.is_aborted() {
            return alpha;
        }
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

fn negaalpha_eval_leaf_slow_impl(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    depth: i32,
    search: &mut SearchContext,
) -> i32 {
    debug_assert!(alpha < beta);

    search.stats.eval_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

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
        return -negaalpha_eval_leaf_slow_impl(&passed, -beta, -alpha, depth, search);
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
        let score = -negaalpha_eval_leaf_slow_impl(&child, -beta, -alpha, depth - 1, search);
        if search.is_aborted() {
            return alpha;
        }
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
        let board = Board::new();
        let flip = board.flip_bit(D3);
        board.make_move_from_flip_bit(D3, flip);
        board
    }

    fn search_context<'a>(
        stats: &'a mut SearchStats,
        evaluator: &Arc<Evaluator>,
        mpc: &Arc<MpcConfig>,
        tt: &Arc<TranspositionTable>,
    ) -> SearchContext<'a> {
        SearchContext::new(evaluator.clone(), mpc.clone(), tt.clone(), stats)
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

    #[test]
    fn ordering_depth_one_with_parent_indexes_matches_regular_path() {
        let mut board = Board::new();
        let evaluator = Arc::new(Evaluator::default());
        let mpc = Arc::new(MpcConfig::default());
        let tt = Arc::new(TranspositionTable::new());

        for ply in 0..24 {
            let moves = board.moves();
            if moves == 0 {
                board = board.passed();
                if board.moves() == 0 {
                    break;
                }
                continue;
            }

            let swapped = FeatureIndexes::from_board(&board.passed());
            for (alpha, beta) in [(-SCORE_MAX, SCORE_MAX), (-1, 0), (0, 1), (5, 12)] {
                let mut regular_stats = SearchStats::default();
                let mut regular = search_context(&mut regular_stats, &evaluator, &mpc, &tt);
                let expected = negaalpha_eval_ordering(&board, alpha, beta, 1, &mut regular);

                let mut indexed_stats = SearchStats::default();
                let mut indexed = search_context(&mut indexed_stats, &evaluator, &mpc, &tt);
                let actual =
                    negaalpha_eval_ordering_depth_one(&board, &swapped, alpha, beta, &mut indexed);

                assert_eq!(actual, expected, "ply={ply}, window=[{alpha}, {beta})");
            }

            let selected = ply as usize % moves.count_ones() as usize;
            let mut candidates = moves;
            for _ in 0..selected {
                candidates &= candidates - 1;
            }
            board = board.make_move(candidates & candidates.wrapping_neg());
        }
    }
}
