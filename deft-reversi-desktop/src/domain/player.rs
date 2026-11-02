//! Who plays each colour, and how strongly the AI plays (§7.1 of the plan).
use crate::services::engine::SearchSpec;
use deft_reversi_engine::Goal;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    Human,
    Ai(Strength),
}

impl Player {
    pub fn is_ai(self) -> bool {
        matches!(self, Self::Ai(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
    Strongest,
}

impl Preset {
    pub const ALL: [Self; 5] = [
        Self::Beginner,
        Self::Intermediate,
        Self::Advanced,
        Self::Expert,
        Self::Strongest,
    ];

    /// Levels 1 to 10 search without ProbCut; the strongest is the highest
    /// level whose longest move stayed near 5 s on 8 threads (§7.1).
    pub fn level(self) -> u8 {
        match self {
            Self::Beginner => 1,
            Self::Intermediate => 3,
            Self::Advanced => 6,
            Self::Expert => 10,
            Self::Strongest => 24,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    Preset(Preset),
    Level(u8),
    TimePerMove(Duration),
}

impl Default for Strength {
    fn default() -> Self {
        Self::Preset(Preset::Intermediate)
    }
}

/// A move-time search reads as deep as this level allows until the time is
/// up; its midgame depth keeps growing over any choosable time.
const TIMED_LEVEL: u8 = 40;

pub const MOVE_TIMES: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
];

impl Strength {
    pub fn spec(self) -> SearchSpec {
        match self {
            Self::Preset(preset) => SearchSpec {
                goal: Goal::Level(preset.level()),
                time_limit: None,
            },
            Self::Level(level) => SearchSpec {
                goal: Goal::Level(level.clamp(1, 60)),
                time_limit: None,
            },
            Self::TimePerMove(limit) => SearchSpec {
                goal: Goal::Level(TIMED_LEVEL),
                time_limit: Some(limit),
            },
        }
    }
}
