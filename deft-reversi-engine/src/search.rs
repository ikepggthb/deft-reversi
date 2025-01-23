use crate::{board::*, simplest_eval, TableData};
use crate::eval::Evaluator;
use crate::eval_search::negaalpha_eval;
use crate::t_table::TranspositionTable;
use crate::mpc::NO_MPC;

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
    pub put_place: u8
}

#[inline(always)]
pub fn get_put_boards(board: &Board, legal_moves: u64) -> Vec<PutBoard>
{
    let mut put_boards: Vec<PutBoard> = Vec::with_capacity(legal_moves.count_ones() as usize);
    for pos in MoveIterator::new(legal_moves) {
        let mut board = board.clone();
        board.put_piece_fast(pos);
        put_boards.push(PutBoard{board, put_place: pos.trailing_zeros() as u8})
    }    
    put_boards
}


#[inline(always)]
pub fn t_table_cut_off_td(
    alpha   :       &mut i32,
    beta    :       &mut i32,
    lv      :       i32,
    selectivity_lv: i32,
    table_data :       &Option<&TableData> ) -> Option<i32>
{
    if let Some(t) = table_data {
        if t.lv as i32 != lv || t.selectivity_lv as i32 != selectivity_lv {return None;}
        let max = t.max as i32;
        let min = t.min as i32;
        if max <= *alpha {return Some(max);}
        else if min >= *beta {return Some(min);}
        else if max == min {return Some(max);}
        if min > *alpha {*alpha = min};
        if max < *beta {*beta = max};
    }
    None
}

#[inline(always)]
pub fn t_table_cut_off(
    board   :       & Board,
    alpha   :       &mut i32,
    beta    :       &mut i32,
    lv      :       i32,
    selectivity_lv: i32,
    t_table :       & TranspositionTable ) -> Option<i32>
{
    if let Some(t) = t_table.get(board) {
        if t.lv as i32 != lv || t.selectivity_lv as i32 != selectivity_lv {return None;}
        let max = t.max as i32;
        let min = t.min as i32;
        if max <= *alpha {return Some(max);}
        else if min >= *beta {return Some(min);}
        else if max == min {return Some(max);}
        if min > *alpha {*alpha = min};
        if max < *beta {*beta = max};
    }
    None
}

pub struct MoveBoard {
    eval: i32,
    pub board: Board,
    pub put_place: u8,
    pub skip: bool
}
pub const MOVE_MAX: usize = 36;


#[inline(always)]
pub fn set_move_list(board: &Board, moves_bit: u64, moves_list : &mut [MoveBoard]) {
    for (i, move_bit) in MoveIterator::new(moves_bit).enumerate(){
        let mut b = board.clone();
        b.put_piece_fast(move_bit);
        moves_list[i] = MoveBoard { eval: 0, board: b, put_place: move_bit.trailing_zeros() as u8 , skip: false};
    }
}


#[inline(always)]
pub fn set_move_eval(move_list: &mut [MoveBoard], lv: i32, search: &mut Search) {
    for move_board in move_list.iter_mut() {
        if move_board.skip { continue;} 
        move_board.eval = 
        if lv < 1 {
            -simplest_eval(&move_board.board)
        } else {
            -negaalpha_eval(&move_board.board, -SCORE_INF, SCORE_INF, lv-1, search)
        };   
    }
}

const MC: u64 = 0b1000000100000000000000000000000000000000000000000000000010000001_u64;
const MX: u64 = 0b0000000001000010000000000000000000000000000000000100001000000000_u64;

#[inline(always)]
pub fn set_move_eval_ffs(move_list: &mut [MoveBoard]) {
    for move_board in move_list.iter_mut() {
        if move_board.skip {continue;}
        let ec = (move_board.board.player & MC).count_ones() as i32 - (move_board.board.opponent & MC).count_ones() as i32;
        let ex = (move_board.board.player & MX).count_ones() as i32 - (move_board.board.opponent & MX).count_ones() as i32;
        move_board.eval = -(move_board.board.moves().count_ones() as i32) - (2 * ec - ex);
    }
}


#[inline(always)]
pub fn et_cut_off(
    alpha   :       &mut i32,
    beta    :       &mut i32,
    move_list: &mut [MoveBoard],
    lv      :       i32,
    selectivity_lv: i32,
    n_skip  : &mut i32,
    t_table :       & TranspositionTable ) -> Option<i32>
{
    for move_board in move_list.iter_mut() {
        if let Some(t) = t_table.get(&move_board.board) {
            if t.lv as i32 != (if lv == 60 {60} else {lv  - 1}) || t.selectivity_lv as i32 != selectivity_lv {continue;}
            let u: i32 = t.max as i32;
            let l = t.min as i32;
            move_board.eval += 10; // 置換表に登録されている手を優先
            
            // 1手進めた手: [l, u]
            // 親ノード [-u, -l]
            if -u >= *beta {// alpha < beta <= -u <= -l
                // println!("fail high !");
                return Some(-u); // fail high
            } else if *alpha <= -u { // alpha <= -u <= beta <= -l or alpha <= -u <= -l <= beta
                *alpha = -u; // update alpha (alpha <= -u)
                if -l <= *alpha || u == l { //この条件ならば、この子ノードは探索する必要がない
                    move_board.skip = true;
                    *n_skip += 1;
                } 
            } 
            else if -l <= *alpha { // -u <= -l <= alpha < beta
                move_board.skip = true;
                *n_skip += 1;
            }
        }
    }
    None
}



