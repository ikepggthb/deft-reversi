//! ダーク／ライト両テーマに対応した意味ベースの色パレット定義。
//!
//! 盤面・石・緑盤上のマーカー色はテーマに関わらず固定とし、周囲の UI 配色のみを切り替える。
use gpui::{Rgba, rgb};
use std::cell::Cell;

/// 現在の描画テーマモード。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// ダークモード。
    Dark,
    /// ライトモード。
    Light,
}

thread_local! {
    // ビューはすべてメインスレッドで描画されるため、`RootView::render` が毎フレーム先頭でモードを設定する。
    static MODE: Cell<Mode> = const { Cell::new(Mode::Dark) };
}

/// 現在のスレッドの描画テーマモードを設定する。
pub fn set_mode(mode: Mode) {
    MODE.with(|m| m.set(mode));
}

/// 現在のテーマモードに応じてダーク色またはライト色を選択する。
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
/// エディタの注意点などページ上の警告色。
pub fn warning() -> Rgba {
    pick(0xfac775, 0x8a5a00)
}
/// AI 思考中インジケータのドット色。
pub fn thinking_dot() -> Rgba {
    rgb(0x1d9e75)
}

// 盤面上の配色はダーク／ライト共通で固定。
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
/// 盤面上のキーボードカーソルおよび最善手ハイライトの枠線色。
pub fn focus_ring() -> Rgba {
    rgb(0xfac775)
}
/// 盤面候補手の下に表示する読みの深さ注記色。
pub fn candidate_mark() -> Rgba {
    rgb(0x9fc2ad)
}
pub fn candidate_text() -> Rgba {
    rgb(0xd8e8de)
}
/// 解析時の読み種別（通常探索／選択的終局／完全終局）を表す色。
pub fn reading_depth() -> Rgba {
    rgb(0x85b7eb)
}
pub fn reading_selective() -> Rgba {
    rgb(0xef9f27)
}
pub fn reading_exact() -> Rgba {
    rgb(0x1d9e75)
}
