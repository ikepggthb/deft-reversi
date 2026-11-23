//! The position editor's draft (§9.1 of the plan). It is apart from the
//! game, so cancelling leaves the game as it was.
use deft_reversi_engine::{Color, Position, Square};

/// Fewer discs than this are outside what the evaluation was trained on:
/// such positions are shown and saved, but the AI and analysis stay off.
pub const MIN_SEARCH_DISCS: u32 = 4;

pub fn searchable(position: Position) -> bool {
    position.disc_count(Color::Black) + position.disc_count(Color::White) >= MIN_SEARCH_DISCS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pen {
    Black,
    White,
    Erase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorDraft {
    black: u64,
    white: u64,
    pub side: Color,
}

/// What starting from the draft would mean, when it needs saying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caveat {
    /// The side to move cannot; the other side starts after a pass.
    Pass(Color),
    /// Neither side can move: a finished position.
    Finished,
    TooFewDiscs,
}

impl EditorDraft {
    pub fn from_position(position: Position) -> Self {
        Self {
            black: position.black(),
            white: position.white(),
            side: position.side_to_move(),
        }
    }

    pub fn initial() -> Self {
        Self::from_position(Position::initial())
    }

    pub fn empty(side: Color) -> Self {
        Self {
            black: 0,
            white: 0,
            side,
        }
    }

    pub fn paint(&mut self, square: Square, pen: Pen) {
        let bit = square.bit();
        self.black &= !bit;
        self.white &= !bit;
        match pen {
            Pen::Black => self.black |= bit,
            Pen::White => self.white |= bit,
            Pen::Erase => {}
        }
    }

    /// The draft never has a square of both colours.
    pub fn position(&self) -> Position {
        Position::from_colors(self.black, self.white, self.side).expect("colours are disjoint")
    }

    pub fn caveat(&self) -> Option<Caveat> {
        let position = self.position();
        if !searchable(position) {
            Some(Caveat::TooFewDiscs)
        } else if position.is_terminal() {
            Some(Caveat::Finished)
        } else if position.must_pass() {
            Some(Caveat::Pass(self.side))
        } else {
            None
        }
    }

    /// The engine's text: 64 squares from A1 (`X`, `O`, `-`), a space, and
    /// the side to move.
    pub fn text(&self) -> String {
        self.position().to_string()
    }

    /// Reads the engine's text; surrounding spaces and line breaks are fine.
    pub fn parse(text: &str) -> Option<Self> {
        text.trim()
            .parse::<Position>()
            .ok()
            .map(Self::from_position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(name: &str) -> Square {
        name.parse().unwrap()
    }

    #[test]
    fn painting_text_and_caveats() {
        let mut draft = EditorDraft::initial();
        assert_eq!(draft.caveat(), None);
        assert_eq!(
            EditorDraft::parse(&format!("  {}\n", draft.text())),
            Some(draft)
        );
        assert_eq!(EditorDraft::parse("not a position"), None);

        // Painting replaces whatever was on the square.
        draft.paint(square("d4"), Pen::Black);
        assert_eq!(draft.position().disc_count(Color::Black), 3);
        draft.paint(square("d4"), Pen::Erase);
        assert_eq!(draft.position().disc_count(Color::White), 1);

        let mut draft = EditorDraft::empty(Color::Black);
        draft.paint(square("a1"), Pen::Black);
        draft.paint(square("b1"), Pen::White);
        assert_eq!(draft.caveat(), Some(Caveat::TooFewDiscs));

        // Black A1, D1; White B1, A2; White to move cannot, Black can.
        draft.paint(square("d1"), Pen::Black);
        draft.paint(square("a2"), Pen::White);
        draft.side = Color::White;
        assert_eq!(draft.caveat(), Some(Caveat::Pass(Color::White)));

        // Only black discs: nobody can move.
        let mut draft = EditorDraft::empty(Color::Black);
        for name in ["a1", "b1", "c1", "d1"] {
            draft.paint(square(name), Pen::Black);
        }
        assert_eq!(draft.caveat(), Some(Caveat::Finished));
    }
}
