
use crate::board::Board;
use std::cmp;

use crate::eval_search::negaalpha_eval;
use crate::evaluator_const::SCORE_MAX;
use crate::t_table::N_TT_MOVES;
use crate::{board::*, simplest_eval};

use crate::solver::SearchEngine;

const SCORE_INF: i32 = i8::MAX as i32;


pub struct MoveIterator {
    bits: u64, // 対象となるビット列
}

impl MoveIterator {
    pub fn new(bits: u64) -> Self {
        Self { bits }
    }
}

impl Iterator for MoveIterator {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.bits == 0 {
            None
        } else {
            let lsb = self.bits & (!self.bits + 1); // 最下位の1ビットを取り出す
            self.bits &= self.bits - 1; // 取り出したビットを除去
            Some(lsb)
        }
    }
}

/// 盤面を4×4に分割し、さらにコーナーを優先するMove Ordering
pub struct MoveIteratorParity {
    corner_odd: u64,
    corner_even: u64,
    odd: u64,
    even: u64,
}

impl MoveIteratorParity {
    /// オセロの四隅だけを抜き出すマスク
    /// (0b1000000100000000000000000000000000000000000000000000000010000001)
    const CORNER_MASK: u64 = 0x8100_0000_0000_0081;

    /// 4つの小盤面（左下・右下・左上・右上）に分割するマスク
    const QUADRANT_MASKS: [u64; 4] = [
        0x0000_0000_0f0f_0f0f,
        0x0000_0000_f0f0_f0f0,
        0xf0f0_f0f0_0000_0000,
        0x0f0f_0f0f_0000_0000,
    ];

    pub fn new(legal_moves: u64, board: &Board) -> Self {
        let empties = !(board.player | board.opponent);

        let corner_move = legal_moves & Self::CORNER_MASK;
        let other_move = legal_moves & !Self::CORNER_MASK;

        // 4つの小盤面をそれぞれ見て「空きマスが奇数ならodd, 偶数ならeven」に振り分ける。
        let mut corner_odd = 0u64;
        let mut corner_even = 0u64;
        let mut odd = 0u64;
        let mut even = 0u64;

        for &mask in Self::QUADRANT_MASKS.iter() {
            if legal_moves & mask == 0 {
                continue; // この小盤面に合法手がなければスキップ
            }

            let empty_count = (empties & mask).count_ones() as u64;
            if empty_count % 2 == 1 {
                corner_odd |= corner_move & mask;
                odd |= other_move & mask;
            } else {
                corner_even |= corner_move & mask;
                even |= other_move & mask;
            }
        }

        Self {
            corner_odd,
            corner_even,
            odd,
            even,
        }
    }

    pub fn divide(legal_moves: u64, board: &Board) -> (u64, u64) {
        let empties = !(board.player | board.opponent);

        // 4つの小盤面をそれぞれ見て「空きマスが奇数ならodd, 偶数ならeven」に振り分ける。
        let mut odd = 0u64;
        let mut even = 0u64;

        for &mask in Self::QUADRANT_MASKS.iter() {
            if legal_moves & mask == 0 {
                continue; // この小盤面に合法手がなければスキップ
            }

            let empty_count = (empties & mask).count_ones() as u64;
            if empty_count % 2 == 1 {
                odd |= legal_moves & mask;
            } else {
                even |= legal_moves & mask;
            }
        }

        (odd, even)
    }
}

impl Iterator for MoveIteratorParity {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        // 1. corner_odd
        if self.corner_odd != 0 {
            let lsb = self.corner_odd & (!self.corner_odd + 1);
            self.corner_odd &= self.corner_odd - 1;
            return Some(lsb);
        }
        // 2. odd
        if self.odd != 0 {
            let lsb = self.odd & (!self.odd + 1);
            self.odd &= self.odd - 1;
            return Some(lsb);
        }
        // 3. corner_even
        if self.corner_even != 0 {
            let lsb = self.corner_even & (!self.corner_even + 1);
            self.corner_even &= self.corner_even - 1;
            return Some(lsb);
        }
        // 4. even
        if self.even != 0 {
            let lsb = self.even & (!self.even + 1);
            self.even &= self.even - 1;
            return Some(lsb);
        }
        None
    }
}

pub struct MoveBoard {
    pub eval: i32,
    pub board: Board,
    pub put_place: u8,
    pub skip: bool,
}

