//! GPUI views. They read the controller's state and send it commands.
mod ai_panel;
mod board;
mod files;
mod new_game;
mod record;
mod root;
mod settings_view;
mod theme;
mod widgets;

pub use root::{RootView, bind_keys};
