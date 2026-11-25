//! 空き 1〜4 マス専用の Null-Window（幅 1 の探索窓）終盤ソルバカーネルです。
//! 中断要求やノード上限はラッパーの [`LeafSearch`] 経由で伝播し、境界値が公開される前に確実に中断します。
//!
//! ## 出典
//!
//! 空き 1〜4 マスのカーネル、象限パリティとマス価値（`SQUARE_VALUE`）による並べ替え、`NEIGHBOUR` 表は、[Edax](https://github.com/abulmo/edax-reversi)（Richard Delorme・奥原俊彦、GPL-3.0）の `endgame.c`・`move.c` を移植した旧 Deft の実装を引き継いだものです。
use super::parity::QUADRANT_ID;
use super::*;
const SCORE_MAX: i32 = 64;
// 有効な 4 象限パリティ（4×4 の各領域の空きマス数が奇数か偶数かを表す値、0..16）は 4 ビットに収まるため、
// 16 を「未計算」のセンチネル値とします。
const UNKNOWN_PARITY: u8 = 16;
/// 空き 2〜4 マスカーネル内でノード予算・中断フラグをチェックするためのアダプタです。
struct LeafSearch<'a> {
    context: &'a mut Context,
    failed: bool,
}
impl LeafSearch<'_> {
    /// ノード訪問を計上し、中断条件に達した場合は `failed` フラグを立てます。
    fn visit(&mut self) {
        if !self.failed && self.context.enter().is_err() {
            self.failed = true;
        }
    }
}
impl Context {
    /// パリティ未計算の状態で空き 4 マスカーネルを呼び出します。
    pub(super) fn last_four_score(&mut self, board: Board, alpha: i32) -> Result<i32, Aborted> {
        self.last_four_score_with_parity(board, alpha, UNKNOWN_PARITY)
    }
    /// 引き回した象限パリティ `parity` を用いて空き 4 マスカーネルを実行します。
    pub(super) fn last_four_score_with_parity(
        &mut self,
        board: Board,
        alpha: i32,
        parity: u8,
    ) -> Result<i32, Aborted> {
        debug_assert!(parity <= UNKNOWN_PARITY);
        let mut search = LeafSearch {
            context: self,
            failed: false,
        };
        let score = solve_score_4_empties(
            board.player_bits(),
            board.opponent_bits(),
            alpha,
            parity,
            &mut search,
        );
        if search.failed {
            Err(Aborted)
        } else {
            Ok(score)
        }
    }
}
/// 空き 1 マスの最終局面を Null-Window（`[alpha, alpha + 1]`）で解きます。
#[inline(always)]
fn solve_score_1_empties(player: u64, alpha: i32, pos: usize) -> i32 {
    debug_assert!(pos < 64);

    let n_flips = crate::board::last::doubled_flips(pos, player);
    let player_count_x2 = 2 * player.count_ones() as i32;

    if n_flips != 0 {
        // プレイヤーが pos に着手し、n_flips/2 枚を裏返して終局します。
        return player_count_x2 - 64 + 2 + n_flips;
    }

    let mut score = player_count_x2 - 64 + 2;
    if score <= 0 {
        score -= 2;
        if score > alpha {
            score -= crate::board::last::doubled_flips(pos, !player);
        }
        return score;
    }

    if score > alpha {
        let opponent_flips = crate::board::last::doubled_flips(pos, !player);
        if opponent_flips != 0 {
            score -= opponent_flips + 2;
            return score;
        }
    }

    // どちらも置けないため空きマスは未確定です。`solve_score` の慣習に揃え、
    // 勝っている側に空きマス分を加算します。63 マス埋まっているので diff は奇数で 0 にはなりません。
    let diff = player_count_x2 - 63;
    if diff > 0 { diff + 1 } else { diff - 1 }
}

/// `pos` の周囲 1 マス（8 近傍）を立てたビットマスクです。
///
/// 旧実装からの定数表です。空きマスの周囲に相手の石がなければ着手しても 1 枚も
/// 裏返せないため、`flip_bit` の計算をスキップする早期判定に使います。
///
/// 参考: <https://eukaryote.hateblo.jp/entry/2020/04/26/031246>
#[rustfmt::skip]
pub(super) const NEIGHBOUR: [u64; 64] = [
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
    Board { player, opponent }.flip_mask(1u64 << pos)
}

/// 与えられた手番石数と残り空きマス数 `n_empties` から、両者パス終局時の石差スコアを計算します。
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

/// 盤面上の空きマスから 4 象限パリティを直接計算します（`UNKNOWN_PARITY` 時のフォールバックです）。
#[inline(always)]
fn final_parity(player: u64, opponent: u64) -> i32 {
    let mut empties = !(player | opponent);
    let mut parity = 0;
    while empties != 0 {
        let pos = empties.trailing_zeros() as usize;
        empties &= empties - 1;
        parity ^= QUADRANT_ID[pos];
    }
    parity
}

