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

    /// The name in files.
    pub fn key(self) -> &'static str {
        match self {
            Self::Beginner => "beginner",
            Self::Intermediate => "intermediate",
            Self::Advanced => "advanced",
            Self::Expert => "expert",
            Self::Strongest => "strongest",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

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
            Self::Preset(preset) => SearchSpec::new(Goal::Level(preset.level())),
            Self::Level(level) => SearchSpec::new(Goal::Level(level.clamp(1, 60))),
            Self::TimePerMove(limit) => SearchSpec {
                time_limit: Some(limit),
                ..SearchSpec::new(Goal::Level(TIMED_LEVEL))
            },
        }
    }
}
