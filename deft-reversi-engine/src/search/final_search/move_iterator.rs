//! 終盤探索向けの move iterator。
//!
//! - [`MoveIterator`] は単に bitset の立ったビットを最下位から順に列挙する。
//! - [`MoveIteratorParity`] は盤面を 4 つの 4×4 象限に分け、
//!   各象限の空きマスのパリティ(奇/偶)とコーナーを優先キーにして並べ替える。
//!   旧実装と同じ順序: corner_odd → odd → corner_even → even。
//!
//! parity による move ordering は完全読みでカット率を大きく上げるため、
//! 終盤の NegaAlpha では `MoveIteratorParity` を使う。

use crate::board::board::Board;

/// 立っているビットを最下位から順に取り出すだけの iterator。
pub struct MoveIterator {
    bits: u64,
}

impl MoveIterator {
    #[inline(always)]
    pub fn new(bits: u64) -> Self {
        Self { bits }
    }
}

impl Iterator for MoveIterator {
    type Item = u64;

    #[inline(always)]
    fn next(&mut self) -> Option<u64> {
        if self.bits == 0 {
            None
        } else {
            let lsb = self.bits & self.bits.wrapping_neg();
            self.bits &= self.bits - 1;
            Some(lsb)
        }
    }
}

/// 4 つの 4×4 象限のパリティ + コーナーで move ordering する iterator。
///
/// 列挙順:
/// 1. 奇数パリティ象限のコーナー
/// 2. 奇数パリティ象限の非コーナー
/// 3. 偶数パリティ象限のコーナー
/// 4. 偶数パリティ象限の非コーナー
pub struct MoveIteratorParity {
    corner_odd: u64,
    odd: u64,
    corner_even: u64,
    even: u64,
}

const CORNER_MASK: u64 = 0x8100_0000_0000_0081;

const QUADRANT_MASKS: [u64; 4] = [
    0x0000_0000_0f0f_0f0f,
    0x0000_0000_f0f0_f0f0,
    0xf0f0_f0f0_0000_0000,
    0x0f0f_0f0f_0000_0000,
];

impl MoveIteratorParity {
    pub fn new(legal_moves: u64, board: &Board) -> Self {
        let empties = !(board.player | board.opponent);

        let corner_moves = legal_moves & CORNER_MASK;
        let other_moves = legal_moves & !CORNER_MASK;

        let mut corner_odd = 0u64;
        let mut odd = 0u64;
        let mut corner_even = 0u64;
        let mut even = 0u64;

        for &mask in &QUADRANT_MASKS {
            if legal_moves & mask == 0 {
                continue;
            }
            if (empties & mask).count_ones() & 1 == 1 {
                corner_odd |= corner_moves & mask;
                odd |= other_moves & mask;
            } else {
                corner_even |= corner_moves & mask;
                even |= other_moves & mask;
            }
        }

        Self {
            corner_odd,
            odd,
            corner_even,
            even,
        }
    }
}

impl Iterator for MoveIteratorParity {
    type Item = u64;

    #[inline(always)]
    fn next(&mut self) -> Option<u64> {
        for bucket in [
            &mut self.corner_odd,
            &mut self.odd,
            &mut self.corner_even,
            &mut self.even,
        ] {
            if *bucket != 0 {
                let lsb = *bucket & bucket.wrapping_neg();
                *bucket &= *bucket - 1;
                return Some(lsb);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_iterator_yields_each_set_bit_once_lsb_first() {
        let bits = 0b1010_0110u64;
        let collected: Vec<u64> = MoveIterator::new(bits).collect();
        assert_eq!(
            collected,
            vec![1u64 << 1, 1u64 << 2, 1u64 << 5, 1u64 << 7]
        );
        assert_eq!(MoveIterator::new(bits).count(), 4);
    }

    #[test]
    fn move_iterator_parity_yields_all_legal_moves() {
        let board = Board::new();
        let legal = board.moves();
        let mut yielded = 0u64;
        let mut count = 0;
        for bit in MoveIteratorParity::new(legal, &board) {
            assert!(bit.count_ones() == 1);
            assert!(yielded & bit == 0, "yielded same bit twice");
            yielded |= bit;
            count += 1;
        }
        assert_eq!(yielded, legal);
        assert_eq!(count as u32, legal.count_ones());
    }

    /// corner_odd が odd より先、odd が corner_even/even より先に出ることを確認する。
    #[test]
    fn move_iterator_parity_orders_corner_odd_before_other_odd_before_even() {
        let player = 1u64 << 27; // d4
        let opponent = 1u64 << 28; // e4
        let board = Board { player, opponent };

        // a1 (corner) と b2 (non-corner) は同じ象限 (左上から見れば左下)。
        // この象限の空きマス数のパリティを見て先 / 後 が決まる。
        let legal = (1u64 << 0) | (1u64 << 9); // a1, b2

        let order: Vec<u32> = MoveIteratorParity::new(legal, &board)
            .map(|b| b.trailing_zeros())
            .collect();

        // corner (a1) が non-corner (b2) より先に来るべき(同じ象限内、同じパリティバケット内ではコーナー優先)
        assert_eq!(order, vec![0, 9]);
    }
}