#[inline(always)]
pub fn sort_move_list(move_list: &mut [MoveBoard]) {
    let n_boards = move_list.len();

    if n_boards < 2 {
        return;
    }

    if n_boards == 2 && move_list[0].eval < move_list[1].eval {
        move_list.swap(0, 1);
        return;
    }
    
    if n_boards == 3 {
        if move_list[1].eval < move_list[2].eval {
            move_list.swap(1, 2);
        }
        if move_list[0].eval < move_list[1].eval {
            move_list.swap(0, 1);
        }
        return;
    }

    if n_boards <= 5 {
        move_list.sort_unstable_by(|a, b| b.eval.partial_cmp(&a.eval).unwrap());
        return;
    }

    // n_board > 5
    let top_n = 5; // 最大で上位5つをソート
    for i in 0..top_n {
        let mut max_idx = i;
        for j in (i + 1)..n_boards {
            if move_list[j].eval > move_list[max_idx].eval {
                max_idx = j;
            }
        }
        move_list.swap(i, max_idx);
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
    pub fn new(evaluator: Evaluator) -> Search{
        Search{
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
    pub fn clear_node_count(&mut self){
        self.eval_search_node_count = 0;
        self.eval_search_leaf_node_count = 0;
        self.perfect_search_node_count = 0;
        self.perfect_search_leaf_node_count = 0;
    }

    pub fn set_board(&mut self, board :&Board) {
        self.origin_board = board.clone();
        self.clear_node_count();
    }
}


/// テスト用
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_iterator_parity_ordering() {
        // 4つの小盤面のマスクを1つずつ作り、
        //   Q0 (左下): 3マス空き → 奇数
        //   Q1 (右下): 2マス空き → 偶数
        //   Q2 (左上): 5マス空き → 奇数
        //   Q3 (右上): 4マス空き → 偶数
        // とするため、適当に下位ビットだけ操作します。
        
        // 左下16マス (Q0)
        let q0 = 0b111u64; // 下位3ビットが空き
        // 右下16マス (Q1)
        let q1 = 0b11u64 << 16; // 16～17ビットが空き
        // 左上16マス (Q2)
        let q2 = 0b1_1111u64 << 32; // 32～36ビットが空き (5ビット)
        // 右上16マス (Q3)
        let q3 = 0b1111u64 << 48; // 48～51ビットが空き (4ビット)

        // 全空きマス
        let empties = q0 | q1 | q2 | q3;

        // 今回は簡単のため、player=!(empties), opponent=0 とし
        // "empties 以外は全部 player の石" という仮の状態にします。
        let board = Board {
            player: !empties,
            opponent: 0,
        };

        // 全ての空きマスが合法手と仮定 (単純化のため)
        let legal_moves = empties;

        // イテレータを作る
        let iter = MoveIteratorParity::new(legal_moves, &board);

        // まず odd (Q0, Q2) が先に返ってきて、次に even (Q1, Q3) が返ってくるはず。
        // odd は q0(3ビット) + q2(5ビット) = 計8ビット
        // even は q1(2ビット) + q3(4ビット) = 計6ビット
        let mut result = Vec::new();
        for m in iter {
            result.push(m);
        }

        // 期待する個数: odd 8手 + even 6手 = 14手
        assert_eq!(result.len(), 14, "合計手数が想定(14)と一致しない");

        // odd に含まれるビット
        let expected_odd = q0 | q2;
        // even に含まれるビット
        let expected_even = q1 | q3;

        // テスト方針：
        //   - イテレータから取り出した順で、最初に出てくるのはすべて odd に属しているはず
        //   - odd に属する手が尽きたあとは、even に属する手のみが返るはず
        let mut in_odd_phase = true;

        for (i, &bit) in result.iter().enumerate() {
            let is_odd = (bit & expected_odd) != 0;
            let is_even = (bit & expected_even) != 0;
            // どちらかには属しているはず
            assert!(is_odd ^ is_even, "どちらの集合にも属さない / 両方に属するビットが混在している");

            if in_odd_phase {
                // odd フェーズ中に even が来たら、その後は odd が来てはいけない
                if is_even {
                    in_odd_phase = false;
                }
            } else {
                // even フェーズに入ったら odd は登場しないはず
                assert!(is_even, "odd 手が終わった後に odd ビットが返ってきた");
            }

            eprintln!("{}番目: 0x{:x} (is_odd={})", i, bit, is_odd);
        }

        // 実際に8個分 odd、あとの6個分 even になっているか
        let odd_moves: Vec<u64> = result.iter().copied().filter(|m| (expected_odd & m) != 0).collect();
        let even_moves: Vec<u64> = result.iter().copied().filter(|m| (expected_even & m) != 0).collect();
        assert_eq!(odd_moves.len(), 8, "奇数マス空きの手(odd)が8手でなかった");
        assert_eq!(even_moves.len(), 6, "偶数マス空きの手(even)が6手でなかった");
    }
}

