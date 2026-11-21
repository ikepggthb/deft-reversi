//! Analysis mode (§9.2): on the right, the shown position's value for the
//! side to move, the best move and line, and every move's value; under the
//! board, the values over the record.
use super::{
    ai_panel::reading_text,
    record::move_number,
    root::RootView,
    theme,
    widgets::{self, caption, card, heading},
};
use crate::{
    app::Command,
    services::engine::{Reading, Report},
};
use deft_reversi_book::Bound;
use deft_reversi_engine::{Action, Color, Square};
use gpui::{
    Bounds, ClickEvent, Context, Div, FontWeight, PathBuilder, Pixels, Rgba, SharedString, Window,
    canvas, div, fill, linear_color_stop, linear_gradient, point, prelude::*, px, rgb, size,
};

/// The height of each half of the graph.
const GRAPH_HALF: f32 = 40.;
/// The value lines are drawn this far apart.
const GRAPH_STEP: i32 = 10;
/// The scale on the graph's left.
const GRAPH_SCALE: f32 = 40.;
/// The line of move numbers under the graph.
const GRAPH_AXIS: f32 = 14.;
/// Below this width the graph's heading leaves out the legend.
const GRAPH_LEGEND_WIDTH: f32 = 680.;

/// The value at the graph's top and bottom: the largest margin so far,
/// rounded up to a whole line, so the line fills the height it has.
fn graph_range(values: &[Option<i32>]) -> i32 {
    let largest = values.iter().flatten().map(|v| v.abs()).max().unwrap_or(0);
    ((largest + GRAPH_STEP - 1) / GRAPH_STEP).max(1) * GRAPH_STEP
}

/// The value lines, a faint mark every ten moves, the values as one line
/// (broken where a value is missing) and a dot on each marked position.
fn paint_graph(
    bounds: Bounds<Pixels>,
    slots: usize,
    range: i32,
    values: &[Option<i32>],
    marked: &[usize],
    window: &mut Window,
) {
    let slot = bounds.size.width / slots as f32;
    let middle = bounds.origin.y + px(GRAPH_HALF);
    let x = |k: usize| bounds.origin.x + slot * (k as f32 + 0.5);
    let y = |v: i32| middle - px(v as f32 / range as f32 * GRAPH_HALF);
    let rule = |window: &mut Window, top: Pixels, color: Rgba| {
        window.paint_quad(fill(
            Bounds::new(point(bounds.origin.x, top), size(bounds.size.width, px(1.))),
            color,
        ));
    };
    for v in (GRAPH_STEP..range).step_by(GRAPH_STEP as usize) {
        rule(window, y(v), theme::hairline());
        rule(window, y(-v), theme::hairline());
    }
    rule(window, middle, theme::border());
    for k in (10..slots).step_by(10) {
        window.paint_quad(fill(
            Bounds::new(
                point(x(k), bounds.origin.y),
                size(px(1.), bounds.size.height),
            ),
            theme::hairline(),
        ));
    }
    let mut path = PathBuilder::stroke(px(2.));
    let mut drawing = false;
    let mut any = false;
    for (k, value) in values.iter().enumerate() {
        match value {
            Some(v) => {
                let at = point(x(k), y(*v));
                if drawing {
                    path.line_to(at);
                    any = true;
                } else {
                    path.move_to(at);
                }
                drawing = true;
            }
            None => drawing = false,
        }
    }
    if any && let Ok(path) = path.build() {
        window.paint_path(path, theme::text_secondary());
    }
    for (i, &k) in marked.iter().enumerate() {
        let Some(Some(v)) = values.get(k) else {
            continue;
        };
        let r = px(3.5);
        let color = if i == 0 {
            theme::text()
        } else {
            theme::accent()
        };
        window.paint_quad(
            fill(
                Bounds::new(point(x(k) - r, y(*v) - r), size(r * 2., r * 2.)),
                color,
            )
            .corner_radii(r),
        );
    }
}

pub(super) fn action_text(action: Action) -> String {
    action.to_string().to_lowercase()
}

/// "+6", "−6" or "0".
pub(super) fn signed(value: i32) -> String {
    format!("{value:+}").replace('-', "−").replace("+0", "0")
}

