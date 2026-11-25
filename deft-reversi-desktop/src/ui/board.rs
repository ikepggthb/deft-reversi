//! 座標ラベル付きリバーシ盤面の描画と、盤面直下の棋譜ナビゲーション／対局操作バーを担うモジュール。
use super::{
    animation::Frame,
    format::{reading_mark, signed},
    root::RootView,
    theme, widgets,
};
use crate::{
    app::{Command, Status},
    services::engine::Reading,
};
use deft_reversi_engine::{Action, Color, Square};
use gpui::{
    ClickEvent, Context, Div, FontWeight, MouseButton, Pixels, Rgba, Window, div, prelude::*, px,
};

/// 盤面の外周に表示する座標ラベル列・行の幅/高さ（論理ピクセル）。
pub(super) const LABEL: f32 = 18.;

/// 8×8 の盤面フレームを [`BoardView`](crate::domain::BoardView) の向きに合わせた列（a〜h）・行（1〜8）ラベルで囲む。
pub(super) fn labeled_board(view: crate::domain::BoardView, cell: Pixels, frame: Div) -> Div {
    let label = |text: String| {
        div()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(theme::text_muted())
            .child(text)
    };
    // 盤面が回転・反転していても、同じ列・行に並ぶマスは必ず同じ筋（x）または段（y）を共有する。
    let line_label = |a: Square, b: Square| {
        if a.x() == b.x() {
            ((b'a' + a.x()) as char).to_string()
        } else {
            (a.y() + 1).to_string()
        }
    };
    let files = div()
        .flex()
        .flex_row()
        .pl(px(LABEL + 2.))
        .gap(px(1.))
        .children((0..8u8).map(|x| {
            label(line_label(view.square_at(x, 0), view.square_at(x, 1)))
                .w(cell)
                .h(px(LABEL))
        }));
    let ranks = div()
        .flex()
        .flex_col()
        .pt(px(2.))
        .gap(px(1.))
        .children((0..8u8).map(|y| {
            label(line_label(view.square_at(0, y), view.square_at(1, y)))
                .w(px(LABEL))
                .h(cell)
        }));
    div()
        .flex()
        .flex_col()
        .child(files)
        .child(div().flex().flex_row().child(ranks).child(frame))
}

