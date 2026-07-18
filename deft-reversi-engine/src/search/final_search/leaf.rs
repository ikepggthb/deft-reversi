use crate::{
    board::board::Board,
    eval::evaluator_const::SCORE_MAX,
    search::final_search::{count_last_flip::*, solve_score::*},
    search::stability_cut::stability_cut_nws,
    search::SearchContext,
};

/// 空きマスが 0 の盤面のスコア。`solve_score` と等価だが定数除外で速い。
// Specialized leaf kept for future final-search micro-optimization.
#[allow(dead_code)]
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
pub(crate) fn solve_score_1_empties(player: u64, alpha: i32, pos: usize) -> i32 {
    debug_assert!(pos < 64);

    let n_flips = count_last_flip(pos, player);
    let player_count_x2 = 2 * player.count_ones() as i32;

    if n_flips != 0 {
        // プレイヤーが pos に着手し、n_flips/2 枚を反転して終局
        return player_count_x2 - 64 + 2 + n_flips;
    }

    let mut score = player_count_x2 - 64 + 2;
    if score <= 0 {
        score -= 2;
        if score > alpha {
            score -= count_last_flip(pos, !player);
        }
        return score;
    }

    if score > alpha {
        let opponent_flips = count_last_flip(pos, !player);
        if opponent_flips != 0 {
            score -= opponent_flips + 2;
            return score;
        }
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

#[rustfmt::skip]
const QUADRANT_ID: [i32; 64] = [
    1, 1, 1, 1, 2, 2, 2, 2,
    1, 1, 1, 1, 2, 2, 2, 2,
    1, 1, 1, 1, 2, 2, 2, 2,
    1, 1, 1, 1, 2, 2, 2, 2,
    4, 4, 4, 4, 8, 8, 8, 8,
    4, 4, 4, 4, 8, 8, 8, 8,
    4, 4, 4, 4, 8, 8, 8, 8,
    4, 4, 4, 4, 8, 8, 8, 8,
];

#[rustfmt::skip]
const SQUARE_VALUE: [i32; 64] = [
    18,  4, 16, 12, 12, 16,  4, 18,
     4,  2,  6,  8,  8,  6,  2,  4,
    16,  6, 14, 10, 10, 14,  6, 16,
    12,  8, 10,  0,  0, 10,  8, 12,
    12,  8, 10,  0,  0, 10,  8, 12,
    16,  6, 14, 10, 10, 14,  6, 16,
     4,  2,  6,  8,  8,  6,  2,  4,
    18,  4, 16, 12, 12, 16,  4, 18,
];

#[inline(always)]
fn flip_at(player: u64, opponent: u64, pos: usize) -> u64 {
    Board { player, opponent }.flip_bit(1u64 << pos)
}

#[inline(always)]
fn solve_score_bits(player: u64, n_empties: i32) -> i32 {
    let diff = 2 * player.count_ones() as i32 - 64 + n_empties;
    if diff > 0 {
        diff + n_empties
    } else if diff < 0 {
        diff - n_empties
    } else {
        0
    }
}

#[inline(always)]
pub(crate) fn final_parity(player: u64, opponent: u64) -> i32 {
    let mut empties = !(player | opponent);
    let mut parity = 0;
    while empties != 0 {
        let pos = empties.trailing_zeros() as usize;
        empties &= empties - 1;
        parity ^= QUADRANT_ID[pos];
    }
    parity
}

/// 空きマスが 2 マスの盤面を NegaAlpha 1 段で解く。
///
/// 各候補手の後で `solve_score_1_empties` を呼ぶことで完全に解ける。
/// パスが必要な場合は再帰で対応(2 連続パスで終局)。
pub(crate) fn solve_score_2_empties(
    player: u64,
    opponent: u64,
    alpha: i32,
    x1: usize,
    x2: usize,
    search: &mut SearchContext,
) -> i32 {
    debug_assert_eq!((player | opponent).count_zeros(), 2);

    search.stats.final_search_nodes += 1;

    let beta = alpha + 1;
    let mut best = -SCORE_MAX - 1;

    if NEIGHBOUR[x1] & opponent != 0 {
        let flip = flip_at(player, opponent, x1);
        if flip != 0 {
            search.stats.final_search_nodes += 1;
            search.stats.final_search_leaf_nodes += 1;
            let score = -solve_score_1_empties(opponent ^ flip, -beta, x2);
            if score >= beta {
                return score;
            }
            best = score;
        }
    }

    if NEIGHBOUR[x2] & opponent != 0 {
        let flip = flip_at(player, opponent, x2);
        if flip != 0 {
            search.stats.final_search_nodes += 1;
            search.stats.final_search_leaf_nodes += 1;
            let score = -solve_score_1_empties(opponent ^ flip, -beta, x1);
            if score > best {
                best = score;
            }
        }
    }

    if best != -SCORE_MAX - 1 {
        return best;
    }

    // pass後も残り2マスだけを直接調べる。汎用moves生成と再帰は不要。
    search.stats.final_search_nodes += 1;
    let mut passed_best = SCORE_MAX + 1;

    if NEIGHBOUR[x1] & player != 0 {
        let flip = flip_at(opponent, player, x1);
        if flip != 0 {
            search.stats.final_search_nodes += 1;
            search.stats.final_search_leaf_nodes += 1;
            let score = solve_score_1_empties(player ^ flip, alpha, x2);
            if score <= alpha {
                return score;
            }
            passed_best = score;
        }
    }

    if NEIGHBOUR[x2] & player != 0 {
        let flip = flip_at(opponent, player, x2);
        if flip != 0 {
            search.stats.final_search_nodes += 1;
            search.stats.final_search_leaf_nodes += 1;
            let score = solve_score_1_empties(player ^ flip, alpha, x1);
            if score < passed_best {
                passed_best = score;
            }
        }
    }

    if passed_best != SCORE_MAX + 1 {
        passed_best
    } else {
        search.stats.final_search_leaf_nodes += 1;
        solve_score_bits(player, 2)
    }
}

pub(crate) fn solve_score_3_empties(
    player: u64,
    opponent: u64,
    _alpha: i32,
    _x1: usize,
    _x2: usize,
    _x3: usize,
    _parity: i32,
    search: &mut SearchContext,
) -> i32 {
    search.stats.final_search_nodes += 1;
    let alpha = _alpha;
    let mut x1 = _x1;
    let mut x2 = _x2;
    let mut x3 = _x3;
    let parity = _parity;
    let beta = alpha + 1;

    if parity & QUADRANT_ID[x1] == 0 {
        if parity & QUADRANT_ID[x2] != 0 {
            let tmp = x1;
            x1 = x2;
            if SQUARE_VALUE[x3] > SQUARE_VALUE[tmp] {
                x2 = x3;
                x3 = tmp;
            } else {
                x2 = tmp;
            }
        } else {
            let tmp = x1;
            x1 = x3;
            if SQUARE_VALUE[x2] > SQUARE_VALUE[tmp] {
                x3 = tmp;
            } else {
                x3 = x2;
                x2 = tmp;
            }
        }
    } else {
        if SQUARE_VALUE[x3] > SQUARE_VALUE[x2] {
            std::mem::swap(&mut x2, &mut x3);
        }
        if SQUARE_VALUE[x2] > SQUARE_VALUE[x1] {
            let tmp = x2;
            x2 = x3;
            x3 = tmp;
        }
    }

    let mut best = -SCORE_MAX - 1;
    if let Some(score) = solve3_move(player, opponent, alpha, x1, x2, x3, search) {
        if score >= beta {
            return score;
        }
        best = score;
    }
    if let Some(score) = solve3_move(player, opponent, alpha, x2, x1, x3, search) {
        if score >= beta {
            return score;
        }
        if score > best {
            best = score;
        }
    }
    if let Some(score) = solve3_move(player, opponent, alpha, x3, x1, x2, search) {
        if score > best {
            best = score;
        }
    }

    if best != -SCORE_MAX - 1 {
        return best;
    }

    // pass後は、整列済みの3マスを相手側から直接調べる。
    search.stats.final_search_nodes += 1;
    let mut passed_best = SCORE_MAX + 1;
    for (x, y, z) in [(x1, x2, x3), (x2, x1, x3), (x3, x1, x2)] {
        if NEIGHBOUR[x] & player == 0 {
            continue;
        }
        let flip = flip_at(opponent, player, x);
        if flip == 0 {
            continue;
        }
        let bit = 1u64 << x;
        let score =
            solve_score_2_empties(player ^ flip, opponent ^ (flip | bit), alpha, y, z, search);
        if score <= alpha {
            return score;
        }
        if score < passed_best {
            passed_best = score;
        }
    }

    if passed_best != SCORE_MAX + 1 {
        passed_best
    } else {
        search.stats.final_search_leaf_nodes += 1;
        solve_score_bits(player, 3)
    }
}

#[inline(always)]
fn solve3_move(
    player: u64,
    opponent: u64,
    alpha: i32,
    x: usize,
    y: usize,
    z: usize,
    search: &mut SearchContext,
) -> Option<i32> {
    if NEIGHBOUR[x] & opponent == 0 {
        return None;
    }
    let flip = flip_at(player, opponent, x);
    if flip == 0 {
        return None;
    }
    let bit = 1u64 << x;
    Some(-solve_score_2_empties(
        opponent ^ flip,
        player ^ (flip | bit),
        -alpha - 1,
        y,
        z,
        search,
    ))
}

pub(crate) fn solve_score_4_empties(
    player: u64,
    opponent: u64,
    alpha: i32,
    search: &mut SearchContext,
) -> i32 {
    search.stats.final_search_nodes += 1;

    let board = Board { player, opponent };
    if let Some(score) = stability_cut_nws(&board, alpha, 4, search) {
        return score;
    }

    let mut empties = !(player | opponent);
    let mut x1 = empties.trailing_zeros() as usize;
    empties &= empties - 1;
    let mut x2 = empties.trailing_zeros() as usize;
    empties &= empties - 1;
    let mut x3 = empties.trailing_zeros() as usize;
    empties &= empties - 1;
    let mut x4 = empties.trailing_zeros() as usize;
    let parity = final_parity(player, opponent);
    let beta = alpha + 1;

    if parity & QUADRANT_ID[x1] == 0 {
        if parity & QUADRANT_ID[x2] != 0 {
            if parity & QUADRANT_ID[x3] != 0 {
                let tmp = x1;
                x1 = x2;
                x2 = x3;
                x3 = tmp;
            } else {
                let tmp = x1;
                x1 = x2;
                x2 = x4;
                x4 = x3;
                x3 = tmp;
            }
        } else if parity & QUADRANT_ID[x3] != 0 {
            std::mem::swap(&mut x1, &mut x3);
            std::mem::swap(&mut x2, &mut x4);
        }
    } else if parity & QUADRANT_ID[x2] == 0 {
        if parity & QUADRANT_ID[x3] != 0 {
            std::mem::swap(&mut x2, &mut x3);
        } else {
            let tmp = x2;
            x2 = x4;
            x4 = x3;
            x3 = tmp;
        }
    }

    let mut best = -SCORE_MAX - 1;
    for (x, y, z, w) in [
        (x1, x2, x3, x4),
        (x2, x1, x3, x4),
        (x3, x1, x2, x4),
        (x4, x1, x2, x3),
    ] {
        if NEIGHBOUR[x] & opponent == 0 {
            continue;
        }
        let flip = flip_at(player, opponent, x);
        if flip == 0 {
            continue;
        }
        let bit = 1u64 << x;
        let score = -solve_score_3_empties(
            opponent ^ flip,
            player ^ (flip | bit),
            -beta,
            y,
            z,
            w,
            parity ^ QUADRANT_ID[x],
            search,
        );
        if score >= beta {
            return score;
        }
        if score > best {
            best = score;
        }
    }

    if best != -SCORE_MAX - 1 {
        return best;
    }

    // pass後も空きリストとパリティ順を再利用し、相手側の手を直接調べる。
    search.stats.final_search_nodes += 1;
    let mut passed_best = SCORE_MAX + 1;
    for (x, y, z, w) in [
        (x1, x2, x3, x4),
        (x2, x1, x3, x4),
        (x3, x1, x2, x4),
        (x4, x1, x2, x3),
    ] {
        if NEIGHBOUR[x] & player == 0 {
            continue;
        }
        let flip = flip_at(opponent, player, x);
        if flip == 0 {
            continue;
        }
        let bit = 1u64 << x;
        let score = solve_score_3_empties(
            player ^ flip,
            opponent ^ (flip | bit),
            alpha,
            y,
            z,
            w,
            parity ^ QUADRANT_ID[x],
            search,
        );
        if score <= alpha {
            return score;
        }
        if score < passed_best {
            passed_best = score;
        }
    }

    if passed_best != SCORE_MAX + 1 {
        passed_best
    } else {
        search.stats.final_search_leaf_nodes += 1;
        solve_score_bits(player, 4)
    }
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

    fn next_pseudo_random(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
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
            let bit = bits & bits.wrapping_neg();
            bits ^= bit;
            best = best.max(-brute_force(&board.make_move(bit)));
        }
        best
    }

    fn random_board_with_empties(rng: &mut u64, n_empties: u32) -> Board {
        let mut empties = 0u64;
        while empties.count_ones() < n_empties {
            empties |= 1u64 << (next_pseudo_random(rng) % 64);
        }
        let player = next_pseudo_random(rng) & !empties;
        let opponent = !player & !empties;
        Board { player, opponent }
    }

    #[test]
    fn solve_score_3_4_empties_match_bruteforce_random_nws_windows() {
        let (evaluator, mpc, tt) = shared_resources();
        let mut rng = 0x6d77_9a31_b4c2_1085;

        for n_empties in [3u32, 4] {
            for _ in 0..500 {
                let board = random_board_with_empties(&mut rng, n_empties);
                let expected = brute_force(&board);

                for alpha in [expected - 1, expected, expected + 1] {
                    let mut stats = SearchStats::default();
                    let mut search =
                        SearchContext::new(evaluator.clone(), mpc.clone(), tt.clone(), &mut stats);
                    let actual = if n_empties == 3 {
                        let empties = !(board.player | board.opponent);
                        let x1 = empties.trailing_zeros() as usize;
                        let rest = empties & (empties - 1);
                        let x2 = rest.trailing_zeros() as usize;
                        let x3 = (rest & (rest - 1)).trailing_zeros() as usize;
                        solve_score_3_empties(
                            board.player,
                            board.opponent,
                            alpha,
                            x1,
                            x2,
                            x3,
                            final_parity(board.player, board.opponent),
                            &mut search,
                        )
                    } else {
                        solve_score_4_empties(board.player, board.opponent, alpha, &mut search)
                    };

                    assert_eq!(
                        actual, expected,
                        "n_empties={n_empties} alpha={alpha} player={:#018x} opponent={:#018x}",
                        board.player, board.opponent
                    );
                }
            }
        }
    }

    #[test]
    fn solve_score_1_empties_lazy_cutoff_is_fail_soft() {
        let mut rng = 0xa31f_5c09_7782_4bd1;

        for _ in 0..500 {
            let empty = (next_pseudo_random(&mut rng) % 64) as usize;
            let empty_bit = 1u64 << empty;
            let player = next_pseudo_random(&mut rng) & !empty_bit;
            let board = Board {
                player,
                opponent: !player & !empty_bit,
            };
            let expected = brute_force(&board);

            for alpha in -64..=64 {
                let actual = solve_score_1_empties(player, alpha, empty);
                if expected > alpha {
                    assert_eq!(actual, expected, "alpha={alpha} empty={empty}");
                } else {
                    assert!(
                        actual <= alpha,
                        "alpha={alpha} actual={actual} expected={expected}"
                    );
                }
            }
        }
    }
}
