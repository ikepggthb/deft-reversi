//! Colours by meaning, for a dark and a light page (§9.4). The board, the
//! discs and the marks drawn on the green stay the same in both, so only
//! the page around them changes.
use gpui::{Rgba, rgb};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

thread_local! {
    // Views render on the main thread; the root sets this before each frame.
    static MODE: Cell<Mode> = const { Cell::new(Mode::Dark) };
}

pub fn set_mode(mode: Mode) {
    MODE.with(|m| m.set(mode));
}

fn pick(dark: u32, light: u32) -> Rgba {
    rgb(match MODE.with(Cell::get) {
        Mode::Dark => dark,
        Mode::Light => light,
    })
}

pub fn background() -> Rgba {
    pick(0x1f2023, 0xf6f6f3)
}
pub fn text() -> Rgba {
    pick(0xededed, 0x1f2023)
}
pub fn text_secondary() -> Rgba {
    pick(0xc4c4c4, 0x4a4d53)
}
pub fn text_muted() -> Rgba {
    pick(0x979797, 0x6f7277)
}
pub fn hairline() -> Rgba {
    pick(0x3a3c42, 0xdcdcd7)
}
pub fn hover() -> Rgba {
    pick(0x2c2e33, 0xe9e9e4)
}
pub fn accent() -> Rgba {
    pick(0x85b7eb, 0x1f63ad)
}
pub fn surface() -> Rgba {
    pick(0x27292e, 0xffffff)
}
pub fn selected() -> Rgba {
    pick(0x2b3f57, 0xd9e6f5)
}
pub fn border() -> Rgba {
    pick(0x45484f, 0xc6c6c1)
}
pub fn danger() -> Rgba {
    pick(0xf09595, 0xb3261e)
}
/// Notes such as the editor's caveats, on the page.
pub fn warning() -> Rgba {
    pick(0xfac775, 0x8a5a00)
}
pub fn thinking_dot() -> Rgba {
    rgb(0x1d9e75)
}

// On the board, the same in both modes.
pub fn board() -> Rgba {
    rgb(0x2e7d4f)
}
pub fn board_hover() -> Rgba {
    rgb(0x358a5a)
}
pub fn board_line() -> Rgba {
    rgb(0x1b4d30)
}
pub fn black_disc() -> Rgba {
    rgb(0x111111)
}
pub fn white_disc() -> Rgba {
    rgb(0xf4f4f4)
}
pub fn legal_dot() -> Rgba {
    rgb(0x85b7eb)
}
pub fn last_move_dot() -> Rgba {
    rgb(0xe24b4a)
}
/// The keyboard cursor and the best move, on the green.
pub fn focus_ring() -> Rgba {
    rgb(0xfac775)
}
pub fn candidate_text() -> Rgba {
    rgb(0xd8e8de)
}
/// The three kinds of reading in a level's plan (§9.5).
pub fn reading_depth() -> Rgba {
    rgb(0x85b7eb)
}
pub fn reading_selective() -> Rgba {
    rgb(0xef9f27)
}
pub fn reading_exact() -> Rgba {
    rgb(0x1d9e75)
}
