use crate::board::*;
use crate::eval::*;
use crate::t_table::*;

pub struct Game {
    state: State,
    undo_stack: Vec<State>,
    redo_stack: Vec<State>, 
}

pub struct State {
    board: Board,
    put_place: u8,
}

const PASS: u8 = 64;

impl Game {
    pub fn new() -> Self {
        let initial_board = Board::new();  // Boardの初期状態を作成        
        Game {
            state: State {
                board: initial_board,
                put_place: NO_COORD,
            },
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn get_board(&self) -> &Board {
        &self.state.board
    }

    fn update_new_state(&mut self, new_board: Board, put_place: u8) {
        self.undo_stack.push(State { board: self.get_board().clone(), put_place: put_place});
        self.redo_stack.clear();
        self.state.board = new_board;
        self.state.put_place = NO_COORD;
    }

    pub fn undo(&mut self) {
        if let Some(prev_state) = self.undo_stack.pop() {
            let current_state = std::mem::replace(&mut self.state, prev_state);
            self.redo_stack.push(current_state);
        }
        if self.state.put_place != PASS {
            self.state.put_place = NO_COORD;
        }
    }

    pub fn put(&mut self, positon: &str) -> Result<(), &'static str> {
        let mut b = self.get_board().clone();

        let position_bit = position_str_to_bit(positon)?;

        match b.put_piece(position_bit) {
            Ok(()) => {
                self.update_new_state(b.clone(), position_bit_to_num(position_bit)?);
            },
            Err(_) => return Err("Invalid position")
        };
        Ok(())       
    }

    pub fn is_pass(&self) -> bool {
        let b = self.get_board();
        b.put_able().count_ones() == 0 && b.opponent_put_able().count_ones() != 0
    }

    pub fn pass(&mut self) {
        if !self.is_pass() {return;}

        let mut b = self.get_board().clone();
        b.next_turn ^= 1;
        self.update_new_state(b, PASS);
    }

    pub fn is_end(&self) -> bool {
        let b = self.get_board();
        b.put_able().count_ones() == 0 && b.opponent_put_able().count_ones() == 0
    }

    pub fn record(&self) -> String {
        let mut s = String::new();
        for r in self.undo_stack.iter() {
            if r.put_place != NO_COORD && r.put_place != PASS {
                if let Ok(bit_p) = position_num_to_bit(r.put_place as i32) {
                    let str_p = position_bit_to_str(bit_p).unwrap();
                    s.push_str(&str_p);
                }
            }
        }

        if let Ok(bit_p) = position_num_to_bit(self.state.put_place as i32) {
            let str_p = position_bit_to_str(bit_p).unwrap();
            s.push_str(&str_p);
        }
        
        s
    }

    pub fn get_last_move(&self) -> Option<i32> {
        self.undo_stack.last().map(|p| p.put_place as i32)
    }

    pub fn do_over(&mut self) {
        let next_turn = self.state.board.next_turn;
        loop {
            self.undo();
            if (self.state.board.next_turn == next_turn && self.state.put_place != PASS) 
                || self.undo_stack.is_empty() {
                break;
            }
        }
    }

}


