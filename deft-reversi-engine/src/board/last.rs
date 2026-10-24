//! Last-empty flip counts. Line tables are generated from the rules at compile time.
#[cfg(any(test, not(target_feature = "bmi2")))]
use crate::Board;

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
    const COUNTS: [[u8; 256]; 8] = counts();
    const LINES: [[Line; 3]; 64] = lines();
    #[derive(Clone, Copy)]
    struct Line {
        mask: u64,
        position: usize,
    }
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
    const fn lines() -> [[Line; 3]; 64] {
        let mut out = [[Line {
            mask: 0,
            position: 0,
        }; 3]; 64];
        let mut square = 0;
        while square < 64 {
            let file = square % 8;
            let rank = square / 8;
            let mut other = 0;
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
                        out[square][axis].mask |= 1u64 << other;
                        if other < square {
                            out[square][axis].position += 1;
                        }
                    }
                    axis += 1;
                }
                other += 1;
            }
            square += 1;
        }
        out
    }
    #[inline(always)]
    pub(super) fn doubled_flips(square: usize, player: u64) -> i32 {
        let mut sum = i32::from(COUNTS[square % 8][((player >> (square & 56)) & 255) as usize]);
        for line in &LINES[square] {
            // SAFETY: the complete module is compiled only with BMI2 enabled.
            let packed = unsafe { _pext_u64(player, line.mask) } as usize;
            sum += i32::from(COUNTS[line.position][packed]);
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
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                let empty = 1u64 << square;
                let player = random & !empty;
                let board = Board::from_bits(player, !player & !empty).unwrap();
                assert_eq!(
                    doubled_flips(square, player),
                    2 * board.flip_mask(empty).count_ones() as i32
                );
            }
        }
    }
}
