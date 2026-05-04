//! 終盤の確定スコア計算と、空きマスが残り少ないときの高速 leaf solver。
//!
//! - `solve_score`: 任意の盤面に対する終局スコア(空きマスは勝っている側に加算)
//! - `solve_score_0_empties`: 0 マス残りの専用版
//! - `solve_score_1_empties`: 1 マス残り。`count_last_flip` を使い flip を取らずに数えるだけ
//! - `solve_score_2_empties`: 2 マス残り。NegaAlpha 1 段で `solve_score_1_empties` を呼ぶ

use crate::board::board::Board;
use crate::search::final_search::count_last_flip::count_last_flip;
use crate::search::search::SearchContext;

/// 終局盤面の exact score を返す。
///
/// 空きマスが残っていても、勝っている側に空きマス数を加算する(終局時の慣習)。
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

/// 空きマスが 0 の盤面のスコア。`solve_score` と等価だが定数除外で速い。
#[inline(always)]
pub(crate) fn solve_score_0_empties(board: &Board) -> i32 {
    debug_assert_eq!(board.player | board.opponent, u64::MAX);
    debug_assert_eq!(
        2 * (board.player.count_ones() as i32) - 64,
        solve_score(board)
    );

    2 * (board.player.count_ones() as i32) - 64
}

/// 空きマスが 1 マス (`pos`) だけ残っている盤面のスコア(現プレイヤー視点)。
///
/// プレイヤー / 相手 / どちらも置けない の 3 通りを内部で解決し、最終スコアを返す。
/// 旧実装には「双方置けず P < O」のとき `2P-62` を返す不具合があったため、ここで修正済み。
#[inline(always)]
pub(crate) fn solve_score_1_empties(player: u64, pos: usize) -> i32 {
    debug_assert!(pos < 64);

    let n_flips = count_last_flip(pos, player);
    let player_count_x2 = 2 * player.count_ones() as i32;

    if n_flips != 0 {
        // プレイヤーが pos に着手し、n_flips/2 枚を反転して終局
        return player_count_x2 - 64 + 2 + n_flips;
    }

    let n_flips = count_last_flip(pos, !player);
    if n_flips != 0 {
        // プレイヤーはパス、相手が pos に着手して終局
        return player_count_x2 - 64 - n_flips;
    }

    // どちらも置けない -> 空きマスは未確定。`solve_score` の慣習に揃え、
    // 勝っている側に空きマス分を加算する。63 マス埋まっているので diff は奇数で 0 にはならない。
    let diff = player_count_x2 - 63;
    if diff > 0 {
        diff + 1
    } else {
        diff - 1
    }
}

/// `pos` の周囲 1 マス(8 近傍)を立てたビットマスク。
///
/// 旧実装からの定数表。空きマスの近傍に相手の石が無ければ着手しても 1 枚も
/// 反転できないので、`flip_bit` の高コスト計算をスキップする早期判定に使う。
///
/// 参考: <https://eukaryote.hateblo.jp/entry/2020/04/26/031246>
#[rustfmt::skip]
const NEIGHBOUR: [u64; 64] = [
    0x0000000000000302, 0x0000000000000604, 0x0000000000000e0a, 0x0000000000001c14, 0x0000000000003828, 0x0000000000007050, 0x0000000000006020, 0x000000000000c040,
    0x0000000000030200, 0x0000000000060400, 0x00000000000e0a00, 0x00000000001c1400, 0x0000000000382800, 0x0000000000705000, 0x0000000000602000, 0x0000000000c04000,
    0x0000000003020300, 0x0000000006040600, 0x000000000e0a0e00, 0x000000001c141c00, 0x0000000038283800, 0x0000000070507000, 0x0000000060206000, 0x00000000c040c000,
    0x0000000302030000, 0x0000000604060000, 0x0000000e0a0e0000, 0x0000001c141c0000, 0x0000003828380000, 0x0000007050700000, 0x0000006020600000, 0x000000c040c00000,
    0x0000030203000000, 0x0000060406000000, 0x00000e0a0e000000, 0x00001c141c000000, 0x0000382838000000, 0x0000705070000000, 0x0000602060000000, 0x0000c040c0000000,
    0x0003020300000000, 0x0006040600000000, 0x000e0a0e00000000, 0x001c141c00000000, 0x0038283800000000, 0x0070507000000000, 0x0060206000000000, 0x00c040c000000000,
    0x0002030000000000, 0x0004060000000000, 0x000a0e0000000000, 0x00141c0000000000, 0x0028380000000000, 0x0050700000000000, 0x0020600000000000, 0x0040c00000000000,
    0x0203000000000000, 0x0406000000000000, 0x0a0e000000000000, 0x141c000000000000, 0x2838000000000000, 0x5070000000000000, 0x2060000000000000, 0x40c0000000000000,
];

