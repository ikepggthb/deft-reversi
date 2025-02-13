use crate::eval::Evaluator;
use crate::eval_search::negaalpha_eval;
use crate::mpc::NO_MPC;
use crate::t_table::TranspositionTable;
use crate::t_table::N_TT_MOVES;
use crate::{board::*, simplest_eval, TableData};

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

pub struct PutBoard {
    pub board: Board,
    pub put_place: u8,
}

#[inline(always)]
pub fn get_put_boards(board: &Board, legal_moves: u64) -> Vec<PutBoard> {
    let mut put_boards: Vec<PutBoard> = Vec::with_capacity(legal_moves.count_ones() as usize);
    for pos in MoveIterator::new(legal_moves) {
        let mut board = board.clone();
        board.put_piece_fast(pos);
        put_boards.push(PutBoard {
            board,
            put_place: pos.trailing_zeros() as u8,
        })
    }
    put_boards
}

#[inline(always)]
pub fn t_table_cut_off_td(
    alpha: &mut i32,
    beta: &mut i32,
    lv: i32,
    selectivity_lv: i32,
    table_data: &Option<&TableData>,
) -> Option<i32> {
    if let Some(t) = table_data {
        if t.lv as i32 != lv || t.selectivity_lv as i32 != selectivity_lv {
            return None;
        }
        let max = t.max as i32;
        let min = t.min as i32;
        if max <= *alpha {
            return Some(max);
        } else if min >= *beta {
            return Some(min);
        } else if max == min {
            return Some(max);
        }
        if min > *alpha {
            *alpha = min
        };
        if max < *beta {
            *beta = max
        };
    }
    None
}

#[inline(always)]
pub fn t_table_cut_off(
    board: &Board,
    alpha: &mut i32,
    beta: &mut i32,
    lv: i32,
    selectivity_lv: i32,
    t_table: &TranspositionTable,
) -> Option<i32> {
    if let Some(t) = t_table.get(board) {
        if t.lv as i32 != lv || t.selectivity_lv as i32 != selectivity_lv {
            return None;
        }
        let max = t.max as i32;
        let min = t.min as i32;
        if max <= *alpha {
            return Some(max);
        } else if min >= *beta {
            return Some(min);
        } else if max == min {
            return Some(max);
        }
        if min > *alpha {
            *alpha = min
        };
        if max < *beta {
            *beta = max
        };
    }
    None
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
pub fn set_move_eval(move_list: &mut [MoveBoard], lv: i32, alpha: i32, search: &mut Search) {
    for move_board in move_list.iter_mut() {
        if move_board.skip {
            continue;
        }
        move_board.eval = if lv < 1 {
            -simplest_eval(&move_board.board)
        } else {
            -negaalpha_eval(&move_board.board, -alpha - 6, -alpha + 6, lv - 1, search)
        };
    }
}

#[inline(always)]
pub fn set_move_eval_for_end_nws(move_list: &mut [MoveBoard], lv: i32, alpha: i32, search: &mut Search) {
    for move_board in move_list.iter_mut() {
        if move_board.skip {
            continue;
        }
        move_board.eval = if lv < 1 {
            -simplest_eval(&move_board.board)
        } else {
            -negaalpha_eval(&move_board.board, -alpha - 6, -alpha + 6, lv - 1, search)
        };

        // 終盤で検討
        let em = -(move_board.board.moves().count_ones() as i32);
        let ec = -((move_board.board.player & MC).count_ones() as i32);
        // let emc = 40 - (em + ec);
        move_board.eval += (em + ec) * 4;

        if move_board.board.moves().count_ones() as i32 <= 1 {
            move_board.eval += 60;
        }
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
        let em = -(move_board.board.moves().count_ones() as i32);
        let ec = -((move_board.board.player & MC).count_ones() as i32);
        // let ex = -((move_board.board.opponent & MX).count_ones() as i32);
        move_board.eval = em + ec; //(2 * ec - ex);
    }
}

#[inline(always)]
pub fn et_cut_off(
    alpha: &mut i32,
    beta: &mut i32,
    move_list: &mut [MoveBoard],
    lv: i32,
    selectivity_lv: i32,
    n_skip: &mut i32,
    t_table: &TranspositionTable,
) -> Option<i32> {
    for move_board in move_list.iter_mut() {
        if move_board.skip {
            continue;
        }
        if let Some(t) = t_table.get(&move_board.board) {
            if t.lv as i32 != (if lv == 60 { 60 } else { lv - 1 })
                || t.selectivity_lv as i32 != selectivity_lv
            {
                continue;
            }
            let u: i32 = t.max as i32;
            let l = t.min as i32;
            move_board.eval += 10; // 置換表に登録されている手を優先

            // 1手進めた手: [l, u]
            // 親ノード [-u, -l]
            if -u >= *beta {
                // alpha < beta <= -u <= -l
                // println!("fail high !");
                return Some(-u); // fail high
            } else if *alpha <= -u {
                // alpha <= -u <= beta <= -l or alpha <= -u <= -l <= beta
                *alpha = -u; // update alpha (alpha <= -u)
                if -l <= *alpha || u == l {
                    //この条件ならば、この子ノードは探索する必要がない
                    move_board.skip = true;
                    *n_skip += 1;
                }
            } else if -l <= *alpha {
                // -u <= -l <= alpha < beta
                move_board.skip = true;
                *n_skip += 1;
            }
        }
    }
    None
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
pub struct Search {
    pub eval_search_node_count: u64,
    pub eval_search_leaf_node_count: u64,
    pub perfect_search_node_count: u64,
    pub perfect_search_leaf_node_count: u64,
    pub t_table: TranspositionTable,
    pub origin_board: Board,
    pub eval_func: Evaluator,
    pub selectivity_lv: i32,
}

impl Search {
    pub fn new(evaluator: Evaluator) -> Search {
        Search {
            eval_search_node_count: 0,
            eval_search_leaf_node_count: 0,
            perfect_search_node_count: 0,
            perfect_search_leaf_node_count: 0,
            t_table: TranspositionTable::new(),
            origin_board: Board::new(),
            eval_func: evaluator,
            selectivity_lv: NO_MPC,
        }
    }
    pub fn clear_node_count(&mut self) {
        self.eval_search_node_count = 0;
        self.eval_search_leaf_node_count = 0;
        self.perfect_search_node_count = 0;
        self.perfect_search_leaf_node_count = 0;
    }

    pub fn set_board(&mut self, board: &Board) {
        self.origin_board = board.clone();
        self.clear_node_count();
    }
}
