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
use crate::domain::settings::GRAPH_HEIGHTS;
use crate::{
    app::Command,
    i18n::Strings,
    services::engine::{Reading, Report},
};
use deft_reversi_book::Bound;
use deft_reversi_engine::{Action, Color};
use gpui::{
    Bounds, ClickEvent, Context, Div, FontWeight, MouseButton, MouseDownEvent, PathBuilder, Pixels,
    Rgba, SharedString, Window,
    canvas, div, fill, linear_color_stop, linear_gradient, point, prelude::*, px, rgb, size,
};

/// The scale on the graph's left.
const GRAPH_SCALE: f32 = 40.;
/// The line of move numbers under the graph.
const GRAPH_AXIS: f32 = 14.;
/// The value shown large, and the height of its line.
const VALUE_FONT: &str = "Segoe UI";
const VALUE_LINE: f32 = 36.;
/// Below this width the graph's heading leaves out the legend.
const GRAPH_LEGEND_WIDTH: f32 = 680.;
/// The graph's card around its plot: padding, heading and axis.
pub(super) const GRAPH_CHROME: f32 = 85. + GRAPH_READOUT + 6.;
/// The line under the heading: the position pointed at and its value.
const GRAPH_READOUT: f32 = 16.;
/// The smallest square the graph leaves the board when dragged.
const MIN_CELL: f32 = 28.;
/// The grab above the graph that sizes it.
pub(super) const GRAPH_GRIP: f32 = 12.;

/// The value at the graph's top and bottom, and how far apart its lines
/// are: it zooms in on a close game down to ±4, and the lines always fall
/// on even values.
fn graph_scale(largest: i32) -> (i32, i32) {
    let largest = largest.max(1);
    let step = match largest {
        ..=8 => 2,
        9..=20 => 4,
        21..=40 => 10,
        _ => 20,
    };
    let range = ((largest + step - 1) / step * step).clamp(4, 64);
    (range, step)
}

/// One line on the graph: the value after each action, from `from` on.
pub(super) struct GraphLine {
    values: Vec<Option<i32>>,
    from: usize,
    color: Rgba,
}

