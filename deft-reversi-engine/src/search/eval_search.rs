use crate::board::board::Board;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::mpc::{eval_search_mpc, ProbCutResult};
use crate::search::search::EvalSearch;

/// 終局盤面の exact score を返す。
///
/// 空きマスが残っていても、勝っている側に空きマスを与えた値に変換する。
#[inline(always)]
pub fn solve_score(board: &Board) -> i32 {
    let n_player = board.player.count_ones() as i32;
    let n_opponent = board.opponent.count_ones() as i32;
    let diff = n_player - n_opponent;

    if diff > 0 {
        diff + (64 - n_player - n_opponent)
    } else if diff < 0 {
        diff - (64 - n_player - n_opponent)
    } else {
        0
    }
}

/// 末端側の単純な null-window 探索。
///
/// move ordering や TT はまだ使わないが、MPC はここから差し込む。
pub fn nws_eval_leaf(board: &Board, alpha: i32, depth: i32, search: &mut EvalSearch) -> i32 {
    nws_eval_leaf_impl(board, alpha, depth, true, search)
}

pub(crate) fn nws_eval_leaf_no_mpc(
    board: &Board,
    alpha: i32,
    depth: i32,
    search: &mut EvalSearch,
) -> i32 {
    nws_eval_leaf_impl(board, alpha, depth, false, search)
}

fn nws_eval_leaf_impl(
    board: &Board,
    alpha: i32,
    depth: i32,
    use_mpc: bool,
    search: &mut EvalSearch,
) -> i32 {
    debug_assert!(alpha < SCORE_MAX);

    search.stats.nodes += 1;

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
    let mut current_alpha = alpha;

    while moves_bit != 0 {
        let move_bit = moves_bit & moves_bit.wrapping_neg();
        moves_bit &= moves_bit - 1;

        let child = board.make_move(move_bit);

        let score = -nws_eval_leaf_impl(&child, -beta, depth - 1, use_mpc, search);
        if score >= beta {
            return score;
        }
        if score > current_alpha {
            current_alpha = score;
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
    fn solve_score_returns_zero_for_initial_board() {
        assert_eq!(solve_score(&Board::new()), 0);
    }

    #[test]
    fn nws_eval_leaf_depth_zero_matches_static_eval() {
        let board = played_board();
        let evaluator = Arc::new(Evaluator::default());
        let mpc = Arc::new(MpcConfig::default());
        let tt = Arc::new(TranspositionTable::new());
        let mut stats = crate::search::search::SearchStats::default();
        let mut search = EvalSearch::new(evaluator.clone(), mpc, tt.clone(), &mut stats);

        assert_eq!(
            nws_eval_leaf(&board, -SCORE_MAX, 0, &mut search),
            evaluator.evaluate_board_slow(&board)
        );
        assert_eq!(search.stats.nodes, 1);
        assert_eq!(search.stats.eval_search_leaf_nodes, 1);
    }
}