/// A disc on the page, lit from the top left like a stone. The light side
/// keeps black readable on the dark page, and the shaded side keeps white
/// readable on the light one, without an outline.
pub(super) fn disc(size: f32, color: Color) -> Div {
    let (light, dark) = match color {
        Color::Black => (rgb(0x5a5a5a), rgb(0x0a0a0a)),
        Color::White => (rgb(0xffffff), rgb(0xbdbdbd)),
    };
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(linear_gradient(
            135.,
            linear_color_stop(light, 0.),
            linear_color_stop(dark, 0.75),
        ))
}

impl RootView {
    pub(super) fn render_analysis(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let c = &self.controller;
        let history = c.history();
        let position = history.current();
        let side = position.side_to_move();
        let analyzer = c.analyzer();
        let report: Option<&Report> = analyzer
            .live(position)
            .or_else(|| analyzer.get(position).map(|a| &a.report));

        let title = div()
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::MEDIUM)
                    .child(s.analysis_heading),
            )
            .when(!position.is_terminal() && !position.must_pass(), |el| {
                el.child(widgets::button(
                    "exact",
                    s.exact_solve,
                    widgets::handler(|this, cx| this.run(Command::AnalyzeExact, cx)),
                    cx,
                ))
            });
        if position.is_terminal() {
            return card()
                .gap(px(8.))
                .child(title)
                .child(caption(s.game_finished));
        }

        // The value for the side to move.
        let value = report.and_then(|r| r.score).or_else(|| {
            analyzer
                .black_value(history, history.cursor())
                .map(|v| match side {
                    Color::Black => v,
                    Color::White => -v,
                })
        });
        let value_block = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(heading(s.value_label))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(disc(20., side))
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::MEDIUM)
                            .when(value.is_none(), |el| el.text_color(theme::text_muted()))
                            .child(value.map_or("–".into(), signed)),
                    ),
            );

        let line = report.filter(|r| !r.pv.is_empty()).map(|r| {
            let rest: Vec<String> =
                r.pv.iter()
                    .skip(1)
                    .take(8)
                    .map(|&a| action_text(a))
                    .collect();
            let field = |label: &'static str, value: Div| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(heading(label))
                    .child(value)
            };
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(28.))
                        .child(field(
                            s.best_move,
                            div()
                                .text_lg()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::warning())
                                .child(action_text(r.pv[0])),
                        ))
                        .children(r.reading.map(|reading| {
                            field(
                                s.reading_label,
                                div()
                                    .text_color(theme::text_secondary())
                                    .child(reading_text(s, reading)),
                            )
                        })),
                )
                .when(!rest.is_empty(), |el| {
                    el.child(field(
                        s.expected_line,
                        div()
                            .text_color(theme::text_secondary())
                            .child(rest.join(" ")),
                    ))
                })
        });

        // Moves appear one by one as the search scores them; the rest wait
        // below. A finished search shows its list until the next one has one.
        let searching = analyzer.is_searching(position);
        let moves = analyzer
            .live(position)
            .map(|r| &r.moves)
            .filter(|m| !m.is_empty())
            .or_else(|| analyzer.get(position).map(|a| &a.report.moves));
        let candidates = moves.map(|moves| {
            let list =
                if moves.is_empty() {
                    div().children((!searching).then(|| caption(s.moves_incomplete)))
                } else {
                    let best_score = moves[0].score;
                    // Moves the running level has not reached show the lower
                    // level's value, faintly, in that order.
                    let earlier =
                        |square: Square| {
                            analyzer.get(position)?.report.moves.iter().find_map(|m| {
                                (m.action == Action::Place(square)).then_some(m.score)
                            })
                        };
                    let mut waiting: Vec<(Square, Option<i32>)> = position
                        .legal_moves()
                        .filter(|&square| !moves.iter().any(|m| m.action == Action::Place(square)))
                        .map(|square| (square, earlier(square)))
                        .collect();
                    waiting.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
                    div()
                        .flex()
                        .flex_col()
                        .children(moves.iter().enumerate().map(|(i, m)| {
                            let color: Rgba = if m.score == best_score {
                                theme::warning()
                            } else if i < 3 {
                                theme::text()
                            } else {
                                theme::text_secondary()
                            };
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .h(px(22.))
                                .text_color(color)
                                .child(div().w(px(36.)).child(action_text(m.action)))
                                .child(div().w(px(40.)).child(signed(m.score)))
                        }))
                        .children(waiting.into_iter().map(|(square, score)| {
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .h(px(22.))
                                .text_color(theme::text_muted())
                                .child(div().w(px(36.)).child(action_text(Action::Place(square))))
                                .child(div().w(px(40.)).child(match score {
                                    Some(score) => signed(score),
                                    None if searching => "…".into(),
                                    None => "–".into(),
                                }))
                        }))
                };
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pt(px(10.))
                .border_t_1()
                .border_color(theme::hairline())
                .child(heading(s.candidates))
                .child(list.mt(px(4.)))
        });

        // The book's moves, when the position is in it; values the AI would
        // not play are faint.
        let book = self
            .book_lines(position)
            .filter(|lines| !lines.is_empty())
            .map(|lines| {
                let rows = lines.into_iter().map(|line| {
                    let value = line.value.map_or("–".into(), |v| match v.bound {
                        Bound::Point => signed(v.score),
                        Bound::Lower => format!("≥{}", signed(v.score)),
                        Bound::Upper => format!("≤{}", signed(v.score)),
                    });
                    let reading = line
                        .value
                        .and_then(|v| v.reading)
                        .map(|reading| match reading {
                            Reading::Depth {
                                depth,
                                percent: 100,
                            } => s.depth_mark(depth),
                            Reading::Depth { depth, percent } => {
                                format!("{} @ {percent}%", s.depth_mark(depth))
                            }
                            Reading::Endgame { percent: 100 } => s.exact_mark.into(),
                            Reading::Endgame { percent } => format!("{percent}%"),
                            Reading::Book => String::new(),
                        })
                        .unwrap_or_default();
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .h(px(22.))
                        .text_color(if line.usable {
                            theme::text()
                        } else {
                            theme::text_muted()
                        })
                        .child(div().w(px(36.)).child(action_text(line.action)))
                        .child(div().w(px(48.)).child(value))
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(theme::text_secondary())
                                .child(reading),
                        )
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .pt(px(10.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .child(heading(s.book))
                    .child(div().mt(px(4.)).flex().flex_col().children(rows))
            });
        card()
            .gap(px(12.))
            .child(title)
            .child(value_block)
            .children(line)
            .children(candidates)
            .children(book)
    }

    /// The values over the record, under the board and as wide as it.
    pub(super) fn render_graph(&self, width: Pixels, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let c = &self.controller;
        let history = c.history();
        let analyzer = c.analyzer();
        let count = history.actions().len() + 1;
        // The axis spans a whole game from the start, so the line grows
        // as it is played; a longer record with passes widens it.
        let slots = count.max(61);
        // The card's padding and border, and the scale on the left.
        let plot = width - px(26. + GRAPH_SCALE);
        let slot = plot / slots as f32;
        let values: Vec<Option<i32>> = (0..count)
            .map(|k| analyzer.black_value(history, k))
            .collect();
        let missing = values.iter().any(Option::is_none);
        let shown = self.graph_hover.filter(|&k| k < count);
        let range = graph_range(&values);
        let columns = (0..slots)
            .map(|k| {
                let column = div()
                    .id(("graph", k))
                    .w(slot)
                    .h_full()
                    .when(shown == Some(k), |el| el.bg(theme::hover()))
                    .when(k == history.cursor(), |el| el.bg(theme::selected()));
                if k < count {
                    column
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.run(Command::GoTo(k), cx)
                        }))
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.graph_hover = Some(k);
                            } else if this.graph_hover == Some(k) {
                                this.graph_hover = None;
                            }
                            cx.notify();
                        }))
                } else {
                    column
                }
            })
            .collect::<Vec<_>>();

        // The scale: Black ahead above the line, White below.
        let scale_mark = |color: Color| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(3.))
                .child(disc(8., color))
                .child(range.to_string())
        };
        let lines = {
            let values = values.clone();
            let marked: Vec<usize> = [Some(history.cursor()), shown]
                .into_iter()
                .flatten()
                .collect();
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_graph(bounds, slots, range, &values, &marked, window)
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let scale = div()
            .w(px(GRAPH_SCALE))
            .flex_none()
            .h(px(GRAPH_HALF * 2. + 1.))
            .flex()
            .flex_col()
            .justify_between()
            .text_xs()
            .text_color(theme::text_muted())
            .child(scale_mark(Color::Black))
            .child(div().pl(px(11.)).child("0"))
            .child(scale_mark(Color::White));
        let axis = div()
            .relative()
            .ml(px(GRAPH_SCALE))
            .w(plot)
            .h(px(GRAPH_AXIS))
            .text_xs()
            .text_color(theme::text_muted())
            .children((1..=slots / 10).map(|t| {
                let k = t * 10;
                div()
                    .absolute()
                    .top_0()
                    .left(slot * (k as f32 + 0.5) - px(8.))
                    .w(px(16.))
                    .flex()
                    .justify_center()
                    .child(k.to_string())
            }));

        // What the pointer is over, or else the shown position.
        let k = shown.unwrap_or(history.cursor());
        let which: SharedString = match k.checked_sub(1).map(|i| history.actions()[i]) {
            None => s.start_label.into(),
            Some(Action::Place(square)) => format!(
                "{} {}",
                s.move_number(move_number(history.position(k - 1))),
                action_text(Action::Place(square))
            )
            .into(),
            Some(Action::Pass) => s.pass.into(),
        };
        let readout = div()
            .flex()
            .flex_row()
            .gap(px(8.))
            .text_xs()
            .child(div().text_color(theme::text_secondary()).child(which))
            .child(
                div()
                    .text_color(theme::text())
                    .child(values[k].map_or("–".into(), |v| s.lead(v))),
            );
        // How that value was read: the level, the depth and the probability,
        // in the plot's top right corner.
        let how = analyzer
            .analysis_at(history, k)
            .filter(|_| !history.position(k).is_terminal())
            .and_then(|a| {
                let reading = reading_text(s, a.report.reading?);
                Some(match a.level {
                    Some(level) => format!("Lv {level} · {reading}"),
                    None => reading,
                })
            })
            .map(|text| {
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .px(px(6.))
                    .rounded(px(4.))
                    .bg(theme::surface())
                    .text_xs()
                    .text_color(theme::text_secondary())
                    .whitespace_nowrap()
                    .child(text)
            });

        let actions = match analyzer.batch_progress() {
            Some((done, total)) => Some(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("{} {done} / {total}", s.analyzing))
                    .child(widgets::button(
                        "stop-batch",
                        s.stop,
                        widgets::handler(|this, cx| this.run(Command::StopAnalysis, cx)),
                        cx,
                    )),
            ),
            // Once every value is there, the record can be read again.
            None => Some(div().child(widgets::button(
                "analyze-record",
                if missing {
                    s.analyze_record
                } else {
                    s.reanalyze_record
                },
                widgets::handler(move |this, cx| {
                    this.run(Command::AnalyzeRecord { redo: !missing }, cx)
                }),
                cx,
            ))),
        };

        // On a narrow window the legend gives way first, then the readout
        // is cut short; the button always keeps its room.
        let legend = (width >= px(GRAPH_LEGEND_WIDTH)).then(|| {
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .text_xs()
                .text_color(theme::text_muted())
                .child("↑")
                .child(disc(8., Color::Black))
                .child(s.black_ahead)
                .child(div().w(px(6.)))
                .child("↓")
                .child(disc(8., Color::White))
                .child(s.white_ahead)
        });
        card()
            .w(width)
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(14.))
                    .h(px(24.))
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::MEDIUM)
                            .child(s.graph_title),
                    )
                    .children(legend)
                    .child(readout.flex_1().min_w_0().overflow_hidden())
                    .children(actions.map(|el| el.flex_none())),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div().flex().flex_row().child(scale).child(
                            div()
                                .relative()
                                .w(plot)
                                .h(px(GRAPH_HALF * 2. + 1.))
                                .flex()
                                .flex_row()
                                .children(columns)
                                .child(lines)
                                .children(how),
                        ),
                    )
                    .child(axis),
            )
    }
}

#[cfg(test)]
mod graph_tests {
    use super::graph_range;

    #[test]
    fn the_range_rounds_the_largest_margin_up_to_a_line() {
        assert_eq!(graph_range(&[]), 10);
        assert_eq!(graph_range(&[Some(0), None]), 10);
        assert_eq!(graph_range(&[Some(4), Some(-10)]), 10);
        assert_eq!(graph_range(&[Some(3), Some(-26)]), 30);
        assert_eq!(graph_range(&[Some(64)]), 70);
    }
}
