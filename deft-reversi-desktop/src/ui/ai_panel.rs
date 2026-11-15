//! The right column: the analysis in analysis mode, and at the bottom a
//! line on what the engine is doing with the controls for it.
use super::{root::RootView, theme, widgets};
use crate::{
    app::{Command, EngineStatus},
    domain::editor,
    i18n::Strings,
    services::engine::{Reading, Report},
};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

pub(super) fn reading_text(s: &Strings, reading: Reading) -> String {
    match reading {
        Reading::Depth {
            depth,
            percent: 100,
        } => s.depth(depth),
        Reading::Depth { depth, percent } => format!("{} @ {percent}%", s.depth(depth)),
        Reading::Endgame { percent: 100 } => s.exact.into(),
        Reading::Endgame { percent } => format!("{} @ {percent}%", s.selective_endgame),
    }
}

pub(super) fn nodes_text(nodes: u64) -> String {
    match nodes {
        n if n >= 1_000_000_000 => format!("{:.1} G", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.1} M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.0} K", n as f64 / 1e3),
        n => n.to_string(),
    }
}

fn report_text(s: &Strings, report: &Report) -> String {
    let mut parts = Vec::new();
    if let Some(reading) = report.reading {
        parts.push(reading_text(s, reading));
    }
    parts.push(format!("{:.1} s", report.elapsed.as_secs_f64()));
    parts.push(nodes_text(report.nodes));
    let seconds = report.elapsed.as_secs_f64();
    if seconds > 0.05 {
        parts.push(format!(
            "{}/s",
            nodes_text((report.nodes as f64 / seconds) as u64)
        ));
    }
    parts.join(" · ")
}

impl RootView {
    pub(super) fn render_ai(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let c = &self.controller;
        let muted =
            |text: SharedString| div().text_xs().text_color(theme::text_muted()).child(text);

        let mut lines: Vec<gpui::AnyElement> = Vec::new();
        if !self.has_engine() {
            lines.push(muted(s.model_missing.into()).into_any_element());
        } else {
            match c.engine() {
                EngineStatus::Loading => lines.push(muted(s.ai_loading.into()).into_any_element()),
                EngineStatus::Unavailable(error) => {
                    lines.push(muted(format!("{}: {error}", s.ai_failed).into()).into_any_element())
                }
                EngineStatus::Ready => {}
            }
        }
        for notice in c
            .notice()
            .map(str::to_string)
            .into_iter()
            .chain(self.notice.as_ref().map(|n| n.to_string()))
        {
            lines.push(
                div()
                    .text_xs()
                    .text_color(theme::danger())
                    .child(notice)
                    .into_any_element(),
            );
        }
        if let Some(thinking) = c.thinking() {
            let detail = thinking.report.as_ref().map(|r| report_text(s, r));
            lines.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(div().size(px(6.)).rounded_full().bg(theme::thinking_dot()))
                            .child(format!("{} {}", s.color(thinking.color), s.thinking))
                            .child(div().flex_1())
                            .child(widgets::link(
                                "move-now",
                                s.move_now,
                                widgets::handler(|this, cx| this.run(Command::MoveNow, cx)),
                                cx,
                            )),
                    )
                    .children(detail.map(|d| muted(d.into())))
                    .into_any_element(),
            );
        }
        let position = c.history().current();
        if (c.has_ai() || c.analyzing()) && !editor::searchable(position) {
            lines.push(muted(s.too_few_discs.into()).into_any_element());
        }
        if c.analyzing() && c.analyzer().is_searching(position) {
            let detail = c.analyzer().live(position).map(|r| report_text(s, r));
            lines.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(div().size(px(6.)).rounded_full().bg(theme::thinking_dot()))
                            .child(s.analyzing)
                            .child(div().flex_1())
                            .child(widgets::link(
                                "stop-analysis",
                                s.stop,
                                widgets::handler(|this, cx| this.run(Command::StopAnalysis, cx)),
                                cx,
                            )),
                    )
                    .children(detail.map(|d| muted(d.into())))
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(c.analyzing().then(|| self.render_analysis(cx)))
            .child(div().flex_1())
            .children(lines)
    }
}
