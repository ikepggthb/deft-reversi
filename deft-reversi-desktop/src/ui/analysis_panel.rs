//! 解析モードの UI（右カラムの局面評価・最善手・読み筋・候補手・Book 一覧と、
//! 盤面下部の評価値推移グラフおよびリサイズグリップ）を描画するモジュール。
use super::{
    format::{action_text, reading_mark, reading_text, signed},
    record::move_number,
    root::RootView,
    theme,
    widgets::{self, caption, card, heading},
};
use crate::domain::settings::GRAPH_HEIGHTS;
use crate::{
    app::Command,
    services::engine::{Reading, Report},
};
use deft_reversi_book::Bound;
use deft_reversi_engine::{Action, Color};
use gpui::{
    Bounds, ClickEvent, Context, Div, FontWeight, MouseButton, MouseDownEvent, PathBuilder, Pixels,
    Rgba, SharedString, Window, canvas, div, fill, linear_color_stop, linear_gradient, point,
    prelude::*, px, rgb, size,
};

/// グラフ左側の縦軸目盛領域の幅（論理ピクセル）。
const GRAPH_SCALE: f32 = 40.;
/// グラフ下部の手数横軸領域の高さ（論理ピクセル）。
const GRAPH_AXIS: f32 = 14.;
/// 大きな評価値表示に使用するフォントおよび行高さ。
const VALUE_FONT: &str = "Segoe UI";
const VALUE_LINE: f32 = 36.;
/// 凡例を省略するグラフ幅の閾値。
const GRAPH_LEGEND_WIDTH: f32 = 680.;
/// グラフカードの上下パディング・ヘッダ・横軸を含む固定高さ。
pub(super) const GRAPH_CHROME: f32 = 85. + GRAPH_READOUT + 6.;
/// グラフ見出し下の「指している手・評価値・読み深さ」行の高さ。
const GRAPH_READOUT: f32 = 16.;
/// グラフを最大まで広げたときでも盤面 1 マスに残す最小サイズ。
const MIN_CELL: f32 = 28.;
/// グラフ上端のリサイズグリップの高さ。
pub(super) const GRAPH_GRIP: f32 = 12.;

/// 評価値グラフの上下端レンジと目盛線の間隔を計算する。
///
/// 接戦時は ±4 まで自動拡大し、目盛線は常に偶数値になるよう調整する。
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

/// グラフ上に描画する 1 本の折れ線系列（検討モード時は本譜と検討手順の 2 本を描画する）。
pub(super) struct GraphLine {
    values: Vec<Option<i32>>,
    from: usize,
    color: Rgba,
}

/// GPUI キャンバス上に評価値グラフのグリッド線・折れ線・各手ドット・強調ドットを描画する。
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
        // 1 手のみの単独ポイントでも視認できるよう各点に小さなドットを描画する。
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

/// 左上から光が当たった立体的な石アイコンを描画する。
///
/// 枠線を使わずに、暗い背景上の黒石・明るい背景上の白石双方の視認性を保つためのグラデーション。
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
    /// 右カラムの局面解析カードを描画する。
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
            // 解析を途中で止めたあとや、やり直したいときに再解析できるようボタンを出す。
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

        // 現在手番側から見た評価値を表示する（候補手探索の完了結果がなければ一括解析キャッシュから補う）。
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
                        // 符号（+ / −）と数字でベースラインがずれないよう、全記号を含む単一フォントと固定行高を指定する。
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

        // 各手の評価値は読み終わり次第すぐに表示する。
        // いま読んでいる手は直前の反復イテレーションの暫定値を表示し、前のレベルの古い値は読み直されるまで薄く表示する。
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
                    Some(_) if m.fresh => theme::text_secondary(),
                    _ => theme::text_muted(),
                };
                let reading = m.value.as_ref().and_then(|v| {
                    let mark = reading_mark(s, v.reading?);
                    Some(if v.provisional {
                        format!("{mark} …")
                    } else {
                        mark
                    })
                });
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
                    }))
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

        // 現在の局面が Book に登録されていれば候補手を表示する（AI の使用条件を満たさない手は薄く表示する）。
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

    /// ドラッグ中または保存済みの評価値グラフ高さを返す（ウィンドウサイズに応じてクランプ）。
    pub(super) fn graph_height(&self) -> f32 {
        let height = f32::from(self.graph_live.unwrap_or(self.settings.graph_height));
        height
            .min(self.graph_most())
            .max(f32::from(*GRAPH_HEIGHTS.start()))
    }

    /// 現在のウィンドウ高さで盤面が最小サイズを保てる範囲内のグラフ最大高さを返す。
    fn graph_most(&self) -> f32 {
        let window = self.viewport.map_or(900., |(_, h)| h as f32);
        let board = 8. * MIN_CELL + 11.;
        let most = window - super::root::BOARD_CHROME - GRAPH_CHROME - GRAPH_GRIP - board;
        most.min(f32::from(*GRAPH_HEIGHTS.end()))
    }

    /// 盤面下部に配置する評価値推移グラフ（リサイズグリップ付き）を描画する。
    ///
    /// 検討モード時は本譜の線に加えて分岐地点から検討手順の線を重ねて描画する。
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
        // 検討モード中は本譜の評価値列と、本譜から枝分かれした開始手数の位置を求める。
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
        // 横軸は初手から終局（60 手）までを基準に確保し、対局が進むにつれて折れ線が右へ伸びる
        // （パスが多く 60 手を超える棋譜では自動で広がる）。
        let slots = count.max(61);
        let height = self.graph_height();
        // カード左右の余白・枠線と左端の縦軸目盛の幅を引いてプロット領域の幅を求める。
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

        // 縦軸は中央の 0 を基準に、上側が黒有利、下側が白有利を表す。
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

        // マウスが乗っている手（乗っていなければ現在表示中の手）の情報を上に表示する。
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
        // その評価値がどのレベル・深さ・選択率で読まれたかをグラフ右上に添える。
        let how = analyzer
            .analysis_at(history, k)
            .filter(|_| !history.position(k).is_terminal())
            .and_then(|a| {
                let reading = reading_text(s, a.report.as_ref()?.reading?);
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
        // グラフ幅が狭いときでも評価値が隠れないよう専用の 1 行に配置する。
        let readout = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .h(px(GRAPH_READOUT))
            .text_xs()
            .whitespace_nowrap()
            .overflow_hidden()
            .child(
                div()
                    .flex_none()
                    .text_color(theme::text_secondary())
                    .child(which),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme::text())
                    .child(value_text),
            )
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
            // 全手数の評価値が埋まったあとは、再解析（redo: true）ボタンとして機能する。
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

        // ウィンドウ幅が狭いときは凡例を先に隠し、次に読み上げ行を切り詰め、操作ボタンだけは常に残す。
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
        // 上にドラッグするとグラフが広がり、そのぶん盤面が自動で縮んで収まる。
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

    /// グラフ上端のリサイズグリップをドラッグ中にポインタ位置に合わせて高さを更新する。
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

    /// グラフのリサイズドラッグを終了し、確定した高さを設定へ保存する。
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
