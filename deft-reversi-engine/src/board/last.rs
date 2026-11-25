//! 残り 1 マス（最終手）で返る石数の 2 倍値を高速に計算するモジュールです。
//!
//! 4 軸（横・縦・斜め 2 方向）のラインテーブルをコンパイル時に生成しておき、
//! BMI2（x86-64 のビット操作拡張命令セット）が有効な環境では
//! `_pext_u64`（PEXT: マスクで指定したビット位置だけを抜き出して下位に詰める命令）で参照します。
#[cfg(any(test, not(target_feature = "bmi2")))]
use crate::Board;

/// 空きマス `square` に手番側が打ったときに返る相手石数の 2 倍（`2 * flips`）を返します。
///
/// リバーシでは石を 1 個裏返すと「自分の石が +1、相手の石が -1」になるため、
/// 返した石数の 2 倍だけ手番側の石差が増えます。終盤の最終手計算でそのまま加算できるよう 2 倍値を直接返します。
#[inline(always)]
pub(crate) fn doubled_flips(square: usize, player: u64) -> i32 {
    #[cfg(target_feature = "bmi2")]
    {
        bmi2::doubled_flips(square, player)
    }
    #[cfg(not(target_feature = "bmi2"))]
    {
        let empty = 1u64 << square;
        2 * Board {
            player,
            opponent: !player & !empty,
        }
        .flip_mask(empty)
        .count_ones() as i32
    }
}

#[cfg(target_feature = "bmi2")]
mod bmi2 {
    use std::arch::x86_64::_pext_u64;
    /// 8 マスのライン上の位置 `pos` と石パターン `bits` に対する「両側の反転数 × 2」を収めたテーブルです。
    const COUNTS: [[u8; 256]; 8] = counts();
    // 縦・主対角・副対角の 8 ビット抽出マスク（`masks[..3]`）と、実際の 4 軸和集合マスク（`masks[3]`）です。
    // 短い対角線では PEXT 結果の行位置を揃えるため無関係なダミービットを 1 つ立てますが、
    // 先に `player & masks[3]` で和集合マスクに通すことでダミービットは確実に 0 になります。
    const MASKS: [[u64; 4]; 64] = masks();
    /// コンパイル時に `COUNTS[pos][bits]` テーブルを構築します。
    const fn counts() -> [[u8; 256]; 8] {
        let mut out = [[0; 256]; 8];
        let mut pos = 0;
        while pos < 8 {
            let mut bits = 0;
            while bits < 256 {
                let mut total = 0;
                let mut direction = -1i32;
                while direction <= 1 {
                    let mut index = pos as i32 + direction;
                    let mut ray = 0;
                    while index >= 0 && index < 8 {
                        if bits & (1 << index) != 0 {
                            total += ray;
                            break;
                        }
                        ray += 1;
                        index += direction;
                    }
                    direction += 2;
                }
                out[pos][bits] = total * 2;
                bits += 1;
            }
            pos += 1;
        }
        out
    }
    /// 各マスについて、横・縦・両斜めの 4 軸の抽出マスクをコンパイル時に構築します。
    const fn masks() -> [[u64; 4]; 64] {
        let mut out = [[0u64; 4]; 64];
        let mut square = 0;
        while square < 64 {
            let file = square % 8;
            let rank = square / 8;
            let mut other = 0;
            let mut actual = [0u64; 3];
            while other < 64 {
                let f = other % 8;
                let r = other / 8;
                let membership = [
                    f == file,
                    f as i32 - r as i32 == file as i32 - rank as i32,
                    f + r == file + rank,
                ];
                let mut axis = 0;
                while axis < 3 {
                    if membership[axis] {
                        actual[axis] |= 1u64 << other;
                    }
                    axis += 1;
                }
                other += 1;
            }
            let union = actual[0] | actual[1] | actual[2] | (0xffu64 << (rank * 8));
            out[square][3] = union;
            let mut axis = 0;
            while axis < 3 {
                let mut padded = actual[axis];
                let mut r = 0;
                while r < 8 {
                    let row = 0xffu64 << (r * 8);
                    if padded & row == 0 {
                        // 短い斜め線では対角線外の行にビットが存在しないため、
                        // `COUNTS[rank]` の行インデックスと一致させる目的で和集合外のダミーマスを 1 つ補います。
                        // 実際の探索時は先に `player & union` を取るため、ダミービットは常に 0 になります。
                        let available = row & !union;
                        assert!(available != 0);
                        padded |= available.isolate_lowest_one();
                    }
                    r += 1;
                }
                assert!(padded.count_ones() == 8);
                out[square][axis] = padded;
                axis += 1;
            }
            square += 1;
        }
        out
    }
    /// BMI2 の `_pext_u64` 命令を用いて 4 軸の反転数 × 2 を合計します。
    #[inline(always)]
    pub(super) fn doubled_flips(square: usize, player: u64) -> i32 {
        let masks = &MASKS[square];
        let player = player & masks[3];
        let horizontal = &COUNTS[square & 7];
        let vertical = &COUNTS[square >> 3];
        let mut sum = i32::from(horizontal[usize::from((player >> (square & 56)) as u8)]);
        for &mask in &masks[..3] {
            // SAFETY: BMI2 が有効であり、各パディング済みマスクには厳密に 8 ビットが立っています。
            let packed = unsafe { _pext_u64(player, mask) } as u8;
            sum += i32::from(vertical[usize::from(packed)]);
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_full_flip_for_each_last_square() {
        let mut random = 0x89abcdef01234567u64;
        for square in 0..64 {
            for _ in 0..128 {
                crate::test_util::xorshift(&mut random);
                let empty = 1u64 << square;
                let player = random & !empty;
                let board = Board::from_bits(player, !player & !empty).unwrap();
                assert_eq!(
                    doubled_flips(square, player),
                    2 * board.flip_mask(empty).count_ones() as i32
                );
                // 相手石の個数を求める側では `!player` を渡すため、最後の空きマスのビットも 1 になります。
                // そのビットがどちらのラインテーブルの計算にも影響しないことを確認します。
                assert_eq!(
                    doubled_flips(square, player | empty),
                    doubled_flips(square, player)
                );
            }
        }
    }
}