/// The value lines, a faint mark every ten moves, each line of values
/// (broken where a value is missing) with a dot on every move, and larger
/// dots on the marked positions.
fn paint_graph(
    bounds: Bounds<Pixels>,
    slots: usize,
    (range, step): (i32, i32),
    lines: &[GraphLine],
    marked: &[(usize, i32, Rgba)],
    window: &mut Window,
) {
    let half = bounds.size.height / 2.;
    let slot = bounds.size.width / slots as f32;
    let middle = bounds.origin.y + half;
    let x = |k: usize| bounds.origin.x + slot * (k as f32 + 0.5);
    let y = |v: i32| middle - half * (v as f32 / range as f32);
    let rule = |window: &mut Window, top: Pixels, color: Rgba| {
        window.paint_quad(fill(
            Bounds::new(point(bounds.origin.x, top), size(bounds.size.width, px(1.))),
            color,
        ));
    };
    for v in (step..range).step_by(step as usize) {
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
    let dot = |window: &mut Window, k: usize, v: i32, r: Pixels, color: Rgba| {
        window.paint_quad(
            fill(
                Bounds::new(point(x(k) - r, y(v) - r), size(r * 2., r * 2.)),
                color,
            )
            .corner_radii(r),
        );
    };
    for line in lines {
        let mut path = PathBuilder::stroke(px(2.));
        let mut drawing = false;
        let mut any = false;
        for (k, value) in line.values.iter().enumerate().skip(line.from) {
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
            window.paint_path(path, line.color);
        }
        // Every move played is a point, so a lone value shows too.
        for (k, value) in line.values.iter().enumerate().skip(line.from) {
            if let Some(v) = value {
                dot(window, k, *v, px(2.5), line.color);
            }
        }
    }
    for &(k, v, color) in marked {
        dot(window, k, v, px(4.), color);
    }
}

pub(super) fn action_text(action: Action) -> String {
    action.to_string().to_lowercase()
}

/// How a value was read, in a few characters: "d24@83%", "END@95%",
/// "END@100%" or "Book".
pub(super) fn reading_mark(s: &Strings, reading: Reading) -> String {
    match reading {
        Reading::Depth { depth, percent } => format!("d{depth}@{percent}%"),
        Reading::Endgame { percent } => format!("END@{percent}%"),
        Reading::Book => s.book.into(),
    }
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
        let report: Option<&Report> = analyzer.report(position);
        let searching = analyzer.is_searching(position);

        let title = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::MEDIUM)
                    .child(s.analysis_heading),
            )
            // Once a search is stopped, or to read it all again.
            .when(
                !searching && analyzer.get(position).is_some() && !position.must_pass(),
                |el| {
                    el.child(widgets::button(
                        "reanalyze",
                        s.reanalyze,
                        widgets::handler(|this, cx| this.run(Command::Reanalyze, cx)),
                        cx,
                    ))
                },
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
                    .h(px(VALUE_LINE))
                    .child(disc(20., side))
                    .child(
                        // One font with every sign, and a line of a set
                        // height: a sign taken from a fallback font would
                        // move the digits up or down against the disc.
                        div()
                            .font_family(VALUE_FONT)
                            .line_height(px(VALUE_LINE))
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

        // Each move's value appears as soon as it is read, with how it was
        // read; one still being read shows its last finished iteration.
        // Values from the level before are faint until read again.
        let shown = analyzer.moves(position);
        let any_value = shown.iter().any(|m| m.value.is_some());
        let candidates = (any_value || searching).then(|| {
            let best_score = shown
                .iter()
                .filter(|m| m.fresh)
                .filter_map(|m| m.value.as_ref())
                .filter(|v| !v.provisional)
                .map(|v| v.score)
                .max();
            let rows = shown.iter().enumerate().map(|(i, m)| {
                let color: Rgba = match &m.value {
                    Some(v) if m.fresh && !v.provisional && Some(v.score) == best_score => {
                        theme::warning()
                    }
                    Some(v) if m.fresh && !v.provisional && i < 3 => theme::text(),
                    Some(v) if m.fresh && !v.provisional => theme::text_secondary(),
                    Some(_) if m.fresh => theme::text_secondary(),
                    _ => theme::text_muted(),
                };
                let reading = m.value.as_ref().and_then(|v| {
                    let mark = reading_mark(s, v.reading?);
                    Some(if v.provisional { format!("{mark} …") } else { mark })
                });
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .h(px(22.))
                            .text_color(color)
                            .child(div().w(px(36.)).child(action_text(m.action)))
                            .child(div().w(px(44.)).child(match &m.value {
                                Some(v) => signed(v.score),
                                None if searching => "…".into(),
                                None => "–".into(),
                            }))
                            .children(reading.map(|r| {
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .whitespace_nowrap()
                                    .child(r)
                            })),
                    )
            });
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pt(px(10.))
                .border_t_1()
                .border_color(theme::hairline())
                .child(heading(s.candidates))
                .child(div().mt(px(4.)).flex().flex_col().children(rows))
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

    /// The plot's height: as it is being dragged, or as it was left.
    pub(super) fn graph_height(&self) -> f32 {
        let height = f32::from(self.graph_live.unwrap_or(self.settings.graph_height));
        height.min(self.graph_most()).max(f32::from(*GRAPH_HEIGHTS.start()))
    }

    /// The tallest the graph may be in this window: the board keeps squares
    /// large enough for the buttons under it.
    fn graph_most(&self) -> f32 {
        let window = self.viewport.map_or(900., |(_, h)| h as f32);
        let board = 8. * MIN_CELL + 11.;
        let most = window - super::root::BOARD_CHROME - GRAPH_CHROME - GRAPH_GRIP - board;
        most.min(f32::from(*GRAPH_HEIGHTS.end()))
    }

    /// The values over the record, under the board and as wide as it, with
    /// a grip above it to size it. In study mode the line studied is drawn
    /// over the record from where it leaves it.
    pub(super) fn render_graph(&self, width: Pixels, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let c = &self.controller;
        let history = c.history();
        let analyzer = c.analyzer();
        let values_of = |history: &crate::domain::History| -> Vec<Option<i32>> {
            (0..=history.actions().len())
                .map(|k| analyzer.black_value(history, k))
                .collect()
        };
        let shown_values = values_of(history);
        // The record, and where the study line leaves it.
        let main = c.main_line().map(|main| {
            let shared = (0..=main.actions().len().min(history.actions().len()))
                .take_while(|&k| main.position(k) == history.position(k))
                .last()
                .unwrap_or(0);
            (values_of(main), shared)
        });
        let count = shown_values
            .len()
            .max(main.as_ref().map_or(0, |(values, _)| values.len()));
        // The axis spans a whole game from the start, so the line grows
        // as it is played; a longer record with passes widens it.
        let slots = count.max(61);
        let height = self.graph_height();
        // The card's padding and border, and the scale on the left.
        let plot = width - px(26. + GRAPH_SCALE);
        let slot = plot / slots as f32;
        let shown_count = shown_values.len();
        let shown = self.graph_hover.filter(|&k| k < shown_count);
        let largest = shown_values
            .iter()
            .chain(main.iter().flat_map(|(values, _)| values))
            .flatten()
            .map(|v| v.abs())
            .max()
            .unwrap_or(0);
        let scale = graph_scale(largest);
        let columns = (0..slots)
            .map(|k| {
                let column = div()
                    .id(("graph", k))
                    .w(slot)
                    .h_full()
                    .when(shown == Some(k), |el| el.bg(theme::hover()))
                    .when(k == history.cursor(), |el| el.bg(theme::selected()));
                if k < shown_count {
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
                .child(scale.0.to_string())
        };
        let lines = {
            let mut lines = Vec::new();
            match &main {
                Some((values, shared)) => {
                    lines.push(GraphLine {
                        values: values.clone(),
                        from: 0,
                        color: theme::text_secondary(),
                    });
                    lines.push(GraphLine {
                        values: shown_values.clone(),
                        from: *shared,
                        color: theme::accent(),
                    });
                }
                None => lines.push(GraphLine {
                    values: shown_values.clone(),
                    from: 0,
                    color: theme::text_secondary(),
                }),
            }
            let marked: Vec<(usize, i32, Rgba)> = [
                (Some(history.cursor()), theme::text()),
                (shown, theme::accent()),
            ]
            .into_iter()
            .filter_map(|(k, color)| {
                let k = k?;
                Some((k, shown_values.get(k).copied()??, color))
            })
            .collect();
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_graph(bounds, slots, scale, &lines, &marked, window)
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let scale_labels = div()
            .w(px(GRAPH_SCALE))
            .flex_none()
            .h(px(height + 1.))
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
        let value_text = format!(
            "{}: {}",
            s.value_label,
            shown_values[k].map_or("–".into(), signed)
        );
        // How that value was read: the level, the depth and the probability,
        // on the right of the line above the plot.
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
                    .flex_none()
                    .text_color(theme::text_muted())
                    .child(text)
            });
        // Its own line, so a narrow graph still shows the value.
        let readout = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .h(px(GRAPH_READOUT))
            .text_xs()
            .whitespace_nowrap()
            .overflow_hidden()
            .child(div().flex_none().text_color(theme::text_secondary()).child(which))
            .child(div().flex_none().text_color(theme::text()).child(value_text))
            .child(div().flex_1())
            .children(how);

        let actions = match analyzer.batch_progress() {
            Some((done, total)) => div()
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
            // Once every value is there, the record can be read again.
            None => {
                let missing = shown_values.iter().any(Option::is_none);
                div().child(widgets::button(
                    "analyze-record",
                    s.analyze_record,
                    widgets::handler(move |this, cx| {
                        this.run(Command::AnalyzeRecord { redo: !missing }, cx)
                    }),
                    cx,
                ))
            }
        };

        // On a narrow window the legend gives way first, then the readout
        // is cut short; the button always keeps its room.
        let swatch = |color: Rgba, label: &'static str| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .child(div().w(px(12.)).h(px(2.)).bg(color))
                .child(label)
        };
        let legend = (width >= px(GRAPH_LEGEND_WIDTH)).then(|| {
            let legend = div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .text_xs()
                .text_color(theme::text_muted());
            if main.is_some() {
                legend
                    .child(swatch(theme::text_secondary(), s.main_line))
                    .child(div().w(px(6.)))
                    .child(swatch(theme::accent(), s.study_line))
            } else {
                legend
                    .child("↑")
                    .child(disc(8., Color::Black))
                    .child(s.black_ahead)
                    .child(div().w(px(6.)))
                    .child("↓")
                    .child(disc(8., Color::White))
                    .child(s.white_ahead)
            }
        });
        let card = card()
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
                    .child(div().flex_1())
                    .child(actions.flex_none()),
            )
            .child(readout.ml(px(GRAPH_SCALE)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div().flex().flex_row().child(scale_labels).child(
                            div()
                                .relative()
                                .w(plot)
                                .h(px(height + 1.))
                                .flex()
                                .flex_row()
                                .children(columns)
                                .child(lines),
                        ),
                    )
                    .child(axis),
            );
        // Dragged up, the graph grows and the board gives it room.
        let grip = div()
            .id("graph-grip")
            .w(width)
            .h(px(GRAPH_GRIP))
            .flex()
            .items_center()
            .justify_center()
            .cursor_row_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let height = this.graph_height() as u16;
                    this.graph_drag = Some((event.position.y, height));
                    this.graph_live = Some(height);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .w(px(36.))
                    .h(px(3.))
                    .rounded(px(2.))
                    .bg(theme::hairline()),
            );
        div().flex().flex_col().child(grip).child(card)
    }

    /// Follows the pointer while the graph's grip is held.
    pub(super) fn drag_graph(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let Some((start, height)) = self.graph_drag else {
            return;
        };
        let grown = f32::from(start - y);
        let height = (f32::from(height) + grown)
            .round()
            .min(self.graph_most())
            .max(f32::from(*GRAPH_HEIGHTS.start()));
        self.graph_live = Some(height as u16);
        cx.notify();
    }

    /// The graph keeps the height it was dragged to.
    pub(super) fn end_graph_drag(&mut self, cx: &mut Context<Self>) {
        if self.graph_drag.take().is_some()
            && let Some(height) = self.graph_live.take()
        {
            self.settings.graph_height = height;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod graph_tests {
    use super::graph_scale;

    #[test]
    fn a_close_game_is_zoomed_in_with_lines_on_even_values() {
        assert_eq!(graph_scale(0), (4, 2));
        assert_eq!(graph_scale(3), (4, 2));
        assert_eq!(graph_scale(5), (6, 2));
        assert_eq!(graph_scale(8), (8, 2));
        assert_eq!(graph_scale(9), (12, 4));
        assert_eq!(graph_scale(20), (20, 4));
        assert_eq!(graph_scale(26), (30, 10));
        assert_eq!(graph_scale(64), (64, 20));
        for largest in 0..=64 {
            let (range, step) = graph_scale(largest);
            assert!(range >= largest && range >= 4);
            assert_eq!(step % 2, 0);
        }
    }
}
