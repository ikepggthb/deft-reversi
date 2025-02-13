use crate::board::*;


pub struct Game {
    pub current: State,
    undo_stack: Vec<State>,
    redo_stack: Vec<State>, 
}

pub struct State {
    pub board: Board,
    pub put_place: u8,
    pub turn: Color
}

#[derive(Clone, Copy)]
pub enum Color {
    Black,
    White
}

impl Color {
    pub fn opponent(&self) -> Color {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black
        }
    }
    pub fn get_char(&self) -> char {
        match self {
            Self::Black => 'X',
            Self::White => 'O'
        }
    }
    pub fn get_str(&self) -> &str {
         match self {
            Self::Black => "Black",
            Self::White => "White"
        }
    }
}

/// 棋譜データ（ASCII 文字列）を2文字単位で処理します。
///
/// # Arguments
///
/// * `record` - 棋譜データを表す ASCII 文字列。
/// * `process_chunk` - 各2文字のチャンクに対する処理を行うクロージャ。(Result<(), &'static str>)
///
/// # Errors
///
/// ASCII 文字以外が含まれている場合、または UTF-8 変換エラーの場合はエラーを返します。
fn for_each_record<F>(record: &str, mut process_chunk: F) -> Result<(), &'static str>
where
    F: FnMut(&str) -> Result<(), &'static str>,
{
    if !record.is_ascii() {
        return Err("Record contains non-ASCII characters");
    }
    for chunk in record.as_bytes().chunks_exact(2) {
        let chunk_str = std::str::from_utf8(chunk).map_err(|_| "Invalid UTF-8 sequence")?;
        process_chunk(chunk_str)?;
    }
    Ok(())
}

pub fn count_record(record: &str) -> Result<usize, &'static str> {
    if !record.is_ascii() {
        return Err("Record contains non-ASCII characters");
    }
    Ok(record.len() / 2)
}

pub fn check_record(record: &str) -> Result<(), &str> {
    if !record.is_ascii() {
        return Err("Record contains non-ASCII characters");
    }
    let mut b = Board::new();
    for chunk in record.as_bytes().chunks_exact(2) {
        let chunk_str = std::str::from_utf8(chunk).map_err(|_| "Invalid UTF-8 sequence")?;
        let put_mask = position_str_to_bit(chunk_str)?;
        if b.put(put_mask).is_err() {
            return Err("invalid move");
        }
    }

    Ok(())
}

impl Game {
    pub fn new() -> Self {
        let initial_board = Board::new();  // Boardの初期状態を作成        
        Game {
            current: State {
                board: initial_board,
                put_place: NO_COORD,
                turn: Color::Black
            },
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }


    pub fn from_record(record: &str) -> Result<Self, &'static str>
    {
        let mut game = Game::new();

        for_each_record(record, |position| {
            game.put(position)?;
            if game.is_pass() {game.pass();}
            Ok(())
        })?;
        
        Ok(game)
    }
    
    fn update_new_state(&mut self, new_board: Board, put_place: u8, turn: Color) {
        self.undo_stack.push(State { board: self.current.board.clone(), put_place, turn: self.current.turn});
        self.redo_stack.clear();
        self.current.board = new_board;
        self.current.put_place = NO_COORD;
        self.current.turn = turn;
    }

    pub fn undo(&mut self) -> Result<(), &'static str>{
        match  self.undo_stack.pop(){
            Some(prev_state) => {
                let current_state = std::mem::replace(&mut self.current, prev_state);
                self.redo_stack.push(current_state);
                if self.current.put_place != PASS {
                    self.current.put_place = NO_COORD;
                }
                Ok(())
            },
            None => {
                Err("no previous state to undo")
            }
        }
    }

    pub fn redo(&mut self) -> Result<(), &'static str> {
        match self.redo_stack.pop() {
            Some(next_state) => {
                let current_state = std::mem::replace(&mut self.current, next_state);
                self.undo_stack.push(current_state);
                Ok(())
            },
            None => Err("no next state to redo"),
        }
    }

    pub fn put(&mut self, positon: &str) -> Result<(), &'static str> {
        let mut b = self.current.board.clone();

        let position_bit = position_str_to_bit(positon)?;

        match b.put(position_bit) {
            Ok(()) => {
                self.update_new_state(b.clone(), position_bit_to_num(position_bit)?, self.current.turn.opponent());
            },
            Err(_) => return Err("Invalid position")
        };
        Ok(())       
    }

    pub fn is_pass(&self) -> bool {
        let b = &self.current.board;
        b.moves().count_ones() == 0 && b.opponent_moves().count_ones() != 0
    }

    pub fn pass(&mut self) {
        if !self.is_pass() {return;}

        let mut b = self.current.board.clone();
        b.swap();
        self.update_new_state(b, PASS, self.current.turn.opponent());
    }

    pub fn is_end(&self) -> bool {
        let b = &self.current.board;
        b.moves().count_ones() == 0 && b.opponent_moves().count_ones() == 0
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

        if let Ok(bit_p) = position_num_to_bit(self.current.put_place as i32) {
            let str_p = position_bit_to_str(bit_p).unwrap();
            s.push_str(&str_p);
        }
        
        s
    }

    pub fn get_last_move(&self) -> Option<i32> {
        self.undo_stack.last().map(|p| p.put_place as i32)
    }

}


