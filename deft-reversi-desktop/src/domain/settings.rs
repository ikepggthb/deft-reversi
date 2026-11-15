//! What the settings screen changes, and the new game dialog's last choices.
use super::{Player, Preset, Strength, player::MOVE_TIMES};
use crate::{app::AnalysisSettings, engine_loader, i18n::Lang, services::engine::Resources};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    pub fn strength(&self) -> Strength {
        match (self.choice, self.mode) {
            (AiChoice::Preset(preset), _) => Strength::Preset(preset),
            (AiChoice::Manual, ManualMode::Level) => Strength::Level(self.level),
            (AiChoice::Manual, ManualMode::Time) => Strength::TimePerMove(MOVE_TIMES[self.time]),
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
