use crate::board::*;
use rand::{Rng};

#[derive(Clone)]
pub struct TableData {
    exists: bool,
    pub board: Board,
    pub max: i8,
    pub min: i8,
    pub lv: u8,
    pub selectivity_lv : u8,
    pub best_move: u8
}

impl TableData {
    fn make_blank() -> Self{
        Self {
            exists: false,
            board: Board {bit_board: [0, 0],next_turn: 0},
            max: 0,
            min: 0,
            lv: 0,
            selectivity_lv: 0,
            best_move: u8::MAX,
        }
    }
}

const TABLE_SIZE: usize = 1 << 20;
pub struct TranspositionTable {
    table: Vec::<TableData>,
    rand_table: [u32; 1<<16]
}

impl Default for TranspositionTable {
    fn default() -> Self {
        let rand_table: [u32; 1<<16] = Self::gen_rand_table();
        Self {
            table: vec![TableData::make_blank(); TABLE_SIZE],
            rand_table
        }
    }
}

impl TranspositionTable {
    pub fn new() -> Self{
        Self::default()
    }
    fn gen_rand_table() -> [u32; 1<<16] {
        let mut rng = rand::thread_rng();
        let mut table = [0; 1<<16];
    
        for ti in table.iter_mut() {
            *ti = rng.gen_range(0..TABLE_SIZE as u32);
        }
    
        table
    }

    #[inline(always)]
    pub fn hash_board(&self, board: &Board) -> usize{
        let player_board_bit = board.bit_board[board.next_turn];
        let opponent_board_bit = board.bit_board[board.next_turn ^ 1];

        (
            self.rand_table[(player_board_bit & 0xFFFF) as usize] ^
            self.rand_table[((player_board_bit >> 16) & 0xFFFF) as usize] ^
            self.rand_table[((player_board_bit >> 32) & 0xFFFF) as usize] ^
            self.rand_table[((player_board_bit >> 48) & 0xFFFF) as usize] ^
            self.rand_table[((opponent_board_bit >> 48) & 0xFFFF) as usize] ^
            self.rand_table[((opponent_board_bit >> 32) & 0xFFFF) as usize] ^
            self.rand_table[((opponent_board_bit >> 16) & 0xFFFF) as usize] ^
            self.rand_table[(opponent_board_bit & 0xFFFF) as usize]
        ) as usize
    }

    #[inline(always)]
    pub fn add(&mut self, board: &Board, min: i32, max: i32, lv: i32, selectivity_lv: i32,best_move: u8 ) {

    #[cfg(debug_assertions)]
    {
        const MAX:i32 = i8::MAX as i32;
        const MIN:i32 = i8::MIN as i32;
        assert!(MIN <= min && min <= max && max <= MAX, 
            " in function t_table::add() , min: {min}, max: {min}, Lv: {lv}, best move: {best_move}");
    }
        let index = self.hash_board(board);
        self.table[index] = TableData {
            exists: true,
            board: board.clone(),
            max: max as i8,
            min: min as i8,
            lv: lv as u8,
            selectivity_lv: selectivity_lv as u8,
            best_move
        }
    }

    #[inline(always)]
    pub fn get(&self, board: &Board) -> Option<&TableData>{
        let index = self.hash_board(board);
        let x = &self.table[index];
        if !x.exists {return None;}

        if x.board.bit_board[Board::BLACK] == board.bit_board[Board::BLACK] &&
           x.board.bit_board[Board::WHITE] == board.bit_board[Board::WHITE] &&
           x.board.next_turn == board.next_turn {
            Some(x)
        } else {
            None
        }
    }

}