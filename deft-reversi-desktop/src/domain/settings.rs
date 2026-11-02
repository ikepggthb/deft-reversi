//! What the settings screen changes. Saved to disk from P3.
use crate::{engine_loader, i18n::Lang, services::engine::Resources};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub lang: Lang,
    pub show_legal_moves: bool,
    pub resources: Resources,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::Ja,
            show_legal_moves: true,
            resources: engine_loader::default_resources(),
        }
    }
}

/// The table sizes the settings screen offers.
pub const TT_SIZES_MIB: [usize; 6] = [32, 64, 128, 256, 512, 1024];
