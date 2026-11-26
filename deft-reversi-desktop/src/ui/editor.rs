//! 局面エディタ（盤面編集モード）の UI と状態管理を担うモジュール。
//!
//! 編集中は右カラムの AI パネルを隠し、左カラムに石配置ツールを、中央に編集用盤面を表示する。
//! 編集内容は下書き（[`EditorDraft`]）に隔離され、「ここから開始」または「キャンセル」まで
//! 本体の対局状態には影響しない。
use super::{
    analysis_panel::disc,
    board::{LABEL, labeled_board},
    root::RootView,
    theme, widgets,
};
use crate::{
    app::{Command, NewGame},
    domain::editor::{Caveat, EditorDraft, Pen},
};
use deft_reversi_engine::Color;
use gpui::{
    ClipboardItem, Context, Div, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    SharedString, div, prelude::*, px,
};

/// 局面エディタ開扉中の一時状態。
pub(super) struct EditorState {
    /// 編集中の盤面下書き。
    pub draft: EditorDraft,
    /// 現在選択中のペン（黒・白・消去）。
    pub pen: Pen,
    /// マウスドラッグ中に連続適用しているペン。
    painting: Option<Pen>,
    /// エディタを開く前に解析モードが有効だったか（キャンセル時に復元するため保持）。
    was_analyzing: bool,
    /// 貼り付け失敗などの一時通知メッセージ。
    notice: Option<SharedString>,
}

impl RootView {
    /// 現在の表示局面を初期値として局面エディタを開く。
    pub(super) fn open_editor(&mut self, cx: &mut Context<Self>) {
        if self.editor.is_some() {
            return;
        }
        let was_analyzing = self.controller.analyzing();
        if was_analyzing {
            self.run(Command::SetAnalysis(false), cx);
        }
        self.run(Command::Pause, cx);
        self.menu_open = false;
        self.dialog = None;
        self.editor = Some(EditorState {
            draft: EditorDraft::from_position(self.controller.history().current()),
            pen: Pen::Black,
            painting: None,
            was_analyzing,
            notice: None,
        });
        cx.notify();
    }

    /// 局面エディタを破棄して元の対局へ戻る（開く前に解析モードだった場合は再開する）。
    pub(super) fn cancel_editor(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.editor.take()
            && editor.was_analyzing
        {
            self.run(Command::SetAnalysis(true), cx);
        }
        cx.notify();
    }

    /// 下書き局面から、直近の新規対局設定のプレイヤー構成で新しい対局を開始する。
    fn start_from_draft(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        let form = self.settings.new_game;
        self.run(
            Command::NewGame(NewGame {
                players: [form.sides[0].player(), form.sides[1].player()],
                start: editor.draft.position(),
            }),
            cx,
        );
    }

    fn edit(&mut self, change: impl FnOnce(&mut EditorState)) {
        if let Some(editor) = self.editor.as_mut() {
            editor.notice = None;
            change(editor);
        }
    }

    /// エディタ上部のタイトルおよび「キャンセル」「ここから開始」バーを描画する。
    pub(super) fn render_editor_bar(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .child(div().font_weight(FontWeight::MEDIUM).child(s.edit_position))
            .child(div().flex_1())
            .child(widgets::button(
                "cancel-edit",
                s.cancel,
                widgets::handler(|this, cx| this.cancel_editor(cx)),
                cx,
            ))
            .child(widgets::primary(
                "start-edit",
                s.start_from_here,
                widgets::handler(|this, cx| this.start_from_draft(cx)),
                cx,
            ))
    }

