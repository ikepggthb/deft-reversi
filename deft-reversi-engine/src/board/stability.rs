//! 安定石（確定石：今後ゲーム終了まで絶対に裏返らない石）を判定するモジュールです。
//!
//! 辺テーブルから得られる辺上の確定石と、4 軸（横・縦・斜め 2 方向）がすべて埋まったライン上の石を起点として、
//! 隣接する確定石から内側へ確定石を広げていく（不動点伝播）ことで、確定石の個数を求めます。
use crate::Board;

/// Null-Window Search（探索窓の幅を 1 に絞って「スコアが `alpha` を超えるか」だけを判定する高速探索）向けの
/// 高速な上界枝刈り判定です。
///
/// 確定石の計算は比較的コストが高いため、空きマス数に応じた閾値 `THRESHOLD` を満たし、かつ
/// 「仮に相手石がすべて確定石だったとしても最終スコアが `alpha` 以下に収まる可能性があるか」という
/// 事前チェックを通過した fail-low（スコアが `alpha` 以下に確定しそうな局面）近傍でのみ呼び出します。
#[inline(always)]
pub(crate) fn null_window_upper(board: Board, alpha: i32) -> Option<i32> {
    const THRESHOLD: [i32; 64] = [
        99, 99, 99, 99, 6, 8, 10, 12, 14, 16, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38, 40, 42, 44,
        46, 48, 48, 50, 50, 52, 52, 54, 54, 56, 56, 58, 58, 60, 60, 62, 62, 64, 64, 64, 64, 64, 64,
        64, 64, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    ];
    // 相手の確定石は相手石の部分集合であるため、相手石がすべて確定石だと仮定しても上界が alpha を超えるなら、
    // 確定石を計算しても枝刈りできないので即座に諦めます。
    if alpha < THRESHOLD[usize::from(board.empty_count())]
        || 64 - 2 * board.opponent_bits().count_ones() as i32 > alpha
    {
        return None;
    }
    stability_upper(board, alpha)
}
/// 相手の確定石数が「スコアの上界が `alpha` 以下である（fail-low）」と証明するのに足りるかを実際に計算します。
#[inline(never)]
fn stability_upper(board: Board, alpha: i32) -> Option<i32> {
    let full = full_axes(board.player_bits() | board.opponent_bits());
    // `64 - 2 * stable <= alpha` を満たすために必要な相手確定石数の最小値です。
    let needed = (65 - alpha).div_euclid(2) as u32;
    let count = stable(board.opponent_bits(), board.player_bits(), &full, needed).count_ones();
    (count >= needed).then_some(64 - 2 * count as i32)
}
/// テスト用：手番側・相手側双方の確定石数から導かれるスコアの下界と上界のペアを返します。
#[cfg(test)]
pub(crate) fn score_bounds(board: Board) -> (i32, i32) {
    let full = full_axes(board.player_bits() | board.opponent_bits());
    let player = stable(board.player_bits(), board.opponent_bits(), &full, 65).count_ones() as i32;
    let opponent =
        stable(board.opponent_bits(), board.player_bits(), &full, 65).count_ones() as i32;
    (2 * player - 64, 64 - 2 * opponent)
}
/// `bits` 側の確定石マスクを求めます。確定石数が目標の `enough` に達した時点で計算を打ち切ります。
fn stable(bits: u64, opponent: u64, full: &[u64; 4], enough: u32) -> u64 {
    let mut result = super::edge_stability::get_stable_edge(bits, opponent);
    let central = bits & 0x007e7e7e7e7e7e00;
    // 4 軸すべてが埋まっている交点にある中央マスは、どの方向からも挟めないため即座に確定石になります。
    result |= full[0] & full[1] & full[2] & full[3] & central;
    while result.count_ones() < enough {
        // 各方向について「隣接マスが確定石」または「その軸のラインがすべて埋まっている」なら、
        // その方向からは挟まれません。4 軸すべてで挟まれないマスを確定石へ追加していきます。
        let horizontal = (result >> 1) | (result << 1) | full[0];
        let vertical = (result >> 8) | (result << 8) | full[1];
        let diagonal7 = (result >> 7) | (result << 7) | full[2];
        let diagonal9 = (result >> 9) | (result << 9) | full[3];
        let next = result | (horizontal & vertical & diagonal7 & diagonal9 & central);
        if next == result {
            return result;
        }
        result = next;
    }
    result
}
/// 全石配置 `disc` から、4 軸（横・縦・斜め 7・斜め 9）ごとに「その軸のラインがすべて埋まっている」マスを表すビットマスクを返します。
fn full_axes(disc: u64) -> [u64; 4] {
    let mut horizontal = disc;
    horizontal &= horizontal >> 1;
    horizontal &= horizontal >> 2;
    horizontal &= horizontal >> 4;
    let mut vertical = disc;
    vertical &= vertical.rotate_right(8);
    vertical &= vertical.rotate_right(16);
    vertical &= vertical.rotate_right(32);
    let (mut left7, mut right7, mut left9, mut right9) = (disc, disc, disc, disc);
    left7 &= 0xff01010101010101 | (left7 >> 7);
    right7 &= 0x80808080808080ff | (right7 << 7);
    left7 &= 0xffff030303030303 | (left7 >> 14);
    right7 &= 0xc0c0c0c0c0c0ffff | (right7 << 14);
    left7 &= 0xffffffff0f0f0f0f | (left7 >> 28);
    right7 &= 0xf0f0f0f0ffffffff | (right7 << 28);
    left9 &= 0xff80808080808080 | (left9 >> 9);
    right9 &= 0x01010101010101ff | (right9 << 9);
    left9 &= 0xffffc0c0c0c0c0c0 | (left9 >> 18);
    right9 &= 0x030303030303ffff | (right9 << 18);
    [
        (horizontal & 0x0101010101010101) * 0xff,
        vertical,
        left7 & right7,
        left9 & right9 & (0x0f0f0f0ff0f0f0f0 | (left9 >> 36) | (right9 << 36)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn brute(board: Board) -> i32 {
        let mut moves = board.get_moves();
        if moves == 0 {
            return if board.is_terminal() {
                board.final_score()
            } else {
                -brute(board.board_after_pass())
            };
        }
        let mut best = -65;
        while moves != 0 {
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            best = best.max(-brute(board.board_after_move(bit)));
        }
        best
    }
    #[test]
    fn null_window_upper_is_a_sound_fail_low_bound() {
        let mut random = 0x0fedcba987654321u64;
        let mut cuts = 0;
        for _ in 0..512 {
            let mut empty = 0u64;
            while empty.count_ones() < 5 {
                crate::test_util::xorshift(&mut random);
                empty |= 1u64 << (random % 64);
            }
            crate::test_util::xorshift(&mut random);
            let player = random & random.rotate_left(17) & !empty;
            let board = Board::from_bits(player, !player & !empty).unwrap();
            let expected = brute(board);
            let (_, full_upper) = score_bounds(board);
            for alpha in -64..64 {
                if let Some(upper) = null_window_upper(board, alpha) {
                    cuts += 1;
                    assert!(expected <= upper && upper <= alpha && full_upper <= upper);
                }
            }
        }
        assert!(cuts > 0);
    }
    #[test]
    fn stability_bounds_contain_exhaustive_result() {
        let mut random = 0x12345678abcdef00u64;
        for _ in 0..128 {
            let mut empty = 0u64;
            while empty.count_ones() < 4 {
                crate::test_util::xorshift(&mut random);
                empty |= 1u64 << (random % 64);
            }
            crate::test_util::xorshift(&mut random);
            let player = random & !empty;
            let board = Board::from_bits(player, !player & !empty).unwrap();
            let (lower, upper) = score_bounds(board);
            let expected = brute(board);
            assert!(
                lower <= expected && expected <= upper,
                "lower={lower} score={expected} upper={upper} board={board:?}"
            );
        }
    }
}
