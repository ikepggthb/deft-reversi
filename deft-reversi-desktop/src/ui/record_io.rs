//! 棋譜文字列（例: `f5d6c3`）のコピー・入力ダイアログ、および盤面上の「パス」通知オーバーレイを担うモジュール。
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

/// 棋譜として有効な英数字のみを抽出する。
///
/// 日本語キーボード入力時の全角英数字（`０`〜`９`, `ａ`〜`ｚ`, `Ａ`〜`Ｚ`）も半角小文字へ正規化する。
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
    /// 現在の棋譜（パス除く）をクリップボードへコピーし、コピー完了表示タイマーを開始する。
    pub(super) fn copy_record(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        let text = transcript::to_text(self.controller.history().actions());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let token = self.record_copied.map_or(1, |n| n.wrapping_add(1));
        self.record_copied = Some(token);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            this.update(cx, |this, cx| {
                if this.record_copied == Some(token) {
                    this.record_copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Book 生成の起点手順を指定するために棋譜入力ダイアログを開く。
    pub(super) fn enter_book_record(&mut self, cx: &mut Context<Self>) {
        self.enter_record(false, cx);
        self.record_for_book = true;
        if let Some(record) = &self.book_work.record {
            let text = transcript::to_text(record.actions());
            self.record_insert(&text);
        }
    }

    /// 棋譜入力ダイアログを開く（`paste` が `true` ならクリップボード内容を初期挿入する）。
    pub(super) fn enter_record(&mut self, paste: bool, cx: &mut Context<Self>) {
        self.record_for_book = false;
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

    /// キャレット位置へ文字列を挿入する（全選択中なら置換）。
    ///
    /// 2 文字で 1 手を構成するため、奇数の 1 文字目は `record_pending` に保留し、
    /// 2 文字目が揃った時点で手単位の位置へ挿入する。
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

    /// 全選択、入力途中の 1 文字、またはキャレット前後の 1 手を削除する。
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

    /// 棋譜入力ダイアログ表示中のキー入力を処理する。
    pub(super) fn record_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let moves = self.record_moves();
        let ctrl = key.modifiers.control || key.modifiers.platform;
        match key.key.as_str() {
            "left" | "right" | "home" | "end" => {
                // カーソル移動時は入力途中の半端な 1 文字を破棄する。
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
                if self.record_all && !self.record_input.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.record_input.clone()));
                    if key.key == "x" {
                        self.record_clear();
                    }
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

    /// 棋譜チップクリック時にキャレットをその手の直後へ移動する。
    fn record_click(&mut self, caret: usize, cx: &mut Context<Self>) {
        self.record_caret = caret.min(self.record_moves());
        self.record_all = false;
        self.record_pending = None;
        cx.notify();
    }

    /// 入力された棋譜を読み込む（Book 起点入力時は Book ダイアログへ反映、通常時は一時停止状態で対局を置換）。
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
        if self.record_for_book {
            if history.current().is_terminal() {
                return;
            }
            self.record_for_book = false;
            self.book_work.record = Some(history);
            self.book_work.root = super::book_work::RootChoice::Record;
            self.dialog = Some(Dialog::BookWork);
            cx.notify();
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

    /// 棋譜入力モーダルダイアログを描画する。
    pub(super) fn render_record_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let parsed = transcript::parse(&self.record_input);
        let failed = parsed.as_ref().err().map(|&number| number - 1);
        let caret = self.record_caret;
        let all = self.record_all;
        // 入力途中の 1 文字（存在する場合）とキャレット縦線を描画するクロージャ。
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
        // 1 手ごとにチップ化し、長い棋譜が手の中で折り返されないようにする。
        // 打てない最初の手は赤色表示し、それ以降の手は薄く表示する。
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
        let ready = self.record_pending.is_none()
            && parsed.as_ref().is_ok_and(|h| {
                !h.actions().is_empty() && (!self.record_for_book || !h.current().is_terminal())
            });
        widgets::dialog_box(560.)
            .gap(px(12.))
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
                    // 最後の手より右側の余白をクリックした場合は、キャレットを末尾へ移動する。
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            let end = this.record_moves();
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

    /// 画面中央に「黒パス」「白パス」の通知オーバーレイを描画する（クリックまたは一定時間で消える）。
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
