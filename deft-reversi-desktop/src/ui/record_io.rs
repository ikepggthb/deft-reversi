//! The record as text: copied to the clipboard, typed or pasted back in.
//! Also the short "Black passes" over the board.
use super::{
    analysis_panel::disc,
    root::{Dialog, RootView},
    theme, widgets,
};
use crate::{
    app::Command,
    domain::{GameInfo, transcript},
    persistence::record::LoadedGame,
};
use deft_reversi_engine::Color;
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, div,
    prelude::*, px,
};
use std::time::Duration;

/// Only letters and digits make a record, so anything else is dropped;
/// full-width ones, as a Japanese keyboard may type them, count as the
/// plain ones.
fn record_chars(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars()
        .map(|c| match c {
            '０'..='９' | 'ａ'..='ｚ' | 'Ａ'..='Ｚ' => {
                char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c)
            }
            _ => c,
        })
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
}

impl RootView {
    /// The whole record, passes left out, such as `f5d6c3`.
    pub(super) fn copy_record(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        let text = transcript::to_text(self.controller.history().actions());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.record_copied = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            this.update(cx, |this, cx| {
                this.record_copied = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Opens the record dialog, with the clipboard's text when pasting.
    pub(super) fn enter_record(&mut self, paste: bool, cx: &mut Context<Self>) {
        self.record_input.clear();
        self.record_caret = 0;
        self.record_all = false;
        self.record_pending = None;
        if paste && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.record_insert(&text);
        }
        self.open_dialog(Dialog::EnterRecord, cx);
    }

    fn record_moves(&self) -> usize {
        self.record_input.len() / 2
    }

    fn record_clear(&mut self) {
        self.record_input.clear();
        self.record_caret = 0;
        self.record_all = false;
        self.record_pending = None;
    }

    /// Types `text` at the caret, in place of everything when it is all
    /// selected. Characters pair up into moves, so the moves after the
    /// caret keep their places; an odd one out waits for its pair.
    fn record_insert(&mut self, text: &str) {
        if self.record_all {
            self.record_clear();
        }
        for c in record_chars(text) {
            match self.record_pending.take() {
                None => self.record_pending = Some(c),
                Some(first) => {
                    let at = 2 * self.record_caret;
                    self.record_input.insert(at, c);
                    self.record_input.insert(at, first);
                    self.record_caret += 1;
                }
            }
        }
    }

    /// Takes out the selection, the half-typed move, or the whole move
    /// before or after the caret.
    fn record_delete(&mut self, forward: bool) {
        if self.record_all {
            self.record_clear();
        } else if self.record_pending.take().is_some() {
        } else if forward {
            if self.record_caret < self.record_moves() {
                let at = 2 * self.record_caret;
                self.record_input.replace_range(at..at + 2, "");
            }
        } else if self.record_caret > 0 {
            self.record_caret -= 1;
            let at = 2 * self.record_caret;
            self.record_input.replace_range(at..at + 2, "");
        }
    }

    pub(super) fn record_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let moves = self.record_moves();
        let ctrl = key.modifiers.control || key.modifiers.platform;
        match key.key.as_str() {
            "left" | "right" | "home" | "end" => {
                // Moving drops a half-typed move.
                self.record_pending = None;
                self.record_caret = match key.key.as_str() {
                    "home" => 0,
                    "end" => moves,
                    "left" if self.record_all => 0,
                    "right" if self.record_all => moves,
                    "left" => self.record_caret.saturating_sub(1),
                    _ => (self.record_caret + 1).min(moves),
                };
                self.record_all = false;
            }
            "backspace" => self.record_delete(false),
            "delete" => self.record_delete(true),
            "a" if ctrl => self.record_all = moves > 0,
            "c" | "x" if ctrl => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.record_input.clone()));
                if key.key == "x" {
                    self.record_clear();
                }
            }
            "v" if ctrl => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.record_insert(&text);
                }
            }
            _ if ctrl => return,
            _ => {
                if let Some(typed) = &key.key_char
                    && record_chars(typed).next().is_some()
                {
                    self.record_insert(typed);
                }
            }
        }
        cx.notify();
    }

    /// A click on a move puts the caret after it.
    fn record_click(&mut self, caret: usize, cx: &mut Context<Self>) {
        self.record_caret = caret.min(self.record_moves());
        self.record_all = false;
        self.record_pending = None;
        cx.notify();
    }

    /// The typed record replaces the game, with the same players; like any
    /// loaded game it opens paused.
    pub(super) fn load_record(&mut self, cx: &mut Context<Self>) {
        if self.record_pending.is_some() {
            return;
        }
        let Ok(history) = transcript::parse(&self.record_input) else {
            return;
        };
        if history.actions().is_empty() {
            return;
        }
        self.dialog = None;
        let game = LoadedGame {
            game: GameInfo::new(),
            players: self.controller.players(),
            history,
        };
        self.run(Command::Load(Box::new(game)), cx);
    }

    pub(super) fn render_record_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let parsed = transcript::parse(&self.record_input);
        let failed = parsed.as_ref().err().map(|&number| number - 1);
        let caret = self.record_caret;
        let all = self.record_all;
        // The caret, with the half-typed move before it.
        let typing = || {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(1.))
                .children(self.record_pending.map(|c| {
                    div()
                        .px(px(3.))
                        .text_color(theme::warning())
                        .child(c.to_string())
                }))
                .child(div().w(px(2.)).h(px(18.)).bg(theme::text()))
        };
        // One chip a move, so a long record wraps between moves; the first
        // move that cannot be played is red and the rest are faint.
        let chars: Vec<char> = self.record_input.chars().collect();
        let chips = chars.chunks(2).enumerate().map(|(i, pair)| {
            let color = match failed {
                Some(f) if i == f => theme::danger(),
                Some(f) if i > f => theme::text_muted(),
                _ => theme::text(),
            };
            let chip = div()
                .h(px(24.))
                .px(px(3.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .cursor_text()
                .text_color(color)
                .when(all, |el| el.bg(theme::selected()))
                .when(failed == Some(i), |el| {
                    el.border_1().border_color(theme::danger())
                })
                .child(pair.iter().collect::<String>())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.record_click(i + 1, cx)
                    }),
                );
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(2.))
                .child(chip)
                .when(!all && caret == i + 1, |el| el.child(typing()))
        });
        let status = match &parsed {
            _ if self.record_input.is_empty() => div(),
            Ok(history) => div()
                .text_color(theme::text_secondary())
                .child(s.moves(history.actions().len())),
            Err(number) => div()
                .text_color(theme::danger())
                .child(s.record_error(*number)),
        };
        let ready =
            self.record_pending.is_none() && parsed.as_ref().is_ok_and(|h| !h.actions().is_empty());
        div()
            .w(px(560.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(20.))
            .py(px(16.))
            .bg(theme::surface())
            .border_1()
            .border_color(theme::border())
            .rounded(px(10.))
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::MEDIUM)
                    .child(s.enter_record),
            )
            .child(
                div()
                    .min_h(px(72.))
                    .px(px(10.))
                    .py(px(8.))
                    .border_1()
                    .border_color(theme::focus_ring())
                    .rounded(px(6.))
                    .bg(theme::background())
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .content_start()
                    .items_center()
                    .gap(px(4.))
                    .font_family("Consolas")
                    // A click past the last move puts the caret at the end.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            let end = this.record_input.len();
                            this.record_click(end, cx)
                        }),
                    )
                    .when(caret == 0 && !all, |el| el.child(typing()))
                    .map(|el| {
                        if self.record_input.is_empty() && self.record_pending.is_none() {
                            el.child(
                                div()
                                    .text_color(theme::text_muted())
                                    .child(s.record_example),
                            )
                        } else {
                            el.children(chips)
                        }
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(status.flex_1().text_xs())
                    .child(widgets::button(
                        "record-cancel",
                        s.cancel,
                        widgets::handler(|this, cx| this.close_dialog(cx)),
                        cx,
                    ))
                    .child(
                        widgets::primary(
                            "record-load",
                            s.load,
                            widgets::handler(|this, cx| this.load_record(cx)),
                            cx,
                        )
                        .when(!ready, |el| el.opacity(0.4)),
                    ),
            )
    }

    /// "Black passes" in the middle of the window; a click anywhere or a
    /// moment ends it.
    pub(super) fn render_pass_notice(&self, color: Color, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lang.strings();
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.pass_notice = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .px(px(28.))
                    .py(px(16.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::border())
                    .rounded(px(10.))
                    .shadow_lg()
                    .child(disc(24., color))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::MEDIUM)
                            .child(s.passes(color)),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::record_chars;

    #[test]
    fn full_width_letters_and_digits_count_as_plain_ones() {
        let typed: String = record_chars("Ｆ５ ｄ６-c3").collect();
        assert_eq!(typed, "f5d6c3");
    }
}