/// 空きマスが 2 マスの盤面を NegaAlpha 1 段で解きます。
///
/// 各候補手の後で `solve_score_1_empties` を呼ぶことで完全に解けます。
/// パスが必要な場合は相手側の 2 マスを直接調べます（2 連続パスで終局）。
fn solve_score_2_empties(
    player: u64,
    opponent: u64,
    alpha: i32,
    x1: usize,
    x2: usize,
    search: &mut LeafSearch<'_>,
) -> i32 {
    debug_assert_eq!((player | opponent).count_zeros(), 2);

    search.visit();

    let beta = alpha + 1;
    let mut best = -SCORE_MAX - 1;

    if NEIGHBOUR[x1] & opponent != 0 {
        let flip = flip_at(player, opponent, x1);
        if flip != 0 {
            search.visit();
            search.context.stats.leaves += 1;
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
            search.visit();
            search.context.stats.leaves += 1;
            let score = -solve_score_1_empties(opponent ^ flip, -beta, x1);
            if score > best {
                best = score;
            }
        }
    }

    if best != -SCORE_MAX - 1 {
        return best;
    }

    // パス後も残り 2 マスだけを直接調べます。汎用の合法手生成と再帰は不要です。
    search.visit();
    let mut passed_best = SCORE_MAX + 1;

    if NEIGHBOUR[x1] & player != 0 {
        let flip = flip_at(opponent, player, x1);
        if flip != 0 {
            search.visit();
            search.context.stats.leaves += 1;
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
            search.visit();
            search.context.stats.leaves += 1;
            let score = solve_score_1_empties(player ^ flip, alpha, x1);
            if score < passed_best {
                passed_best = score;
            }
        }
    }

    if passed_best != SCORE_MAX + 1 {
        passed_best
    } else {
        search.context.stats.leaves += 1;
        solve_score_bits(player, 2)
    }
}

// Edax のカーネルとアルゴリズムを直接比べられるよう、スカラー引数をそのまま維持しています。
/// 空きマスが 3 マスの盤面を、象限パリティとマス価値で並べ替えて解きます。
#[allow(clippy::too_many_arguments)]
fn solve_score_3_empties(
    player: u64,
    opponent: u64,
    _alpha: i32,
    _x1: usize,
    _x2: usize,
    _x3: usize,
    _parity: i32,
    search: &mut LeafSearch<'_>,
) -> i32 {
    search.visit();
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
            std::mem::swap(&mut x2, &mut x3);
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
    if let Some(score) = solve3_move(player, opponent, alpha, x3, x1, x2, search)
        && score > best
    {
        best = score;
    }

    if best != -SCORE_MAX - 1 {
        return best;
    }

    // パス後は、整列済みの 3 マスを相手側から直接調べます。
    search.visit();
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
        search.context.stats.leaves += 1;
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
    search: &mut LeafSearch<'_>,
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

/// 空きマスが 4 マスの盤面を、確定石カットと象限パリティソート付きで解きます。
fn solve_score_4_empties(
    player: u64,
    opponent: u64,
    alpha: i32,
    known_parity: u8,
    search: &mut LeafSearch<'_>,
) -> i32 {
    // この 4 マスノードの `enter()` は呼び出し元で既に完了しています。

    let board = Board { player, opponent };
    if let Some(score) = crate::board::stability::null_window_upper(board, alpha) {
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
    let parity = if known_parity < UNKNOWN_PARITY {
        i32::from(known_parity)
    } else {
        final_parity(player, opponent)
    };
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

    // パス後も空きリストとパリティ順を再利用し、相手側の手を直接調べます。
    search.visit();
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
        search.context.stats.leaves += 1;
        solve_score_bits(player, 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_one_bounds_match_full_board_rules() {
        let mut random = 0x0123456789abcdefu64;
        for square in 0..64 {
            let empty = 1u64 << square;
            for _ in 0..128 {
                crate::test_util::xorshift(&mut random);
                let player = random & !empty;
                let board = Board::from_bits(player, !player & !empty).unwrap();
                let expected = if board.get_moves() != 0 {
                    -board.board_after_move(empty).final_score()
                } else if board.get_opponent_moves() != 0 {
                    board
                        .board_after_pass()
                        .board_after_move(empty)
                        .final_score()
                } else {
                    board.final_score()
                };
                let base = 2 * player.count_ones() as i32 - 62;
                for alpha in [
                    -65,
                    -31,
                    -1,
                    0,
                    31,
                    63,
                    64,
                    base - 1,
                    base,
                    expected - 1,
                    expected,
                ] {
                    let actual = solve_score_1_empties(player, alpha, square);
                    if expected <= alpha {
                        assert!(
                            expected <= actual && actual <= alpha,
                            "upper: board={board:?} alpha={alpha} actual={actual} expected={expected}"
                        );
                    } else {
                        assert!(
                            alpha < actual && actual <= expected,
                            "lower: board={board:?} alpha={alpha} actual={actual} expected={expected}"
                        );
                    }
                }
            }
        }
    }
}
