//! 現在の対局で AI が打った全手の探索ログ（手数・強さ・着手・評価値・読み・時間・ノード数・NPS・読み筋）を
//! 一覧表示するモーダルダイアログの描画モジュール。
use super::{
    analysis_panel::disc,
    format::{action_text, nodes_text, pv_tail, reading_text, signed, strength_label},
    root::RootView,
    theme, widgets,
};
use gpui::{Context, Div, FontWeight, SharedString, div, prelude::*, px, relative};

/// テーブル各列の固定幅（論理ピクセル）。残りの幅は読み筋（PV）列に割り当てる。
const NUMBER: f32 = 36.;
const COLOR: f32 = 26.;
const STRENGTH: f32 = 74.;
const MOVE: f32 = 40.;
const VALUE: f32 = 44.;
const READING: f32 = 110.;
const TIME: f32 = 56.;
const NODES: f32 = 64.;
const NPS: f32 = 64.;

/// 固定幅のテーブルセル要素を生成する。
fn cell(width: f32) -> Div {
    div().w(px(width)).flex_none().whitespace_nowrap()
}

impl RootView {
    /// AI ログモーダルダイアログを描画する。
    pub(super) fn render_ai_log(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let log = self.controller.ai_log();
        let header = div()
            .flex()
            .flex_row()
            .gap(px(8.))
            .pb(px(6.))
            .pr(px(14.))
            .border_b_1()
            .border_color(theme::hairline())
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme::text_secondary())
            .child(cell(NUMBER).child(s.log_number))
            .child(cell(COLOR))
            .child(cell(STRENGTH).child(s.log_strength))
            .child(cell(MOVE).child(s.log_move))
            .child(cell(VALUE).child(s.value_label))
            .child(cell(READING).child(s.log_reading))
            .child(cell(TIME).child(s.log_time))
            .child(cell(NODES).child(s.log_nodes))
            .child(cell(NPS).child("NPS"))
            .child(div().flex_1().child(s.expected_line));
        let rows: Vec<Div> = if log.is_empty() {
            vec![
                div()
                    .py(px(12.))
                    .text_color(theme::text_muted())
                    .child(s.no_ai_moves),
            ]
        } else {
            log.iter()
                .map(|entry| {
                    let report = &entry.report;
                    let seconds = report.elapsed.as_secs_f64();
                    let nps: SharedString = if seconds > 0.0005 {
                        nodes_text((report.nodes as f64 / seconds) as u64).into()
                    } else {
                        "–".into()
                    };
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .py(px(3.))
                        .border_b_1()
                        .border_color(theme::hairline())
                        .child(
                            cell(NUMBER)
                                .text_color(theme::text_muted())
                                .child(entry.number.to_string()),
                        )
                        .child(cell(COLOR).child(disc(12., entry.color)))
                        .child(
                            cell(STRENGTH)
                                .text_color(theme::text_secondary())
                                .child(strength_label(s, entry.strength)),
                        )
                        .child(
                            cell(MOVE)
                                .font_weight(FontWeight::MEDIUM)
                                .child(action_text(entry.action)),
                        )
                        .child(cell(VALUE).child(report.score.map_or("–".into(), signed)))
                        .child(
                            cell(READING)
                                .text_color(theme::text_secondary())
                                .child(report.reading.map_or("–".into(), |r| reading_text(s, r))),
                        )
                        .child(cell(TIME).child(format!("{seconds:.2} s")))
                        // Book から即答した手は探索ノード数が 0 のためハイフン表示にする。
                        .child(cell(NODES).child(if report.nodes == 0 {
                            "–".into()
                        } else {
                            nodes_text(report.nodes)
                        }))
                        .child(cell(NPS).child(nps))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_color(theme::text_secondary())
                                .child(pv_tail(&report.pv)),
                        )
                })
                .collect()
        };
        widgets::dialog_box(900.)
            .max_w(relative(0.94))
            .h(px(560.))
            .max_h(relative(0.9))
            .child(widgets::dialog_header(
                s.ai_log,
                Some(("close-ai-log", s.close)),
                cx,
            ))
            .child(header)
            .child(widgets::scroll_area(
                "ai-log-list",
                &self.dialog_scroll,
                div().flex().flex_col().text_xs().children(rows),
                cx,
            ))
    }
}
