//! Game state without GPUI: the record of a game, its players and rules.
pub mod editor;
pub mod game;
pub mod history;
pub mod openings;
pub mod player;
pub mod settings;
pub mod transcript;

pub use game::GameInfo;
pub use history::{Grouping, History};
pub use player::{Player, Preset, Strength};
pub use settings::{BoardView, Settings};
