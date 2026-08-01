use crate::board::board::Board;
use crate::board::position::{position_num_to_str, position_str_to_num};
use crate::EngineError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Black,
    White,
}

impl Color {
    pub fn opponent(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }

    pub fn get_char(self) -> char {
        match self {
            Self::Black => 'X',
            Self::White => 'O',
        }
    }

    pub fn get_str(self) -> &'static str {
        match self {
            Self::Black => "Black",
            Self::White => "White",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Position {
    pub board: Board,
    pub turn: Color,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub current: Position,
    record: String,
    history: Vec<(Position, String)>,
    cursor: usize,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        Self {
            current: Position {
                board: Board::new(),
                turn: Color::Black,
            },
            record: String::new(),
            history: vec![(
                Position {
                    board: Board::new(),
                    turn: Color::Black,
                },
                String::new(),
            )],
            cursor: 0,
        }
    }

    pub fn pass(&mut self) {
        self.current.board.make_pass();
        self.current.turn = self.current.turn.opponent();
    }

    pub fn put(&mut self, move_str: &str) -> Result<(), EngineError> {
        let pos = position_str_to_num(move_str)?;
        let move_bit = 1u64 << pos;
        if self.current.board.moves() & move_bit == 0 {
            return Err(EngineError::IllegalMove {
                move_str: move_str.to_string(),
            });
        }
        self.current.board = self.current.board.make_move(move_bit);
        self.current.turn = self.current.turn.opponent();
        self.record.push_str(&position_num_to_str(pos)?);
        self.history.truncate(self.cursor + 1);
        self.history
            .push((self.current.clone(), self.record.clone()));
        self.cursor += 1;
        Ok(())
    }

    pub fn is_end(&self) -> bool {
        self.current.board.moves() == 0 && self.current.board.opponent_moves() == 0
    }

    pub fn is_pass(&self) -> bool {
        self.current.board.moves() == 0 && self.current.board.opponent_moves() != 0
    }

    pub fn undo(&mut self) -> Result<(), EngineError> {
        if self.cursor == 0 {
            return Err(EngineError::InvalidData(
                "undo is not available".to_string(),
            ));
        }
        self.cursor -= 1;
        let (position, record) = self.history[self.cursor].clone();
        self.current = position;
        self.record = record;
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), EngineError> {
        if self.cursor + 1 >= self.history.len() {
            return Err(EngineError::InvalidData(
                "redo is not available".to_string(),
            ));
        }
        self.cursor += 1;
        let (position, record) = self.history[self.cursor].clone();
        self.current = position;
        self.record = record;
        Ok(())
    }

    pub fn record(&self) -> String {
        self.record.clone()
    }
}

pub fn check_record(record: &str) -> Result<(), EngineError> {
    if record.len() % 2 != 0 {
        return Err(EngineError::InvalidRecord {
            reason: "record length must be even".to_string(),
            offset: record.len(),
        });
    }
    let mut game = Game::new();
    for (offset, chunk) in record.as_bytes().chunks(2).enumerate() {
        let byte_offset = offset * 2;
        let move_str = std::str::from_utf8(chunk).map_err(|_| EngineError::InvalidRecord {
            reason: "record is not UTF-8".to_string(),
            offset: byte_offset,
        })?;
        if game.is_pass() {
            game.pass();
        }
        game.put(move_str)
            .map_err(|err| EngineError::InvalidRecord {
                reason: err.to_string(),
                offset: byte_offset,
            })?;
    }
    if game.is_pass() {
        game.pass();
    }
    if !game.is_end() {
        return Err(EngineError::InvalidRecord {
            reason: "game is not ended".to_string(),
            offset: record.len(),
        });
    }
    Ok(())
}
