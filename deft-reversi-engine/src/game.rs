use crate::board::*;
use crate::eval::*;
use crate::t_table::*;
use crate::solver::*;

pub struct Game {
    state: State,
    undo_stack: Vec<State>,
    redo_stack: Vec<State>, 
    level: i32,
    solver: Solver,
}

pub struct State {
    board: Board,
    put_place: u8,
}


impl Game {
    pub fn new(evaluator: Evaluator) -> Self {
        let initial_board = Board::new();  // Boardの初期状態を作成        
        Game {
            state: State {
                board: initial_board,
                put_place: NO_COORD,
            },
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            level: 0,
            solver: Solver::new(evaluator),
        }
    }

    pub fn set_ai_level(&mut self, lv: i32) {
        self.solver.set_ai_level(lv);
        self.level = lv;
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
        self.update_new_state(b, NO_COORD);
    }

    pub fn is_end(&self) -> bool {
        let b = self.get_board();
        b.put_able().count_ones() == 0 && b.opponent_put_able().count_ones() == 0
    }

    pub fn ai_put(&mut self) -> Result<(), &'static str> {
        let a = self.solver.solve(&self.state.board);
        let result = match a {
            Ok(d) => {
                position_bit_to_str(d.best_move).unwrap()
            },
            Err(e) => {
                return Err("Invalid position");
            }
        };

        self.put(result.as_str())
    }

    pub fn get_eval_score(&mut self) -> Result<i32, &'static str> {
        match self.solver.solve(&self.state.board) {
            Ok(d) => {
                Ok(d.eval)
            },
            Err(e) => {
                Err("Invalid position")
            }
        }
    }

    pub fn get_move_scores(&mut self, ai_lv: i32) -> [i32; 64] {
        let mut scores = [0; 64];
        self.set_ai_level(ai_lv);
        let b = &self.state.board;
        let legal_moves =  b.put_able();
        for i in 0..64 {
            let mask = 1u64 << i;
            if mask & legal_moves != 0 {
                let position = position_bit_to_num(mask).unwrap();
                let mut b = b.clone();
                b.put_piece(mask);
                let result = 
                match self.solver.solve_no_iter(&b) {
                    Ok(r) => {
                        r
                    },
                    Err(e) => {
                        if let SolverErr::NoMove = e {
                            eprint!("solve err: No move");
                            return [-100; 64];
                        }
                        return [0; 64];
                    }
                };
                
                scores[position as usize] = -result.eval;
                
            }
        }
        self.set_ai_level(self.level);
        scores
    }

    pub fn record(&self) -> String {
        let mut s = String::new();
        for r in self.undo_stack.iter() {
            if r.put_place != NO_COORD {
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

}


