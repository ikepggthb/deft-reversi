//! 終盤探索のための手リスト管理。
//!
//! `MoveBoard` は 1 手分の情報(着手後の盤面・着手位置・move ordering スコア・skip フラグ)
//! を保持する。`set_move_list` で手ビットマスクから一括生成し、
//! `sort_move_list` で eval 降順に並べ替える。

use arrayvec::ArrayVec;

use crate::board::board::Board;

/// オセロの最大合法手数。
/// 参考: https://eukaryote.hateblo.jp/entry/2023/05/17/163629
///       https://github.com/eukaryo/reversi33
pub const MOVE_MAX: usize = 33;

/// 1 手分の情報を保持する構造体。
#[derive(Clone, Copy)]
pub struct MoveBoard {
    /// move ordering スコア(高いほど先に探索)。
    pub score: i32,
    /// 着手位置(0-63)。
    pub move_num: u8,
    /// 着手する際に反転する石(bit)
    pub flip_bit: u64,
}

/// 4 コーナーのビットマスク。FFS ordering の corner penalty に使う。
const CORNER_MASK: u64 = 0x8100_0000_0000_0081;

pub fn set_move_list(board: &Board, moves_bit: u64) -> ArrayVec<MoveBoard, MOVE_MAX> {
    let mut move_list = ArrayVec::<MoveBoard, MOVE_MAX>::new();

    let mut moves = moves_bit;
    while moves != 0 {
        let move_num = moves.trailing_zeros() as u8;
        let flip_bit = board.flip_bit(1u64 << move_num);
        move_list.push(MoveBoard { score: 0, move_num, flip_bit });
        moves &= moves - 1;
    }

    move_list
}

/// fast first search のスコアを `move_list` に付加する。
///
/// 各手の後で相手が置ける手が少ない方が良いとみなし、
/// コーナーへの合法手はさらにペナルティを与える。
#[inline(always)]
pub fn assign_ffs_scores(board: &Board, move_list: &mut [MoveBoard]) {
    for mb in move_list.iter_mut() {
        let opp_moves = board.make_move_from_flip_bit(1u64 << mb.move_num, mb.flip_bit).moves();
        let n_moves = -(opp_moves.count_ones() as i32);
        let n_corners = -((opp_moves & CORNER_MASK).count_ones() as i32);
        mb.score = n_moves * 2 + n_corners;
    }
}


/// `score` 降順に `move_list` を安定でなく部分ソートする。
///
/// 上位 `TOP_N` 手だけを sorted にし、残りは未ソートのまま。
#[inline(always)]
pub fn sort_move_list(move_list: &mut [MoveBoard]) {
    const TOP_N: usize = 7;
    let n = move_list.len();
    if n <= TOP_N {
        move_list.sort_unstable_by_key(|mb| std::cmp::Reverse(mb.score));
    } else {
        move_list.select_nth_unstable_by_key(TOP_N - 1, |mb| std::cmp::Reverse(mb.score));
        move_list[..TOP_N].sort_unstable_by_key(|mb| std::cmp::Reverse(mb.score));
    }
}


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
