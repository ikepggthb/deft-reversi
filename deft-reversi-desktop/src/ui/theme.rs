//! Colours by meaning. The light theme and OS following come in P6.
use gpui::{Rgba, rgb};

pub fn background() -> Rgba {
    rgb(0x1f2023)
}
pub fn text() -> Rgba {
    rgb(0xe8e8e8)
}
pub fn text_secondary() -> Rgba {
    rgb(0xa3a3a3)
}
pub fn text_muted() -> Rgba {
    rgb(0x6e6e6e)
}
pub fn hairline() -> Rgba {
    rgb(0x34363b)
}
pub fn hover() -> Rgba {
    rgb(0x2c2e33)
}
pub fn accent() -> Rgba {
    rgb(0x85b7eb)
}
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
pub fn focus_ring() -> Rgba {
    rgb(0xfac775)
}
pub fn surface() -> Rgba {
    rgb(0x27292e)
}
pub fn selected() -> Rgba {
    rgb(0x2b3f57)
}
pub fn border() -> Rgba {
    rgb(0x45484f)
}
pub fn danger() -> Rgba {
    rgb(0xf09595)
}
pub fn thinking_dot() -> Rgba {
    rgb(0x1d9e75)
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
pub fn candidate_text() -> Rgba {
    rgb(0xd8e8de)
}
