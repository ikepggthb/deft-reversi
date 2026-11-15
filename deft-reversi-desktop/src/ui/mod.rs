//! GPUI views. They read the controller's state and send it commands.
mod ai_log;
mod ai_panel;
mod analysis_panel;
mod animation;
mod board;
mod editor;
mod files;
mod new_game;
mod record;
mod record_io;
mod root;
mod settings_view;
mod theme;
mod widgets;

pub use root::{RootView, bind_keys};
