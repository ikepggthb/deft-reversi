//! Last-empty flip counts. Line tables are generated from the rules at compile time.
#[cfg(any(
    test,
    all(
        not(target_feature = "bmi2"),
        not(all(target_arch = "wasm32", target_feature = "simd128"))
    )
))]
use crate::Board;

#[inline(always)]
pub(crate) fn doubled_flips(square: usize, player: u64) -> i32 {
    #[cfg(target_feature = "bmi2")]
    {
        bmi2::doubled_flips(square, player)
    }
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        ray_last::doubled_flips(square, player)
    }
    #[cfg(all(
        not(target_feature = "bmi2"),
        not(all(target_arch = "wasm32", target_feature = "simd128"))
    ))]
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

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub(crate) mod ray_last {
    const fn rays() -> [[u64; 8]; 64] {
        let dirs: [(i32, i32); 8] = [
            (1, 0),
            (0, 1),
            (1, 1),
            (-1, 1),
            (-1, 0),
            (0, -1),
            (-1, -1),
            (1, -1),
        ];
        let mut result = [[0u64; 8]; 64];
        let mut sq = 0;
        while sq < 64 {
            let mut d = 0;
            while d < 8 {
                let mut x = (sq % 8) as i32 + dirs[d].0;
                let mut y = (sq / 8) as i32 + dirs[d].1;
                while x >= 0 && x < 8 && y >= 0 && y < 8 {
                    result[sq][d] |= 1u64 << (y * 8 + x);
                    x += dirs[d].0;
                    y += dirs[d].1;
                }
                d += 1;
            }
            sq += 1;
        }
        result
    }
    // The first four rays increase in bit index; the other four decrease.
    static RAYS: [[u64; 8]; 64] = rays();
    // The first square that is not an opponent disc stops the run: it
    // brackets the run only when it contains a friendly disc, not an empty.
    #[inline(always)]
    pub(crate) fn near_full_flips(square: usize, player: u64, opponent: u64) -> u64 {
        let rays = &RAYS[square];
        let mut flips = 0u64;
        for &ray in &rays[..4] {
            let first = (ray & !opponent).isolate_lowest_one();
            flips |=
                first.wrapping_sub(1) & ray & 0u64.wrapping_sub(((first & player) != 0) as u64);
        }
        for &ray in &rays[4..] {
            let blockers = ray & !opponent;
            let last = 1u64 << (63 - (blockers | 1).leading_zeros());
            flips |= !(last | last.wrapping_sub(1))
                & ray
                & 0u64.wrapping_sub(((last & player & blockers) != 0) as u64);
        }
        flips
    }
    // With one empty, all other squares are occupied, so the nearest
    // friendly disc brackets the run without checking for another empty.
    #[inline(always)]
    pub(super) fn doubled_flips(square: usize, player: u64) -> i32 {
        let rays = &RAYS[square];
        let mut flips = 0u64;
        for &ray in &rays[..4] {
            let own = player & ray;
            let first = own.isolate_lowest_one();
            flips |= first.wrapping_sub(1) & ray & 0u64.wrapping_sub((first != 0) as u64);
        }
        for &ray in &rays[4..] {
            let own = player & ray;
            let last = 1u64 << (63 - (own | 1).leading_zeros());
            flips |= !(last | last.wrapping_sub(1)) & ray & 0u64.wrapping_sub((own != 0) as u64);
        }
        2 * flips.count_ones() as i32
    }
}

#[cfg(all(test, target_arch = "wasm32", target_feature = "simd128"))]
mod ray_last_tests {
    use wasm_bindgen_test::wasm_bindgen_test;
    #[wasm_bindgen_test]
    fn rays_match_generic_multi_empty() {
        let mut seed = 0xabc3_1739_b4c5_d697u64;
        for _ in 0..100_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let player = seed;
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let opponent = seed & !player;
            let mut empties = !(player | opponent);
            while empties != 0 {
                let square = empties.trailing_zeros() as usize;
                let bit = 1u64 << square;
                empties &= !bit;
                let board = crate::Board::from_bits(player, opponent).unwrap();
                assert_eq!(
                    super::ray_last::near_full_flips(square, player, opponent),
                    board.flip_mask(bit)
                );
            }
        }
    }
    #[wasm_bindgen_test]
    fn rays_match_generic_last_square() {
        let mut seed = 0xabc3_1739_b4c5_d697u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            for square in 0..64 {
                let empty = 1u64 << square;
                for player in [seed & !empty, seed | empty] {
                    let board = crate::Board::from_bits(player, !player & !empty).unwrap();
                    assert_eq!(
                        super::ray_last::doubled_flips(square, player),
                        2 * board.flip_mask(empty).count_ones() as i32
                    );
                }
            }
        }
    }
}

#[cfg(target_feature = "bmi2")]
mod bmi2 {
    use std::arch::x86_64::_pext_u64;
    const COUNTS: [[u8; 256]; 8] = counts();
    // Three full-height extraction masks plus the actual four-axis union.
    // Dummy bits outside a short diagonal are cleared by the union first.
    const MASKS: [[u64; 4]; 64] = masks();
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
                        // This is outside the physical diagonal. Select one
                        // unrelated square in that row; the union clears it.
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
    #[inline(always)]
    pub(super) fn doubled_flips(square: usize, player: u64) -> i32 {
        let masks = &MASKS[square];
        let player = player & masks[3];
        let horizontal = &COUNTS[square & 7];
        let vertical = &COUNTS[square >> 3];
        let mut sum = i32::from(horizontal[usize::from((player >> (square & 56)) as u8)]);
        for &mask in &masks[..3] {
            // SAFETY: BMI2 is enabled; every padded mask has exactly eight bits.
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
                // The opponent-count path uses !player, which sets the last
                // square's bit. That bit must not affect either line table.
                assert_eq!(
                    doubled_flips(square, player | empty),
                    doubled_flips(square, player)
                );
            }
        }
    }
}
