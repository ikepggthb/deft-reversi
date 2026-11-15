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

/// Only letters and digits make a record, so anything else is dropped.
fn record_chars(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars()
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
        self.record_input = if paste {
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .map(|text| record_chars(&text).collect())
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.open_dialog(Dialog::EnterRecord, cx);
    }

    pub(super) fn record_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.modifiers.control || key.modifiers.platform {
            if key.key == "v"
                && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
            {
                self.record_input.extend(record_chars(&text));
            }
        } else if key.key == "backspace" {
            self.record_input.pop();
        } else if let Some(typed) = &key.key_char {
            self.record_input.extend(record_chars(typed));
        }
        cx.notify();
    }

    /// The typed record replaces the game, with the same players; like any
    /// loaded game it opens paused.
    pub(super) fn load_record(&mut self, cx: &mut Context<Self>) {
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
        // Shown in pairs, one move each, so a long record wraps.
        let chars: Vec<char> = self.record_input.chars().collect();
        let shown = chars
            .chunks(2)
            .map(|pair| pair.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ");
        let status = match &parsed {
            _ if self.record_input.is_empty() => div(),
            Ok(history) => div()
                .text_color(theme::text_secondary())
                .child(s.moves(history.actions().len())),
            Err(number) => div()
                .text_color(theme::danger())
                .child(s.record_error(*number)),
        };
        let ready = parsed.as_ref().is_ok_and(|h| !h.actions().is_empty());
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
                    .map(|el| {
                        if self.record_input.is_empty() {
                            el.text_color(theme::text_muted()).child(s.record_example)
                        } else {
                            el.child(format!("{shown}▏"))
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
