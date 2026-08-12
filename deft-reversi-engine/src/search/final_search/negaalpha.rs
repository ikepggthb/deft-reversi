//! 終盤の NegaAlpha 完全読み。
//!
//! 葉に近いノードで使う最も単純な完全読み。Move ordering は parity ベースで、
//! TT も MPC も使わない(より深いノードでは `nws_final` / `pvs_final` が呼ばれる)。
//!
//! 空きマスが 4 以下になった時点で専用ソルバに切り替えて打ち切る。

use crate::board::board::Board;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::final_search::solve_score::{
    final_parity, solve_score, solve_score_1_empties, solve_score_2_empties, solve_score_3_empties,
    solve_score_4_empties,
};
use crate::search::move_list::MoveIteratorParity;
use crate::search::search::SearchContext;

/// NegaAlpha で完全読みを行い、現プレイヤー視点のスコアを返す。
pub fn negaalpha_final(board: &Board, alpha: i32, beta: i32, search: &mut SearchContext) -> i32 {
    negaalpha_final_impl(board, alpha, beta, search, beta == alpha + 1)
}

fn negaalpha_final_impl(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    search: &mut SearchContext,
    allow_specialized: bool,
) -> i32 {
    debug_assert!(alpha < beta);
    debug_assert!(-SCORE_MAX <= alpha && beta <= SCORE_MAX);

    search.stats.final_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    let n_empties = (board.player | board.opponent).count_zeros() as i32;

    // 4/3/2 空き専用ソルバは null-window 専用。
    // pvs_final から幅のある窓で呼ばれた場合は下の汎用 NegaAlpha で正確値を返す。
    if allow_specialized {
        if n_empties == 4 {
            return solve_score_4_empties(board.player, board.opponent, alpha, search);
        }
        if n_empties == 3 {
            let empties = !(board.player | board.opponent);
            let x1 = empties.trailing_zeros() as usize;
            let rest = empties & (empties - 1);
            let x2 = rest.trailing_zeros() as usize;
            let rest = rest & (rest - 1);
            let x3 = rest.trailing_zeros() as usize;
            return solve_score_3_empties(
                board.player,
                board.opponent,
                alpha,
                x1,
                x2,
                x3,
                final_parity(board.player, board.opponent),
                search,
            );
        }
        if n_empties == 2 {
            let empties = !(board.player | board.opponent);
            let x1 = empties.trailing_zeros() as usize;
            let x2 = (empties & (empties - 1)).trailing_zeros() as usize;
            return solve_score_2_empties(board.player, board.opponent, alpha, x1, x2, search);
        }
    }
    if n_empties == 1 {
        let empty = (!(board.player | board.opponent)).trailing_zeros() as usize;
        search.stats.final_search_leaf_nodes += 1;
        return solve_score_1_empties(board.player, -SCORE_MAX, empty);
    }
    if n_empties == 0 {
        search.stats.final_search_leaf_nodes += 1;
        return solve_score(board);
    }

    let legal_moves = board.moves();
    if legal_moves == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -negaalpha_final_impl(&passed, -beta, -alpha, search, allow_specialized);
    }

    if legal_moves.is_power_of_two() {
        let child = board.make_move(legal_moves);
        let score = -negaalpha_final_impl(&child, -beta, -alpha, search, allow_specialized);
        return if search.is_aborted() { alpha } else { score };
    }

    let mut best_score = -SCORE_MAX;
    for move_bit in MoveIteratorParity::new(legal_moves, board) {
        let child = board.make_move(move_bit);
        let score = -negaalpha_final_impl(&child, -beta, -alpha, search, allow_specialized);
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
    use crate::eval::{default_evaluator, Evaluator};
    use crate::search::mpc::MpcConfig;
    use crate::search::search::SearchStats;
    use crate::t_table::TranspositionTable;
    use std::sync::Arc;

    fn shared_resources() -> (Arc<dyn Evaluator>, Arc<MpcConfig>, Arc<TranspositionTable>) {
        (
            default_evaluator(),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
        )
    }

    /// 任意の盤面に対するブルートフォース NegaMax のリファレンス。
    /// 浅い場合 (空きマス少) しか呼ばないので、深さの最適化は不要。
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
            let child = board.make_move(m);
            let score = -brute_force(&child);
            if score > best {
                best = score;
            }
        }
        best
    }

    fn next_pseudo_random(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
    }

    /// ランダムに 4〜6 マス空きの盤面を作って、NegaAlpha とブルートフォースが一致するか確認する。
    #[test]
    fn negaalpha_final_matches_brute_force_random() {
        let (evaluator, mpc, tt) = shared_resources();
        let mut rng = 0x1234_5678_9abc_def0_u64;

        for _ in 0..200 {
            // 4〜6 マス空きのビットマスクを作る
            let n_empties = 4 + (next_pseudo_random(&mut rng) % 3) as u32;
            let mut empties = 0u64;
            while empties.count_ones() < n_empties {
                let pos = (next_pseudo_random(&mut rng) % 64) as u32;
                empties |= 1u64 << pos;
            }
            let player = next_pseudo_random(&mut rng) & !empties;
            let opponent = !player & !empties;
            let board = Board { player, opponent };

            let mut stats = SearchStats::default();
            let mut search =
                SearchContext::new(evaluator.clone(), mpc.clone(), tt.clone(), &mut stats);

            let actual = negaalpha_final(&board, -SCORE_MAX, SCORE_MAX, &mut search);
            let expected = brute_force(&board);
            assert_eq!(
                actual, expected,
                "negaalpha_final mismatch (empties={n_empties}, player={player:#018x}, opponent={opponent:#018x})",
            );
        }
    }

    /// alpha-beta 範囲を絞っても正しい値(またはカット境界の上下限)を返すことを確認する。
    /// fail-soft: actual ≥ beta なら true score も ≥ beta、actual ≤ alpha なら true ≤ alpha。
    #[test]
    fn negaalpha_final_respects_alpha_beta_bounds() {
        let (evaluator, mpc, tt) = shared_resources();
        let mut rng = 0xface_b00c_1234_5678_u64;

        for _ in 0..100 {
            let mut empties = 0u64;
            while empties.count_ones() < 4 {
                let pos = (next_pseudo_random(&mut rng) % 64) as u32;
                empties |= 1u64 << pos;
            }
            let player = next_pseudo_random(&mut rng) & !empties;
            let opponent = !player & !empties;
            let board = Board { player, opponent };

            let true_score = brute_force(&board);

            for &(alpha, beta) in &[(-10, 10), (0, 1), (-5, 0), (5, 10), (-SCORE_MAX, SCORE_MAX)] {
                if alpha >= beta {
                    continue;
                }
                let mut stats = SearchStats::default();
                let mut search =
                    SearchContext::new(evaluator.clone(), mpc.clone(), tt.clone(), &mut stats);
                let bounded = negaalpha_final(&board, alpha, beta, &mut search);

                if true_score >= beta {
                    assert!(
                        bounded >= beta,
                        "fail-high broken: alpha={alpha} beta={beta} bounded={bounded} true={true_score}",
                    );
                } else if true_score <= alpha {
                    assert!(
                        bounded <= alpha,
                        "fail-low broken: alpha={alpha} beta={beta} bounded={bounded} true={true_score}",
                    );
                } else {
                    assert_eq!(
                        bounded, true_score,
                        "exact mismatch: alpha={alpha} beta={beta}",
                    );
                }
            }
        }
    }
}