/// 空きマスが 2 マスの盤面を NegaAlpha 1 段で解く。
///
/// 各候補手の後で `solve_score_1_empties` を呼ぶことで完全に解ける。
/// パスが必要な場合は再帰で対応(2 連続パスで終局)。
pub(crate) fn solve_score_2_empties(
    board: &Board,
    alpha: i32,
    beta: i32,
    search: &mut SearchContext,
) -> i32 {
    debug_assert!(alpha < beta);
    debug_assert_eq!((board.player | board.opponent).count_zeros(), 2);

    search.stats.final_search_nodes += 1;

    let empties = !(board.player | board.opponent);
    let first = empties & empties.wrapping_neg();
    let second = empties ^ first;
    let first_pos = first.trailing_zeros() as usize;
    let second_pos = second.trailing_zeros() as usize;

    let mut best_score = -i32::MAX;

    // first に着手
    if NEIGHBOUR[first_pos] & board.opponent != 0 {
        let flip = board.flip_bit(first);
        if flip != 0 {
            search.stats.final_search_nodes += 1;
            search.stats.final_search_leaf_nodes += 1;
            let score = -solve_score_1_empties(board.opponent ^ flip, second_pos);
            if score >= beta {
                return score;
            }
            best_score = score;
        }
    }

    // second に着手
    if NEIGHBOUR[second_pos] & board.opponent != 0 {
        let flip = board.flip_bit(second);
        if flip != 0 {
            search.stats.final_search_nodes += 1;
            search.stats.final_search_leaf_nodes += 1;
            let score = -solve_score_1_empties(board.opponent ^ flip, first_pos);
            if score > best_score {
                best_score = score;
            }
            return best_score;
        }
    }

    if best_score != -i32::MAX {
        return best_score;
    }

    // どちらにも着手できない: パス または 終局
    if board.opponent_moves() == 0 {
        search.stats.final_search_leaf_nodes += 1;
        return solve_score(board);
    }
    -solve_score_2_empties(&board.passed(), -beta, -alpha, search)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::mpc::MpcConfig;
    use crate::search::search::SearchStats;
    use crate::t_table::TranspositionTable;
    use std::sync::Arc;

    fn shared_resources() -> (Arc<Evaluator>, Arc<MpcConfig>, Arc<TranspositionTable>) {
        (
            Arc::new(Evaluator::default()),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
        )
    }

    #[test]
    fn solve_score_returns_zero_for_initial_board() {
        assert_eq!(solve_score(&Board::new()), 0);
    }

    /// 任意の 1 マス空き盤面で、ブルートフォース minimax と一致するか確認する。
    #[test]
    fn solve_score_1_empties_matches_brute_force() {
        let mut rng = 0xc0ffee_dead_beef_u64;
        for _ in 0..400 {
            for empty_pos in 0..64usize {
                let empty_bit = 1u64 << empty_pos;
                let player = next_pseudo_random(&mut rng) & !empty_bit;
                let opponent = !player & !empty_bit;
                let board = Board { player, opponent };

                let expected = brute_force_negamax(&board);
                let actual = solve_score_1_empties(player, empty_pos);
                assert_eq!(
                    actual, expected,
                    "1-empty mismatch at pos={empty_pos}, player={player:#018x}",
                );
            }
        }
    }

    /// 1 マスのみ空き、双方着手できない P<O のケースで旧実装の不具合が修正されているか確認する。
    ///
    /// 構築: a8 だけ空、player は b1 の 1 個、opponent が残り 62 個。
    /// a8 から見た 3 方向 (E,S,SE) は全部 opponent だが、それを挟む player が無いため
    /// 双方とも a8 では 1 枚も反転できない。
    #[test]
    fn solve_score_1_empties_handles_double_pass_p_lt_o() {
        let empty_pos = 56usize; // a8
        let player = 1u64 << 1; // b1
        let opponent = !player & !(1u64 << empty_pos);
        let board = Board { player, opponent };
        assert_eq!(board.moves(), 0, "player should have no legal move");
        assert_eq!(board.opponent_moves(), 0, "opponent should have no legal move either");

        let expected = solve_score(&board);
        assert_eq!(solve_score_1_empties(player, empty_pos), expected);
        // P=1, O=62, empty=1 → diff=-61, 多い側 (opponent) に空きマスを加算 → -62
        assert_eq!(expected, -62);
    }

    /// 任意の 2 マス空き盤面で、ブルートフォース minimax と一致するか確認する。
    ///
    /// (first_pos, second_pos, player_bits) の組をランダムに数百ケース生成して比較する。
    /// `solve_score_2_empties` の合法パス全部 (両マス着手不可、片方のみ着手不可、双方着手可、
    /// 連続パスからの終局) をカバーするには、空きマスの距離・配置のバリエーションが必要。
    #[test]
    fn solve_score_2_empties_matches_brute_force() {
        let mut rng = 0xfeed_face_1234_5678_u64;
        let (evaluator, mpc, tt) = shared_resources();
        for _ in 0..400 {
            let first_pos = (next_pseudo_random(&mut rng) % 63) as usize;
            let mut second_pos = (next_pseudo_random(&mut rng) % 64) as usize;
            if second_pos == first_pos {
                second_pos = (first_pos + 1) % 64;
            }
            let empty = (1u64 << first_pos) | (1u64 << second_pos);
            let player = next_pseudo_random(&mut rng) & !empty;
            let opponent = !player & !empty;
            let board = Board { player, opponent };

            let mut stats = SearchStats::default();
            let mut search =
                SearchContext::new(evaluator.clone(), mpc.clone(), tt.clone(), &mut stats);
            let actual = solve_score_2_empties(&board, -i32::MAX, i32::MAX, &mut search);
            let expected = brute_force_negamax(&board);
            assert_eq!(
                actual, expected,
                "2-empty mismatch at {first_pos},{second_pos}, player={player:#018x}",
            );
        }
    }

    /// 任意の盤面に対する NegaMax (合法手列挙 + 2 連続パスで終局) のリファレンス実装。
    ///
    /// 0/1/2 マス空きでしか呼ばないので深さは高々 3。十分速い。
    fn brute_force_negamax(board: &Board) -> i32 {
        let moves = board.moves();
        if moves == 0 {
            if board.opponent_moves() == 0 {
                return solve_score(board);
            }
            return -brute_force_negamax(&board.passed());
        }
        let mut best = -i32::MAX;
        let mut bits = moves;
        while bits != 0 {
            let m = bits & bits.wrapping_neg();
            bits ^= m;
            let child = board.make_move(m);
            let score = -brute_force_negamax(&child);
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
}
