//! The new game dialog (§9.5): human or AI for each colour, the AI's
//! strength, and the start position.
use super::{root::RootView, theme, widgets};
use crate::{
    app::{Command, NewGame},
    domain::{Player, Preset, Strength, player::MOVE_TIMES},
    i18n::Strings,
};
use deft_reversi_engine::{Color, Position, ResultKind, Selectivity, level_reading};
use gpui::{ClickEvent, Context, Div, FontWeight, Rgba, SharedString, div, prelude::*, px};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiChoice {
    Preset(Preset),
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualMode {
    Level,
    Time,
}

#[derive(Clone, Copy, Debug)]
pub struct SideForm {
    pub ai: bool,
    pub choice: AiChoice,
    pub mode: ManualMode,
    pub level: u8,
    /// An index into `MOVE_TIMES`.
    pub time: usize,
}

impl SideForm {
    fn human() -> Self {
        Self {
            ai: false,
            choice: AiChoice::Preset(Preset::Intermediate),
            mode: ManualMode::Level,
            level: 20,
            time: 2,
        }
    }

    fn strength(&self) -> Strength {
        match (self.choice, self.mode) {
            (AiChoice::Preset(preset), _) => Strength::Preset(preset),
            (AiChoice::Manual, ManualMode::Level) => Strength::Level(self.level),
            (AiChoice::Manual, ManualMode::Time) => Strength::TimePerMove(MOVE_TIMES[self.time]),
        }
    }

    fn player(&self) -> Player {
        if self.ai {
            Player::Ai(self.strength())
        } else {
            Player::Human
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NewGameForm {
    /// Black, then White.
    pub sides: [SideForm; 2],
    pub from_current: bool,
}

impl Default for NewGameForm {
    fn default() -> Self {
        Self {
            sides: [
                SideForm::human(),
                SideForm {
                    ai: true,
                    ..SideForm::human()
                },
            ],
            from_current: false,
        }
    }
}

/// How a strength reads in a short label: a preset's name, a level, or a
/// time per move.
pub(super) fn strength_label(s: &Strings, strength: Strength) -> SharedString {
    match strength {
        Strength::Preset(preset) => s.preset(preset).into(),
        Strength::Level(level) => format!("Lv {level}").into(),
        Strength::TimePerMove(limit) => s.seconds(limit.as_secs()).into(),
    }
}

fn side_index(color: Color) -> usize {
    match color {
        Color::Black => 0,
        Color::White => 1,
    }
}

/// One stretch of empties that a level reads the same way.
struct Phase {
    from: u8,
    to: u8,
    reading: ResultKind,
}

fn phases(level: u8) -> Vec<Phase> {
    let mut phases: Vec<Phase> = Vec::new();
    for empties in (1..=60).rev() {
        let reading = level_reading(level, empties);
        match phases.last_mut() {
            Some(last) if last.reading == reading => last.to = empties,
            _ => phases.push(Phase {
                from: empties,
                to: empties,
                reading,
            }),
        }
    }
    phases
}

fn reading_color(reading: ResultKind) -> Rgba {
    match reading {
        ResultKind::Depth { .. } => theme::reading_depth(),
        ResultKind::Endgame {
            selectivity: Selectivity::Full,
        } => theme::reading_exact(),
        ResultKind::Endgame { .. } => theme::reading_selective(),
    }
}

/// Stronger pruning shows lighter.
fn reading_opacity(reading: ResultKind) -> f32 {
    let percent = f32::from(reading.selectivity().percent());
    0.45 + 0.55 * (percent - 68.) / 32.
}

const STRIP_WIDTH: f32 = 500.;

impl RootView {
    pub(super) fn render_new_game(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let form = self.new_game_form;
        let current = self.controller.history().current();
        let move_number = crate::ui::record::move_number(current);
        let start_items = vec![
            (
                s.initial_position.into(),
                !form.from_current,
                widgets::handler(|this, _| this.new_game_form.from_current = false),
            ),
            (
                s.current_position(move_number).into(),
                form.from_current,
                widgets::handler(|this, _| this.new_game_form.from_current = true),
            ),
        ];
        let start = widgets::handler(|this, cx| {
            let form = this.new_game_form;
            let start = if form.from_current {
                this.controller.history().current()
            } else {
                Position::initial()
            };
            this.close_dialog(cx);
            this.run(
                Command::NewGame(NewGame {
                    players: [form.sides[0].player(), form.sides[1].player()],
                    start,
                }),
                cx,
            );
        });
        let manual = [Color::Black, Color::White]
            .into_iter()
            .filter(|&color| {
                let side = form.sides[side_index(color)];
                side.ai && side.choice == AiChoice::Manual
            })
            .map(|color| self.render_manual(color, cx))
            .collect::<Vec<_>>();
        div()
            .w(px(580.))
            .flex()
            .flex_col()
            .bg(theme::surface())
            .border_1()
            .border_color(theme::border())
            .rounded(px(10.))
            .child(
                div()
                    .px(px(20.))
                    .pt(px(16.))
                    .text_base()
                    .font_weight(FontWeight::MEDIUM)
                    .child(s.new_game),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(20.))
                    .px(px(20.))
                    .pt(px(14.))
                    .child(self.render_side(Color::Black, cx))
                    .child(self.render_side(Color::White, cx)),
            )
            .children(manual)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(16.))
                    .px(px(20.))
                    .pt(px(16.))
                    .child(
                        div()
                            .text_color(theme::text_secondary())
                            .child(s.start_position),
                    )
                    .child(widgets::segmented("start-position", start_items, cx)),
            )
            .child(
                div()
                    .mt(px(16.))
                    .px(px(20.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .flex()
                    .flex_row()
                    .justify_end()
                    .items_center()
                    .gap(px(12.))
                    .child(widgets::link(
                        "cancel",
                        s.cancel,
                        widgets::handler(|this, cx| this.close_dialog(cx)),
                        cx,
                    ))
                    .child(widgets::primary("start", s.start_game, start, cx)),
            )
    }

    fn render_side(&self, color: Color, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let i = side_index(color);
        let side = self.new_game_form.sides[i];
        let disc = match color {
            Color::Black => theme::black_disc(),
            Color::White => theme::white_disc(),
        };
        let mut header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .mb(px(6.))
            .child(
                div()
                    .size(px(18.))
                    .rounded(px(3.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme::board())
                    .child(div().size(px(13.)).rounded_full().bg(disc)),
            )
            .child(div().font_weight(FontWeight::MEDIUM).child(s.color(color)));
        if color == Color::Black {
            header = header
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(s.first_player),
                )
                .child(div().flex_1())
                .child(widgets::link(
                    "swap",
                    format!("⇄ {}", s.swap),
                    widgets::handler(|this, _| this.new_game_form.sides.swap(0, 1)),
                    cx,
                ));
        }
        let kind = widgets::segmented(
            if i == 0 { "black-kind" } else { "white-kind" },
            vec![
                (
                    s.human.into(),
                    !side.ai,
                    widgets::handler(move |this, _| this.new_game_form.sides[i].ai = false),
                ),
                (
                    s.ai.into(),
                    side.ai,
                    widgets::handler(move |this, _| this.new_game_form.sides[i].ai = true),
                ),
            ],
            cx,
        );
        let options = side.ai.then(|| {
            let manual_summary: SharedString = match side.mode {
                ManualMode::Level => format!("Lv {}", side.level).into(),
                ManualMode::Time => s.seconds(MOVE_TIMES[side.time].as_secs()).into(),
            };
            let rows = Preset::ALL
                .into_iter()
                .map(|preset| {
                    (
                        AiChoice::Preset(preset),
                        SharedString::from(s.preset(preset)),
                        SharedString::from(format!("Lv {}", preset.level())),
                    )
                })
                .chain([(AiChoice::Manual, s.manual.into(), manual_summary)]);
            div()
                .mt(px(8.))
                .flex()
                .flex_col()
                .children(rows.enumerate().map(|(row, (choice, name, detail))| {
                    let on = side.choice == choice;
                    div()
                        .id((if i == 0 { "black-ai" } else { "white-ai" }, row))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .px(px(6.))
                        .py(px(3.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .when(on, |el| el.bg(theme::hover()))
                        .hover(|style| style.bg(theme::hover()))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.new_game_form.sides[i].choice = choice;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .size(px(12.))
                                .rounded_full()
                                .border_1()
                                .border_color(theme::border())
                                .when(on, |el| el.border_4().border_color(theme::accent())),
                        )
                        .child(div().flex_1().child(name))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(detail),
                        )
                }))
        });
        div()
            .flex_1()
            .flex()
            .flex_col()
            .child(header)
            .child(kind)
            .children(options)
    }

    fn render_manual(&self, color: Color, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let i = side_index(color);
        let side = self.new_game_form.sides[i];
        let id = |name: &'static str, black: &'static str| if i == 0 { black } else { name };
        let mode = widgets::segmented(
            id("white-mode", "black-mode"),
            vec![
                (
                    s.by_level.into(),
                    side.mode == ManualMode::Level,
                    widgets::handler(move |this, _| {
                        this.new_game_form.sides[i].mode = ManualMode::Level
                    }),
                ),
                (
                    s.by_time.into(),
                    side.mode == ManualMode::Time,
                    widgets::handler(move |this, _| {
                        this.new_game_form.sides[i].mode = ManualMode::Time
                    }),
                ),
            ],
            cx,
        );
        let body = match side.mode {
            ManualMode::Level => {
                let step = move |delta: i8| {
                    widgets::handler(move |this, _| {
                        let level = &mut this.new_game_form.sides[i].level;
                        *level = (*level as i8 + delta).clamp(1, 60) as u8;
                    })
                };
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(10.))
                            .child(widgets::stepper(
                                id("white-level", "black-level"),
                                format!("Lv {}", side.level),
                                step(-1),
                                step(1),
                                cx,
                            ))
                            .child(widgets::link(
                                id("white-level-down", "black-level-down"),
                                "−5",
                                step(-5),
                                cx,
                            ))
                            .child(widgets::link(
                                id("white-level-up", "black-level-up"),
                                "+5",
                                step(5),
                                cx,
                            )),
                    )
                    .child(self.render_phases(side.level))
            }
            ManualMode::Time => {
                let items = MOVE_TIMES
                    .iter()
                    .enumerate()
                    .map(|(t, limit)| {
                        (
                            s.seconds(limit.as_secs()).into(),
                            side.time == t,
                            widgets::handler(move |this, _| this.new_game_form.sides[i].time = t),
                        )
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().flex().child(widgets::segmented(
                        id("white-time", "black-time"),
                        items,
                        cx,
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_secondary())
                            .child(s.time_note),
                    )
            }
        };
        div()
            .mx(px(20.))
            .mt(px(12.))
            .p(px(12.))
            .rounded(px(6.))
            .bg(theme::background())
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::MEDIUM)
                            .child(s.manual_title(color)),
                    )
                    .child(mode),
            )
            .child(body)
    }

    /// The level's plan over the empties, as a strip and a table.
    fn render_phases(&self, level: u8) -> Div {
        let s = self.settings.lang.strings();
        let phases = phases(level);
        let strip = div()
            .flex()
            .flex_row()
            .h(px(10.))
            .w(px(STRIP_WIDTH))
            .rounded(px(3.))
            .overflow_hidden()
            .children(phases.iter().map(|phase| {
                let width = STRIP_WIDTH * f32::from(phase.from - phase.to + 1) / 60.;
                div()
                    .w(px(width))
                    .h_full()
                    .bg(reading_color(phase.reading))
                    .opacity(reading_opacity(phase.reading))
            }));
        let rows = phases.iter().map(|phase| {
            let range = if phase.from == phase.to {
                phase.from.to_string()
            } else {
                format!("{}–{}", phase.from, phase.to)
            };
            let (what, percent): (SharedString, SharedString) = match phase.reading {
                ResultKind::Depth { depth, selectivity } => (
                    s.depth(depth).into(),
                    if selectivity == Selectivity::Full {
                        s.no_pruning.into()
                    } else {
                        format!("{}%", selectivity.percent()).into()
                    },
                ),
                ResultKind::Endgame {
                    selectivity: Selectivity::Full,
                } => (s.exact.into(), "".into()),
                ResultKind::Endgame { selectivity } => (
                    s.selective_endgame.into(),
                    format!("{}%", selectivity.percent()).into(),
                ),
            };
            div()
                .flex()
                .flex_row()
                .items_center()
                .text_xs()
                .child(
                    div()
                        .w(px(64.))
                        .text_color(theme::text_muted())
                        .child(range),
                )
                .child(
                    div()
                        .size(px(8.))
                        .mr(px(6.))
                        .rounded(px(2.))
                        .bg(reading_color(phase.reading))
                        .opacity(reading_opacity(phase.reading)),
                )
                .child(div().w(px(150.)).child(what))
                .child(div().text_color(theme::text_secondary()).child(percent))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .mt(px(10.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .w(px(STRIP_WIDTH))
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(div().flex_1().child(format!("{} 60", s.empties)))
                    .child("1"),
            )
            .child(strip)
            .child(div().mt(px(6.)).flex().flex_col().children(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_cover_every_empty_count_in_order() {
        for level in 1..=60 {
            let phases = phases(level);
            assert_eq!(phases.first().map(|p| p.from), Some(60));
            assert_eq!(phases.last().map(|p| p.to), Some(1));
            assert!(phases.windows(2).all(|w| w[0].to == w[1].from + 1));
        }
        // Level 24: the opening correction, the midgame, three selective
        // stages, then the exact solve from 24 empties.
        let p = phases(24);
        assert_eq!(p.len(), 6);
        assert_eq!((p[5].from, p[5].to), (24, 1));
    }
}