impl RootView {
    /// 8×8 盤面（石・アニメーション・最終手ドット・次手リング・合法手・解析評価値・キーボードカーソル）を描画する。
    pub(super) fn render_board(
        &self,
        cell: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let history = self.controller.history();
        let position = history.current();
        let legal = position.legal_moves();
        let last_move = history.last_move();
        // 棋譜を遡って閲覧しているときは、次の手となるマスにリングを表示する。
        let next_move = history.actions()[history.cursor()..]
            .iter()
            .take(2)
            .find_map(|action| match action {
                Action::Place(square) => Some(*square),
                Action::Pass => None,
            });
        // 対局中の AI 手番では盤面クリックを無効化する（一時停止中はどちらの手番でも操作可能）。
        let playable =
            self.controller.ai_to_move().is_none() || self.controller.status() == Status::Paused;
        // AI の着手直前に評価値ヒントが見えてしまわないよう、AI 思考・対局中は盤面評価値を隠す。
        let ai_moving =
            self.controller.ai_to_move().is_some() && self.controller.status() == Status::Playing;
        // 解析モードでは各合法手の最新評価値と読み深さを表示し、Book に登録がある手は Book 値で上書きする。
        let analysis = (self.controller.analyzing() && !ai_moving).then(|| {
            let analyzer = self.controller.analyzer();
            let mut values: Vec<(Action, i32, Option<Reading>)> = analyzer
                .moves(position)
                .into_iter()
                .filter_map(|m| m.value)
                .map(|v| (v.action, v.score, v.reading))
                .collect();
            for line in self.book_lines(position).unwrap_or_default() {
                let Some(score) = line.value.and_then(|v| v.point()) else {
                    continue;
                };
                values.retain(|&(action, ..)| action != line.action);
                values.push((line.action, score, Some(Reading::Book)));
            }
            values
        });
        let best = analysis
            .as_ref()
            .and_then(|values| values.iter().map(|&(_, score, _)| score).max());
        let value_of = |square: Square| {
            analysis
                .as_ref()?
                .iter()
                .find_map(|&(action, score, reading)| {
                    (action == Action::Place(square)).then_some((score, reading))
                })
        };
        // 評価値の下に、どの深さ・精度で読まれたかを短い記号（例: d24@83%）で添える。
        let s = self.settings.lang.strings();
        let mark_of = |reading: Reading| reading_mark(s, reading);
        let cursor = self
            .board_focus
            .is_focused(window)
            .then_some(self.board_cursor);

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
                let mut el = div()
                    .id(("square", usize::from(square.index())))
                    .relative()
                    .size(cell)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme::board());
                if cursor == Some(square) {
                    el = el.border_2().border_color(theme::focus_ring());
                }
                if disc.is_none() && next_move == Some(square) {
                    el = el.child(
                        div()
                            .absolute()
                            .top(cell * 0.1)
                            .left(cell * 0.1)
                            .size(cell * 0.8)
                            .rounded_full()
                            .border_2()
                            .border_color(theme::focus_ring()),
                    );
                }
                if let Some(color) = disc {
                    el = el.child(
                        match self.animation.as_ref().and_then(|a| a.frame(square)) {
                            Some(Frame::Growing(size)) => {
                                div().size(cell * 0.8 * size).rounded_full().bg(color)
                            }
                            Some(Frame::Turning(width, shown)) => div()
                                .w(cell * 0.8 * width)
                                .h(cell * 0.8)
                                .rounded_full()
                                .bg(disc_color(shown)),
                            None => disc_element(cell, color),
                        },
                    );
                    if last_move == Some(square) {
                        let dot = cell * 0.16;
                        el = el.child(
                            div()
                                .absolute()
                                .right(cell * 0.06)
                                .bottom(cell * 0.06)
                                .size(dot)
                                .rounded_full()
                                .bg(theme::last_move_dot()),
                        );
                    }
                } else if playable && legal.contains(square) {
                    el = el
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::board_hover()))
                        .map(|el| {
                            match value_of(square).filter(|_| self.settings.candidate_values) {
                                Some((value, reading)) => {
                                    let exact = reading == Some(Reading::Endgame { percent: 100 });
                                    el.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .child(
                                                div()
                                                    .text_size(cell * 0.3)
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(if Some(value) == best {
                                                        theme::focus_ring()
                                                    } else {
                                                        theme::candidate_text()
                                                    })
                                                    .child(signed(value)),
                                            )
                                            .children(reading.map(mark_of).map(|mark| {
                                                div()
                                                    .text_size(cell * 0.16)
                                                    .line_height(cell * 0.18)
                                                    .when(exact, |el| {
                                                        el.font_weight(FontWeight::MEDIUM)
                                                    })
                                                    .text_color(if exact {
                                                        theme::candidate_text()
                                                    } else {
                                                        theme::candidate_mark()
                                                    })
                                                    .child(mark)
                                            })),
                                    )
                                }
                                None if self.settings.show_legal_moves || analysis.is_some() => el
                                    .child(
                                        div()
                                            .size(cell * 0.2)
                                            .rounded_full()
                                            .bg(theme::legal_dot()),
                                    ),
                                None => el,
                            }
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.run(Command::Play(square), cx)
                        }));
                }
                el
            });
            div().flex().flex_row().gap(px(1.)).children(squares)
        });
        let frame = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .p(px(2.))
            .bg(theme::board_line())
            .children(rows);
        // マウスで盤面をクリックしたときにキーボードフォーカスを盤面へ奪われないようにする。
        // （フォーカスが盤面へ移ると左右矢印キーが「棋譜送り」ではなく「盤面カーソル移動」に切り替わるため、
        // キーボード操作したいときだけ Tab キーで明示的に盤面へフォーカスする設計にしている。）
        labeled_board(view, cell, frame)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
    }

    /// 盤面直下のナビゲーションバー（左：初手・前後移動・終局、右：一時停止・再開・AI 1 手進め）を描画する。
    pub(super) fn render_navigation(
        &self,
        cell: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let c = &self.controller;
        let history = c.history();
        let back = history.cursor() > 0;
        let forward = history.can_redo();
        let buttons = [
            ("first", "⇤", back, Command::First),
            ("back-2", "«", back, Command::Step(-2)),
            ("back-1", "‹", back, Command::Step(-1)),
            ("forward-1", "›", forward, Command::Step(1)),
            ("forward-2", "»", forward, Command::Step(2)),
            ("last", "⇥", forward, Command::Last),
        ];
        // 盤面幅 1 行に収まるよう、幅が狭い場合はナビゲーションボタンを縮め、次に「一時停止中」ラベルを省略する。
        let position = history.current();
        let shows_controls = c.has_ai() && !position.is_terminal();
        let step_ai = c.status() == Status::Paused && c.ai_to_move().is_some();
        let row = f32::from(cell) * 8. + 11.;
        let button_width = |text: &str, padding: f32| text_width(text, 16.) + padding;
        let controls_width = |note: bool| match (shows_controls, c.status()) {
            (false, _) => 0.,
            (true, Status::Playing) => 10. + button_width(s.pause, 26.),
            (true, Status::Paused) => {
                10. + button_width(s.resume, 30.)
                    + if note {
                        6. + text_width(s.paused, 12.)
                    } else {
                        0.
                    }
                    + if step_ai {
                        6. + button_width(s.step_ai, 26.)
                    } else {
                        0.
                    }
            }
        };
        let fits = |note: bool| (row - controls_width(note)) / 6. >= NAV_MIN;
        let note = fits(true) || !fits(false);
        let nav_button = ((row - controls_width(note)) / 6.).clamp(NAV_MIN, NAV_MAX);
        let navigation =
            div()
                .flex()
                .flex_row()
                .flex_none()
                .border_1()
                .border_color(theme::border())
                .rounded(px(6.))
                .overflow_hidden()
                .children(buttons.into_iter().enumerate().map(
                    |(i, (id, glyph, enabled, command))| {
                        let el = div()
                            .id(id)
                            .w(px(nav_button.floor()))
                            .h(px(34.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xl()
                            .when(i > 0, |el| el.border_l_1().border_color(theme::hairline()))
                            .child(glyph);
                        if enabled {
                            el.text_color(theme::text())
                                .cursor_pointer()
                                .hover(|style| style.bg(theme::hover()))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.run(command.clone(), cx)
                                }))
                        } else {
                            el.text_color(theme::text_muted())
                        }
                    },
                ));
        let controls = shows_controls.then(|| match c.status() {
            Status::Playing => div().flex_none().child(widgets::button(
                "pause",
                s.pause,
                widgets::handler(|this, cx| this.run(Command::Pause, cx)),
                cx,
            )),
            Status::Paused => div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .when(note, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme::text_secondary())
                            .whitespace_nowrap()
                            .child(s.paused),
                    )
                })
                .when(c.ai_to_move().is_some(), |el| {
                    el.child(widgets::button(
                        "step-ai",
                        s.step_ai,
                        widgets::handler(|this, cx| this.run(Command::StepAi, cx)),
                        cx,
                    ))
                })
                .child(widgets::primary(
                    "resume",
                    s.resume,
                    widgets::handler(|this, cx| this.run(Command::Resume, cx)),
                    cx,
                )),
        });
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .mt(px(12.))
            .pl(px(LABEL))
            // 盤面と同じ幅に揃える（盤面が小さいときはボタンが収まる幅まで広げて中央に配置する）。
            .min_w(px(LABEL) + cell * 8. + px(7. + 4.))
            .map(|el| {
                if controls.is_some() {
                    el.justify_between()
                } else {
                    el.justify_center()
                }
            })
            .child(navigation)
            .children(controls)
    }
}

fn disc_element(cell: Pixels, color: Rgba) -> Div {
    div().size(cell * 0.8).rounded_full().bg(color)
}

fn disc_color(color: Color) -> Rgba {
    match color {
        Color::Black => theme::black_disc(),
        Color::White => theme::white_disc(),
    }
}

/// ナビゲーションボタンの最小・最大幅（論理ピクセル）。
const NAV_MIN: f32 = 26.;
const NAV_MAX: f32 = 44.;

/// レイアウト配分用の概算テキスト幅（全角はフォントサイズ、ASCII はその 0.6 倍として計算）。
fn text_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { 0.6 * size } else { size })
        .sum()
}
