//! Game state without GPUI: the record of a game, its players and rules.
pub mod history;
pub mod player;
pub mod settings;

pub use history::{Grouping, History};
pub use player::{Player, Preset, Strength};
pub use settings::Settings;
