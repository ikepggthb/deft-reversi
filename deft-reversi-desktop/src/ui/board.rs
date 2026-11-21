//! The board with its coordinates, and the record navigation below it.
use super::{analysis_panel::signed, animation::Frame, root::RootView, theme, widgets};
use crate::{
    app::{Command, Status},
    services::engine::Reading,
};
use deft_reversi_engine::{Action, Color, Square};
use gpui::{
    ClickEvent, Context, Div, FontWeight, MouseButton, Pixels, Rgba, Window, div, prelude::*, px,
};

const LABEL: f32 = 18.;

impl RootView {
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
        // Going back through the record, the next move of the record is ringed.
        let next_move = history.actions()[history.cursor()..]
            .iter()
            .take(2)
            .find_map(|action| match action {
                Action::Place(square) => Some(*square),
                Action::Pass => None,
            });
        // While an AI plays this side the board takes no clicks; paused, the
        // user may play either side.
        let playable =
            self.controller.ai_to_move().is_none() || self.controller.status() == Status::Paused;
        // In analysis mode the candidate moves carry their values, for the
        // side to move; the best ones are highlighted.
        // Not while the AI is about to move: the values would be a hint.
        let ai_moving =
            self.controller.ai_to_move().is_some() && self.controller.status() == Status::Playing;
        // The board shows one level's values at a time: the last level read
        // through, or the first one while it is read. A move in the opening
        // book shows the book's value instead.
        let analysis = (self.controller.analyzing() && !ai_moving).then(|| {
            let analyzer = self.controller.analyzer();
            let mut values: Vec<(Action, i32, Option<Reading>)> = analyzer
                .get(position)
                .map(|a| &a.report)
                .filter(|r| !r.moves.is_empty())
                .or_else(|| analyzer.live(position))
                .map(|r| {
                    r.moves
                        .iter()
                        .map(|m| (m.action, m.score, r.reading))
                        .collect()
                })
                .unwrap_or_default();
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
        // How each value was read, in a few characters under it.
        let s = self.settings.lang.strings();
        let mark_of = |reading: Reading| match reading {
            Reading::Depth { depth, .. } => s.depth_mark(depth),
            Reading::Endgame { percent: 100 } => s.exact_mark.into(),
            Reading::Endgame { percent } => format!("{percent}%"),
            Reading::Book => s.book.into(),
        };
        let cursor = self
            .board_focus
            .is_focused(window)
            .then_some(self.board_cursor);

        let view = self.settings.board_view;
        let label = |text: String| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(theme::text_muted())
                .child(text)
        };
        // A line of squares shares a file or a rank, whichever way the
        // board is turned; its label is that one.
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
        let frame = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .p(px(2.))
            .bg(theme::board_line())
            .children(rows);
        // A click plays without taking keyboard focus, so the arrow keys keep
        // moving through the record; Tab gives the board its focus.
        div()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(files)
            .child(div().flex().flex_row().child(ranks).child(frame))
    }

    /// Moving through the record on the left, and the game's own controls
    /// (pause, resume, one AI move) on the right: both act on what the
    /// board shows.
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
        // The row stays one line as wide as the board: the buttons narrow
        // first, then the "Paused" note gives way.
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
            // As wide as the board, or wider and centred under it on a small one.
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

/// The navigation buttons' width bounds.
const NAV_MIN: f32 = 26.;
const NAV_MAX: f32 = 44.;

/// A rough width of a line of text, enough to share out a row: a full
/// width character takes its font size, any other a bit over half.
fn text_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { 0.6 * size } else { size })
        .sum()
}
