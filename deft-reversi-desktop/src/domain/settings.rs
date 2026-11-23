//! What the settings screen changes, and the new game dialog's last choices.
use super::{Player, Preset, Strength, player::MOVE_TIMES};
use crate::{engine_loader, i18n::Lang};
use deft_reversi_engine::Square;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resources {
    pub threads: usize,
    pub tt_mib: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisSettings {
    /// For the shown position, which also scores every move, and for each
    /// position of the whole record.
    pub level: u8,
    /// A move that loses this many discs or more is marked in the record.
    pub loss_threshold: i32,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            level: 20,
            loss_threshold: 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub lang: Lang,
    pub show_legal_moves: bool,
    pub resources: Resources,
    pub analysis: AnalysisSettings,
    /// In analysis mode, the candidate moves show their values or dots.
    pub candidate_values: bool,
    pub theme: ThemeChoice,
    pub animation: Animation,
    pub sound: bool,
    /// 0 to 100.
    pub volume: u8,
    /// The window's last size, in logical pixels.
    pub window_size: Option<(u32, u32)>,
    /// Kept between games, so the dialog opens with the last choices.
    pub new_game: NewGameForm,
    pub board_view: BoardView,
    pub book: BookSettings,
    /// The plot of the value graph, in logical pixels; dragged to size.
    pub graph_height: u16,
}

/// How tall the value graph may be dragged.
pub const GRAPH_HEIGHTS: std::ops::RangeInclusive<u16> = 60..=400;

/// The opening book the AI plays from (§24).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BookSettings {
    /// A `.dbook`; none by default.
    pub path: Option<PathBuf>,
    /// Off keeps the file chosen but plays no book moves.
    pub enabled: bool,
}

impl BookSettings {
    /// Whether AI moves may come from the book.
    pub fn use_for_ai(&self) -> bool {
        self.enabled && self.path.is_some()
    }
}

/// How the board is turned on screen: mirrored left to right, then turned
/// clockwise a quarter at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardView {
    /// Quarter turns clockwise, 0..4.
    turns: u8,
    pub mirrored: bool,
}

impl BoardView {
    pub fn new(turns: u8, mirrored: bool) -> Self {
        Self {
            turns: turns % 4,
            mirrored,
        }
    }

    pub fn turns(self) -> u8 {
        self.turns
    }

    pub fn turned(self) -> Self {
        Self {
            turns: (self.turns + 1) % 4,
            ..self
        }
    }

    pub fn mirrored(self) -> Self {
        Self {
            mirrored: !self.mirrored,
            ..self
        }
    }

    /// The square shown in column `x` and row `y` (0..8, from the top left).
    pub fn square_at(self, x: u8, y: u8) -> Square {
        debug_assert!(x < 8 && y < 8);
        let (mut x, mut y) = (x, y);
        // A clockwise turn shows board (x, y) at (7 - y, x); undo the turns.
        for _ in 0..self.turns % 4 {
            (x, y) = (y, 7 - x);
        }
        if self.mirrored {
            x = 7 - x;
        }
        Square::from_xy(x, y).expect("x and y are 0..8")
    }

    /// Where `square` is shown: its column and row.
    pub fn place_of(self, square: Square) -> (u8, u8) {
        let (mut x, mut y) = (square.x(), square.y());
        if self.mirrored {
            x = 7 - x;
        }
        for _ in 0..self.turns % 4 {
            (x, y) = (7 - y, x);
        }
        (x, y)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::Ja,
            show_legal_moves: true,
            resources: engine_loader::default_resources(),
            analysis: AnalysisSettings::default(),
            candidate_values: true,
            theme: ThemeChoice::System,
            animation: Animation::Normal,
            sound: true,
            volume: 60,
            window_size: None,
            new_game: NewGameForm::default(),
            board_view: BoardView::default(),
            book: BookSettings {
                enabled: true,
                ..BookSettings::default()
            },
            graph_height: 80,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

/// How discs turn over (§9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animation {
    Normal,
    /// Half as long.
    Short,
    Off,
}

/// The table sizes the settings screen offers.
pub const TT_SIZES_MIB: [usize; 6] = [32, 64, 128, 256, 512, 1024];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiChoice {
    Preset(Preset),
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualMode {
    Level,
    Time,
}

/// One colour in the new game dialog. The manual values stay while a
/// preset is chosen, so switching back keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideForm {
    pub ai: bool,
    pub choice: AiChoice,
    pub mode: ManualMode,
    pub level: u8,
    /// An index into `MOVE_TIMES`.
    pub time: usize,
}

impl SideForm {
    pub fn human() -> Self {
        Self {
            ai: false,
            choice: AiChoice::Preset(Preset::Intermediate),
            mode: ManualMode::Level,
            level: 20,
            time: 2,
        }
    }

    pub fn time_duration(&self) -> Duration {
        MOVE_TIMES[self.time.min(MOVE_TIMES.len() - 1)]
    }

    pub fn strength(&self) -> Strength {
        match (self.choice, self.mode) {
            (AiChoice::Preset(preset), _) => Strength::Preset(preset),
            (AiChoice::Manual, ManualMode::Level) => Strength::Level(self.level),
            (AiChoice::Manual, ManualMode::Time) => Strength::TimePerMove(self.time_duration()),
        }
    }

    pub fn player(&self) -> Player {
        if self.ai {
            Player::Ai(self.strength())
        } else {
            Player::Human
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewGameForm {
    /// Black, then White.
    pub sides: [SideForm; 2],
    pub from_current: bool,
}

impl Default for NewGameForm {
    fn default() -> Self {
        Self {
            sides: [
                SideForm::human(),
                SideForm {
                    ai: true,
                    ..SideForm::human()
                },
            ],
            from_current: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(view: BoardView, x: u8, y: u8) -> String {
        view.square_at(x, y).to_string().to_lowercase()
    }

    #[test]
    fn turns_and_mirrors_move_the_corners() {
        let plain = BoardView::default();
        assert_eq!(named(plain, 0, 0), "a1");
        // A quarter turn clockwise brings a8 to the top left.
        assert_eq!(named(plain.turned(), 0, 0), "a8");
        assert_eq!(named(plain.turned().turned(), 0, 0), "h8");
        assert_eq!(named(plain.mirrored(), 0, 0), "h1");
        assert_eq!(plain.turned().turned().turned().turned(), plain);
        for view in [plain.turned(), plain.mirrored().turned()] {
            for square in (0..64).map(|i| view.square_at(i % 8, i / 8)) {
                let (x, y) = view.place_of(square);
                assert_eq!(view.square_at(x, y), square);
            }
        }
    }
}
