//! Positions with the colour to move, for callers that show or record games.
use crate::{Action, Board, MoveError, Moves, Square};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    Black,
    White,
}
impl Color {
    pub const fn opponent(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }
}

/// A board with the colour to move. Searches take its [`Board`], seen from
/// the side to move, so their scores are for that side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Position {
    board: Board,
    side: Color,
}
impl Default for Position {
    fn default() -> Self {
        Self::initial()
    }
}
impl Position {
    /// The standard starting position, black to move.
    pub fn initial() -> Self {
        Self {
            board: Board::new(),
            side: Color::Black,
        }
    }
    /// Rejects overlapping discs.
    pub fn from_colors(black: u64, white: u64, side: Color) -> Option<Self> {
        let board = match side {
            Color::Black => Board::from_bits(black, white)?,
            Color::White => Board::from_bits(white, black)?,
        };
        Some(Self { board, side })
    }
    /// `board` is seen from `side`, the colour to move.
    pub fn from_board(board: Board, side: Color) -> Self {
        Self { board, side }
    }
    pub fn board(&self) -> Board {
        self.board
    }
    pub fn side_to_move(&self) -> Color {
        self.side
    }
    pub fn discs(&self, color: Color) -> u64 {
        if color == self.side {
            self.board.player_bits()
        } else {
            self.board.opponent_bits()
        }
    }
    pub fn black(&self) -> u64 {
        self.discs(Color::Black)
    }
    pub fn white(&self) -> u64 {
        self.discs(Color::White)
    }
    pub fn disc_count(&self, color: Color) -> u32 {
        self.discs(color).count_ones()
    }
    pub fn empty_count(&self) -> u8 {
        self.board.empty_count()
    }
    pub fn legal_moves(&self) -> Moves {
        self.board.legal_moves()
    }
    pub fn flips(&self, square: Square) -> u64 {
        self.board.flips(square)
    }
    pub fn must_pass(&self) -> bool {
        self.board.must_pass()
    }
    pub fn is_terminal(&self) -> bool {
        self.board.is_terminal()
    }
    /// Disc difference for `color`, with the empty squares given to the winner.
    pub fn final_score(&self, color: Color) -> i32 {
        let score = self.board.final_score();
        if color == self.side { score } else { -score }
    }
    /// The position after `action`; the other colour moves next.
    pub fn play(&self, action: impl Into<Action>) -> Result<Self, MoveError> {
        Ok(Self {
            board: self.board.play(action.into())?,
            side: self.side.opponent(),
        })
    }
}
impl From<Position> for Board {
    fn from(position: Position) -> Self {
        position.board
    }
}
impl From<&Position> for Board {
    fn from(position: &Position) -> Self {
        position.board
    }
}

/// 64 squares from A1 to H8 (`X` black, `O` white, `-` empty), a space,
/// and the colour to move.
impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (black, white) = (self.black(), self.white());
        for index in 0..64 {
            let bit = 1u64 << index;
            f.write_str(if black & bit != 0 {
                "X"
            } else if white & bit != 0 {
                "O"
            } else {
                "-"
            })?;
        }
        f.write_str(match self.side {
            Color::Black => " X",
            Color::White => " O",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsePositionError(&'static str);
impl fmt::Display for ParsePositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ParsePositionError {}

/// Reads the [`Display`](fmt::Display) form and the common variants: `x`
/// and `*` for black, `o` for white, `.` for empty, and a trailing `;`.
impl FromStr for Position {
    type Err = ParsePositionError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split_whitespace();
        let cells = parts
            .next()
            .ok_or(ParsePositionError("missing board"))?
            .as_bytes();
        let side = parts
            .next()
            .ok_or(ParsePositionError("missing side to move"))?
            .trim_end_matches(';');
        if parts.next().is_some() {
            return Err(ParsePositionError("unexpected text after the side to move"));
        }
        if cells.len() != 64 {
            return Err(ParsePositionError("board must have 64 squares"));
        }
        let (mut black, mut white) = (0, 0);
        for (index, cell) in cells.iter().enumerate() {
            match cell {
                b'X' | b'x' | b'*' => black |= 1u64 << index,
                b'O' | b'o' => white |= 1u64 << index,
                b'-' | b'.' => {}
                _ => return Err(ParsePositionError("invalid square character")),
            }
        }
        let side = match side {
            "X" | "x" | "*" => Color::Black,
            "O" | "o" => Color::White,
            _ => return Err(ParsePositionError("side to move must be X or O")),
        };
        Ok(Self::from_colors(black, white, side).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_form_roundtrips_and_keeps_colors_through_moves() {
        let initial = Position::initial();
        let text = initial.to_string();
        assert_eq!(text.parse::<Position>().unwrap(), initial);
        let after = initial.play("F5".parse::<Action>().unwrap()).unwrap();
        assert_eq!(after.side_to_move(), Color::White);
        assert_eq!(after.disc_count(Color::Black), 4);
        assert_eq!(after.disc_count(Color::White), 1);
        assert_eq!(after.to_string().parse::<Position>().unwrap(), after);
        let variant = text.replace('X', "*").replace('O', "o").replace('-', ".") + ";";
        assert_eq!(variant.parse::<Position>().unwrap(), initial);
        assert!("X O".parse::<Position>().is_err());
        assert!(text.replace(" X", " Z").parse::<Position>().is_err());
    }
    #[test]
    fn moves_flips_and_scores_are_seen_by_color() {
        let initial = Position::initial();
        let moves: Vec<String> = initial.legal_moves().map(|s| s.to_string()).collect();
        assert_eq!(moves, ["D3", "C4", "F5", "E6"]);
        let f5 = Square::from_xy(5, 4).unwrap();
        assert_eq!(f5.to_string(), "F5");
        assert_eq!(initial.flips(f5), Square::from_xy(4, 4).unwrap().bit());
        assert_eq!(initial.flips(Square::from_xy(0, 0).unwrap()), 0);
        // White has no discs left: nobody can move, and black takes the empties.
        let terminal = Position::from_colors(0b111, 0, Color::White).unwrap();
        assert!(terminal.is_terminal());
        assert!(!terminal.must_pass());
        assert_eq!(terminal.final_score(Color::Black), 64);
        assert_eq!(terminal.final_score(Color::White), -64);
        assert!(initial.play(Action::Pass).is_err());
    }
}