// https://eukaryote.hateblo.jp/entry/2023/05/17/163629
// オセロの最大分岐数は33
pub const MOVE_MAX: usize = 33;

#[inline(always)]
pub fn set_move_list(board: &Board, moves_bit: u64, moves_list: &mut [MoveBoard]) {
    for (i, move_bit) in MoveIterator::new(moves_bit).enumerate() {
        let mut b = board.clone();
        b.put_piece_fast(move_bit);
        moves_list[i] = MoveBoard {
            eval: 0,
            board: b,
            put_place: move_bit.trailing_zeros() as u8,
            skip: false,
        };
    }
}

pub fn get_move_list(board: &Board, moves_bit: u64) -> Vec<MoveBoard>{
    let mut move_list = Vec::with_capacity(moves_bit as usize);
    for move_bit in MoveIterator::new(moves_bit) {
        let mut b = board.clone();
        b.put_piece_fast(move_bit);
        move_list.push(MoveBoard {
            eval: 0,
            board: b,
            put_place: move_bit.trailing_zeros() as u8,
            skip: false,
        });
    }
    move_list
}

#[inline(always)]
pub fn gen_tt_move_list(board: &Board, tt_move: &[u8]) -> [MoveBoard; N_TT_MOVES] {
    // N_TT_MOVE = 2
    let moves_list_0 = if tt_move[0] != NO_COORD {
        let move_bit = 1u64 << tt_move[0];
        let mut b = board.clone();
        b.put_piece_fast(move_bit);
        MoveBoard {
            eval: 0,
            board: b,
            put_place: tt_move[0],
            skip: false,
        }
    } else {
        MoveBoard {
            eval: 0,
            board: Board {
                // dummy
                player: 0,
                opponent: 0,
            },
            put_place: 0,
            skip: true,
        }
    };

    let moves_list_1 = if tt_move[1] != NO_COORD {
        let move_bit = 1u64 << tt_move[1];
        let mut b = board.clone();
        b.put_piece_fast(move_bit);
        MoveBoard {
            eval: 0,
            board: b,
            put_place: tt_move[1],
            skip: false,
        }
    } else {
        MoveBoard {
            eval: 0,
            board: Board {
                // dummy
                player: 0,
                opponent: 0,
            },
            put_place: 0,
            skip: true,
        }
    };

    [moves_list_0, moves_list_1]
}

#[inline(always)]
pub fn set_move_eval(move_list: &mut [MoveBoard], lv: i32, alpha: i32, search: &mut SearchEngine) {
    for move_board in move_list.iter_mut() {
        if move_board.skip {
            continue;
        }
        let search_eval = if lv < 1 {
            -simplest_eval(&move_board.board)
        } else {
            -negaalpha_eval(&move_board.board, cmp::max(-alpha - 6, -SCORE_MAX), cmp::min(-alpha + 16, SCORE_MAX), lv - 1, search)
        };

        let opponent_mobility_score = {
            let moves_bit = move_board.board.moves();
            let n_move = -(moves_bit.count_ones() as i32);
            let n_coner_moves = -((moves_bit & MC).count_ones() as i32);
            // let emc = 40 - (em + ec);
            n_move * 2 + n_coner_moves
        };
        move_board.eval += search_eval + opponent_mobility_score;
    }
}

const MC: u64 = 0b1000000100000000000000000000000000000000000000000000000010000001_u64;
const MX: u64 = 0b0000000001000010000000000000000000000000000000000100001000000000_u64;

#[inline(always)]
pub fn set_move_eval_ffs(move_list: &mut [MoveBoard]) {
    for move_board in move_list.iter_mut() {
        if move_board.skip {
            continue;
        }

        let moves_bit = move_board.board.moves();
        let em = -(moves_bit.count_ones() as i32);
        let ec = -((moves_bit & MC).count_ones() as i32);
        move_board.eval = em + ec; //(2 * ec - ex);
    }
}


#[inline(always)]
pub fn sort_move_list(move_list: &mut [MoveBoard]) {
    const TOP_N: usize = 7;
    let n = move_list.len();

    if n <= TOP_N {
        move_list.sort_unstable_by(|a, b| b.eval.partial_cmp(&a.eval).unwrap());
    } else {
        move_list.select_nth_unstable_by(TOP_N - 1, |a, b| b.eval.partial_cmp(&a.eval).unwrap());
        move_list[..TOP_N].sort_unstable_by(|a, b| b.eval.partial_cmp(&a.eval).unwrap());
    }
}
