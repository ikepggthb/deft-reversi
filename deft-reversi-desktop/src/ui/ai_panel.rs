//! 右カラムの描画を担うモジュール。
//!
//! 解析モード中の局面解析パネルと、下部のエンジン稼働状況・思考中／解析中インジケータ
//! （および「今すぐ打つ」「停止」リンク）を表示する。
use super::{
    format::{action_text, nodes_text, reading_text},
    root::RootView,
    theme, widgets,
};
use crate::{
    app::{Command, EngineStatus},
    domain::editor,
    i18n::Strings,
    services::engine::Report,
};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

/// 探索レポートから「読み深さ · 経過時間 · ノード数 · NPS」の 1 行サマリ文字列を生成する。
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
    /// 右カラム全体（解析パネルおよびエンジンステータス行）を描画する。
    pub(super) fn render_ai(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let c = &self.controller;
        let muted =
            |text: SharedString| div().text_xs().text_color(theme::text_muted()).child(text);
        let activity = |label: SharedString,
                        detail: Option<String>,
                        link_id: &'static str,
                        link_label: &'static str,
                        command: Command,
                        cx: &mut Context<Self>| {
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
                        .child(label)
                        .child(div().flex_1())
                        .child(widgets::link(
                            link_id,
                            link_label,
                            widgets::handler(move |this, cx| this.run(command.clone(), cx)),
                            cx,
                        )),
                )
                .children(detail.map(|d| muted(d.into())))
        };

        let mut lines: Vec<Div> = Vec::new();
        if !self.has_engine() {
            lines.push(muted(s.model_missing.into()));
        } else {
            match c.engine() {
                EngineStatus::Loading => lines.push(muted(s.ai_loading.into())),
                EngineStatus::Unavailable(error) => {
                    lines.push(muted(format!("{}: {error}", s.ai_failed).into()))
                }
                EngineStatus::Ready => {}
                EngineStatus::Generating => lines.push(muted(s.book_generating.into())),
            }
        }
        for notice in c
            .notice()
            .map(str::to_string)
            .into_iter()
            .chain(self.notice.as_ref().map(|n| n.to_string()))
        {
            lines.push(div().text_xs().text_color(theme::danger()).child(notice));
        }
        if let Some(thinking) = c.thinking() {
            let detail = thinking.report.as_ref().map(|r| report_text(s, r));
            lines.push(activity(
                format!("{} {}", s.color(thinking.color), s.thinking).into(),
                detail,
                "move-now",
                s.move_now,
                Command::MoveNow,
                cx,
            ));
        }
        let position = c.history().current();
        if (c.has_ai() || c.analyzing()) && !editor::searchable(position) {
            lines.push(muted(s.too_few_discs.into()));
        }
        if c.analyzing() && c.analyzer().is_searching(position) {
            // 現在探索中の手・目標レベル・経過時間・ノード数を表示する。
            let reading = c.analyzer().reading_move(position).map(|(level, action)| {
                let level = level.map_or_else(|| s.exact_solve.to_string(), |l| format!("Lv {l}"));
                format!("{level} · {}", action_text(action))
            });
            let progress = c.analyzer().live(position).map(|r| report_text(s, r));
            let detail = match (reading, progress) {
                (Some(reading), Some(progress)) => Some(format!("{reading} · {progress}")),
                (reading, progress) => reading.or(progress),
            };
            lines.push(activity(
                s.analyzing.into(),
                detail,
                "stop-analysis",
                s.stop,
                Command::StopAnalysis,
                cx,
            ));
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
