//! The right column in analysis mode (§9.2): the value of the shown
//! position as the leading side's disc and margin, the best line, the
//! values over the record, and the candidate moves.
use super::{ai_panel::reading_text, root::RootView, theme, widgets};
use crate::{app::Command, services::engine::Report};
use deft_reversi_engine::{Action, Color};
use gpui::{ClickEvent, Context, Div, FontWeight, Rgba, SharedString, div, prelude::*, px};

const GRAPH_WIDTH: f32 = 220.;
const GRAPH_HALF: f32 = 34.;
/// Margins beyond this fill the graph's half height.
const GRAPH_RANGE: f32 = 32.;

pub(super) fn action_text(action: Action) -> String {
    action.to_string().to_lowercase()
}

/// A disc on the board's green, readable on the dark page.
pub(super) fn disc_badge(size: f32, color: Color) -> Div {
    let disc = match color {
        Color::Black => theme::black_disc(),
        Color::White => theme::white_disc(),
    };
    div()
        .size(px(size))
        .rounded(px(size * 0.15))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::board())
        .child(div().size(px(size * 0.7)).rounded_full().bg(disc))
}

impl RootView {
    pub(super) fn render_analysis(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let c = &self.controller;
        let history = c.history();
        let position = history.current();
        let analyzer = c.analyzer();
        let report: Option<&Report> = analyzer
            .live(position)
            .or_else(|| analyzer.get(position).map(|a| &a.report));

        // The value, as the leading side's disc and its margin.
        let value = analyzer.black_value(history, history.cursor()).or_else(|| {
            let score = analyzer.live(position)?.score?;
            Some(match position.side_to_move() {
                Color::Black => score,
                Color::White => -score,
            })
        });
        let headline = match value {
            Some(0) => div().child(div().text_3xl().font_weight(FontWeight::MEDIUM).child("0")),
            Some(v) => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(disc_badge(
                    24.,
                    if v > 0 { Color::Black } else { Color::White },
                ))
                .child(
                    div()
                        .text_3xl()
                        .font_weight(FontWeight::MEDIUM)
                        .child(v.abs().to_string()),
                ),
            None => div().text_3xl().text_color(theme::text_muted()).child("–"),
        };

        let line = report.filter(|r| !r.pv.is_empty()).map(|r| {
            let rest: Vec<String> =
                r.pv.iter()
                    .skip(1)
                    .take(7)
                    .map(|&a| action_text(a))
                    .collect();
            div()
                .flex()
                .flex_row()
                .gap(px(6.))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(action_text(r.pv[0])),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_secondary())
                        .child(rest.join(" ")),
                )
        });
        let reading = report.and_then(|r| r.reading).map(|r| {
            div()
                .text_xs()
                .text_color(theme::text_muted())
                .child(reading_text(s, r))
        });

        let candidates = report.map(|r| {
            if r.moves.is_empty() {
                let searching = analyzer.is_searching(position);
                return div().children((!searching).then(|| {
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(s.moves_incomplete)
                }));
            }
            let best_score = r.moves[0].score;
            div()
                .flex()
                .flex_col()
                .children(r.moves.iter().enumerate().map(|(i, m)| {
                    let best = m.score == best_score;
                    // The engine gives a line only for the best move.
                    let line: SharedString = if i == 0 {
                        r.pv.iter()
                            .skip(1)
                            .take(3)
                            .map(|&a| action_text(a))
                            .collect::<Vec<_>>()
                            .join(" ")
                            .into()
                    } else {
                        "".into()
                    };
                    let color: Rgba = if best {
                        theme::focus_ring()
                    } else if i < 3 {
                        theme::text()
                    } else {
                        theme::text_muted()
                    };
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .h(px(22.))
                        .child(
                            div()
                                .w(px(28.))
                                .text_color(color)
                                .child(action_text(m.action)),
                        )
                        .child(
                            div()
                                .w(px(36.))
                                .text_color(color)
                                .child(format!("{:+}", m.score).replace('-', "−")),
                        )
                        .child(div().text_xs().text_color(theme::text_muted()).child(line))
                }))
        });

        let actions = div().flex().flex_row().gap(px(4.)).when(
            !position.is_terminal() && !position.must_pass(),
            |el| {
                el.child(widgets::link(
                    "exact",
                    s.exact_solve,
                    widgets::handler(|this, cx| this.run(Command::AnalyzeExact, cx)),
                    cx,
                ))
            },
        );

        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(headline)
            .children(line)
            .children(reading)
            .child(self.render_graph(cx).mt(px(10.)))
            .child(div().mt(px(10.)).children(candidates))
            .child(actions.mt(px(6.)))
    }

    fn render_graph(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let c = &self.controller;
        let history = c.history();
        let analyzer = c.analyzer();
        let count = history.actions().len() + 1;
        let bar = (GRAPH_WIDTH / count as f32).floor().clamp(1., 6.);
        let values: Vec<Option<i32>> = (0..count)
            .map(|k| analyzer.black_value(history, k))
            .collect();
        let missing = values.iter().any(Option::is_none);
        let columns = values
            .iter()
            .enumerate()
            .map(|(k, value)| {
                let height = |v: i32| {
                    px((v.unsigned_abs() as f32).min(GRAPH_RANGE) / GRAPH_RANGE * GRAPH_HALF)
                };
                let fill = |el: Div| el.w_full().bg(theme::text_secondary());
                let up = value.filter(|&v| v > 0).map(|v| fill(div()).h(height(v)));
                let down = value.filter(|&v| v < 0).map(|v| fill(div()).h(height(v)));
                div()
                    .id(("graph", k))
                    .w(px(bar))
                    .h(px(GRAPH_HALF * 2. + 1.))
                    .flex()
                    .flex_col()
                    .cursor_pointer()
                    .when(k == history.cursor(), |el| el.bg(theme::selected()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.run(Command::GoTo(k), cx)
                    }))
                    .child(
                        div()
                            .h(px(GRAPH_HALF))
                            .flex()
                            .flex_col()
                            .justify_end()
                            .children(up),
                    )
                    .child(div().h(px(1.)).bg(theme::hairline()))
                    .child(div().h(px(GRAPH_HALF)).children(down))
            })
            .collect::<Vec<_>>();
        let batch = analyzer.batch_progress();
        let footer = match batch {
            Some((done, total)) => Some(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("{} {done} / {total}", s.analyzing))
                    .child(widgets::link(
                        "stop-batch",
                        s.stop,
                        widgets::handler(|this, cx| this.run(Command::StopAnalysis, cx)),
                        cx,
                    )),
            ),
            None if missing => Some(div().flex().flex_row().child(widgets::link(
                "analyze-record",
                s.analyze_record,
                widgets::handler(|this, cx| this.run(Command::AnalyzeRecord, cx)),
                cx,
            ))),
            None => None,
        };
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .justify_between()
                            .h(px(GRAPH_HALF * 2. + 1.))
                            .child(disc_badge(10., Color::Black))
                            .child(disc_badge(10., Color::White)),
                    )
                    .child(div().flex().flex_row().children(columns)),
            )
            .children(footer)
    }
}
