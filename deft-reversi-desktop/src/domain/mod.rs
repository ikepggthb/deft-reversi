//! GPUI に依存しない純粋なドメインモデル層。
//!
//! 対局の履歴・プレイヤー種別・設定・定跡名判定・盤面エディタの下書き・棋譜テキスト変換などを扱う。
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
