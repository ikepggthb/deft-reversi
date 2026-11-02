//! GPUI views. They read the controller's state and send it commands.
mod board;
mod record;
mod root;
mod theme;

pub use root::{RootView, bind_keys};