    /// 左カラムのペン選択・手番切替・初期化・石数・注意点表示を描画する。
    pub(super) fn render_editor_tools(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let Some(editor) = &self.editor else {
            return div();
        };
        let draft = editor.draft;
        let pen = |id: usize, choice: Pen, label: &'static str, cx: &mut Context<Self>| {
            let swatch = match choice {
                Pen::Black => disc(20., Color::Black),
                Pen::White => disc(20., Color::White),
                Pen::Erase => div()
                    .size(px(20.))
                    .rounded_full()
                    .border_1()
                    .border_dashed()
                    .border_color(theme::text_muted()),
            };
            let on = editor.pen == choice;
            div()
                .id(("pen", id))
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .py(px(8.))
                .cursor_pointer()
                .when(on, |el| {
                    el.bg(theme::selected()).text_color(theme::accent())
                })
                .when(!on, |el| {
                    el.text_color(theme::text_secondary())
                        .hover(|style| style.bg(theme::hover()))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit(|e| e.pen = choice);
                    cx.notify();
                }))
                .child(swatch)
                .child(div().text_xs().child(label))
        };
        let side = widgets::segmented(
            "edit-side",
            [Color::Black, Color::White]
                .into_iter()
                .map(|color| {
                    (
                        SharedString::from(s.color(color)),
                        draft.side == color,
                        widgets::handler(move |this, _| this.edit(|e| e.draft.side = color)),
                    )
                })
                .collect(),
            cx,
        );
        let caveat: Option<SharedString> = editor.notice.clone().or_else(|| {
            draft.caveat().map(|c| match c {
                Caveat::Pass(side) => s.pass_start(side).into(),
                Caveat::Finished => s.finished_position.into(),
                Caveat::TooFewDiscs => s.too_few_discs.into(),
            })
        });
        let position = draft.position();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .border_1()
                    .border_color(theme::border())
                    .rounded(px(5.))
                    .overflow_hidden()
                    .child(pen(0, Pen::Black, s.color(Color::Black), cx))
                    .child(pen(1, Pen::White, s.color(Color::White), cx))
                    .child(pen(2, Pen::Erase, s.erase, cx)),
            )
            .child(
                div()
                    .mt(px(5.))
                    .text_xs()
                    .text_color(theme::text_muted())
                    .children(s.paint_hint.lines().map(|l| div().child(l))),
            )
            .child(
                div()
                    .mt(px(14.))
                    .mb(px(5.))
                    .text_xs()
                    .text_color(theme::text_secondary())
                    .child(s.side_to_move),
            )
            .child(side)
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_row()
                    .gap(px(4.))
                    .child(widgets::link(
                        "edit-initial",
                        s.initial_setup,
                        widgets::handler(|this, _| this.edit(|e| e.draft = EditorDraft::initial())),
                        cx,
                    ))
                    .child(widgets::link(
                        "edit-clear",
                        s.clear_all,
                        widgets::handler(|this, _| {
                            this.edit(|e| e.draft = EditorDraft::empty(e.draft.side))
                        }),
                        cx,
                    )),
            )
            .child(div().mt(px(20.)).flex().flex_row().gap(px(14.)).children(
                [Color::Black, Color::White].map(|color| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .child(disc(20., color))
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::MEDIUM)
                                .child(position.disc_count(color).to_string()),
                        )
                }),
            ))
            .children(caveat.map(|text| {
                div()
                    .mt(px(8.))
                    .text_xs()
                    .text_color(theme::warning())
                    .child(text)
            }))
    }

    /// ドラッグ配置・右クリック消去・テキストコピー/貼り付けを備えた編集用盤面を描画する。
    pub(super) fn render_editor_board(&self, cell: Pixels, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let Some(editor) = &self.editor else {
            return div();
        };
        let position = editor.draft.position();
        // 開始前に強制パス状態を確認できるよう、現在手番側の合法手ドットを表示する。
        let legal = position.legal_moves();
        // 通常の対局盤面と同じ回転・反転向きで表示する。
        let view = self.settings.board_view;
        let rows = (0..8u8).map(|y| {
            let squares = (0..8u8).map(|x| {
                let square = view.square_at(x, y);
                let disc = if position.black() & square.bit() != 0 {
                    Some(theme::black_disc())
                } else if position.white() & square.bit() != 0 {
                    Some(theme::white_disc())
                } else {
                    None
                };
                let paint = move |this: &mut Self, pen: Pen| {
                    this.edit(|e| {
                        e.painting = Some(pen);
                        e.draft.paint(square, pen);
                    })
                };
                div()
                    .id(("edit-square", usize::from(square.index())))
                    .size(cell)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme::board())
                    .cursor_crosshair()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            let pen = this.editor.as_ref().map_or(Pen::Black, |e| e.pen);
                            paint(this, pen);
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            paint(this, Pen::Erase);
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        let held = this.editor.as_ref().and_then(|e| e.painting);
                        match (event.pressed_button, held) {
                            (Some(_), Some(pen)) => {
                                let before = this.editor.as_ref().map(|e| e.draft);
                                paint(this, pen);
                                if this.editor.as_ref().map(|e| e.draft) != before {
                                    cx.notify();
                                }
                            }
                            // 盤面の外でマウスボタンが離された場合もドラッグ状態を解除する。
                            (None, Some(_)) => this.edit(|e| e.painting = None),
                            _ => {}
                        }
                    }))
                    .map(|el| match disc {
                        Some(color) => el.child(div().size(cell * 0.8).rounded_full().bg(color)),
                        None if legal.contains(square) => {
                            el.child(div().size(cell * 0.2).rounded_full().bg(theme::legal_dot()))
                        }
                        None => el,
                    })
            });
            div().flex().flex_row().gap(px(1.)).children(squares)
        });
        let frame = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .p(px(2.))
            .bg(theme::board_line())
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.edit(|e| e.painting = None)),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, _, _, _| this.edit(|e| e.painting = None)),
            )
            .children(rows);
        let text = editor.draft.text();
        labeled_board(view, cell, frame).child(
            div()
                .mt(px(10.))
                .ml(px(LABEL))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .flex_1()
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(4.))
                        .bg(theme::surface())
                        .text_xs()
                        .font_family("Consolas")
                        .text_color(theme::text_secondary())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(text.clone()),
                )
                .child(widgets::link(
                    "edit-copy",
                    s.copy,
                    widgets::handler(move |_, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                    }),
                    cx,
                ))
                .child(widgets::link(
                    "edit-paste",
                    s.paste,
                    widgets::handler(|this, cx| {
                        let pasted = cx.read_from_clipboard().and_then(|item| item.text());
                        let s = this.settings.lang.strings();
                        match pasted.as_deref().and_then(EditorDraft::parse) {
                            Some(draft) => this.edit(|e| e.draft = draft),
                            None => this.edit(|e| e.notice = Some(s.paste_failed.into())),
                        }
                    }),
                    cx,
                )),
        )
    }
}
