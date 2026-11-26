//! 手番の色を保持する局面型 [`Position`] を定義します。
//!
//! 探索エンジン内部の [`Board`] は「常に自分から見た盤面」として黒白の区別を持たないため、
//! 対局の表示や棋譜の記録を行う呼び出し側はこちらの型を使います。
use crate::{Action, Board, MoveError, Moves, Square};
use std::{fmt, str::FromStr};

/// プレイヤーの石の色（黒または白）を表します。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// 先手（黒）。
    Black,
    /// 後手（白）。
    White,
}
impl Color {
    /// 相手側の色を返します。
    pub const fn opponent(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }
}

/// 手番の色を保持する盤面です。
///
/// 探索関数には手番側から見た [`Board`] が渡されるため、
/// 探索スコアは常に「現在の手番側から見た石差」になります。
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
    /// リバーシの標準初期局面（黒番）を返します。
    pub fn initial() -> Self {
        Self {
            board: Board::new(),
            side: Color::Black,
        }
    }
    /// 黒石・白石のビットマスクと手番から局面を構築します。同じマスに両方の石が重なっている場合は `None` を返します。
    pub fn from_colors(black: u64, white: u64, side: Color) -> Option<Self> {
        let board = match side {
            Color::Black => Board::from_bits(black, white)?,
            Color::White => Board::from_bits(white, black)?,
        };
        Some(Self { board, side })
    }
    /// 手番側 `side` から見た [`Board`] をそのまま包んで [`Position`] を生成します。
    pub fn from_board(board: Board, side: Color) -> Self {
        Self { board, side }
    }
    /// 手番側から見た内部の [`Board`] を返します。
    pub fn board(&self) -> Board {
        self.board
    }
    /// 現在の手番の色を返します。
    pub fn side_to_move(&self) -> Color {
        self.side
    }
    /// 指定した色の石配置ビットマスクを返します。
    pub fn discs(&self, color: Color) -> u64 {
        if color == self.side {
            self.board.player_bits()
        } else {
            self.board.opponent_bits()
        }
    }
    /// 黒石のビットマスクを返します。
    pub fn black(&self) -> u64 {
        self.discs(Color::Black)
    }
    /// 白石のビットマスクを返します。
    pub fn white(&self) -> u64 {
        self.discs(Color::White)
    }
    /// 指定した色の石数を返します。
    pub fn disc_count(&self, color: Color) -> u32 {
        self.discs(color).count_ones()
    }
    /// 空きマス数を返します。
    pub fn empty_count(&self) -> u8 {
        self.board.empty_count()
    }
    /// 現在の手番の合法手集合を返します。
    pub fn legal_moves(&self) -> Moves {
        self.board.legal_moves()
    }
    /// 指定したマスに着手した際に反転する石のビットマスクを返します（非合法手なら 0）。
    pub fn flips(&self, square: Square) -> u64 {
        self.board.flips(square)
    }
    /// 現在の手番がパスしなければならないか（合法手がなく、かつ終局していないか）を返します。
    pub fn must_pass(&self) -> bool {
        self.board.must_pass()
    }
    /// 両者とも合法手がなく終局しているかを返します。
    pub fn is_terminal(&self) -> bool {
        self.board.is_terminal()
    }
    /// `color` 側から見た終局スコア（空きマスを勝者に加算した石差）を返します。
    pub fn final_score(&self, color: Color) -> i32 {
        let score = self.board.final_score();
        if color == self.side { score } else { -score }
    }
    /// `action` を実行した後の新しい局面を返します（手番は相手側に交代します）。
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

/// A1 から H8 までの 64 文字（`X` が黒、`O` が白、`-` が空）、空白、手番の色を出力します。
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

/// 局面文字列（[`Position::from_str`]）の解析に失敗したときのエラーです。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsePositionError(&'static str);
impl fmt::Display for ParsePositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ParsePositionError {}

/// [`Display`](fmt::Display) 形式および一般的な表記揺れ
/// （黒に `x` / `*`、白に `o`、空マスに `.`、末尾の `;`）を読み込みます。
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
        // 白石が 0 個になったため両者とも打てず終局となり、空きマスはすべて黒に加算されます。
        let terminal = Position::from_colors(0b111, 0, Color::White).unwrap();
        assert!(terminal.is_terminal());
        assert!(!terminal.must_pass());
        assert_eq!(terminal.final_score(Color::Black), 64);
        assert_eq!(terminal.final_score(Color::White), -64);
        assert!(initial.play(Action::Pass).is_err());
    }
}
