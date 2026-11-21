//! The left column: disc counts with the side to move, the move number or
//! the result, and the record.
use super::{
    analysis_panel::disc,
    new_game::strength_label,
    root::RootView,
    theme,
    widgets::{self, card, heading},
};
use crate::{
    app::Command,
    domain::{Player, openings},
};
use deft_reversi_engine::{Action, Color, Position};
use gpui::{ClickEvent, Context, Div, FontWeight, SharedString, div, prelude::*, px};

/// The number of the move that leads to `position`'s next disc; it counts
/// discs, so it also works from an edited start.
pub(super) fn move_number(position: Position) -> usize {
    let discs = position.disc_count(Color::Black) + position.disc_count(Color::White);
    discs as usize - 3
}

impl RootView {
    pub(super) fn render_record(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let history = self.controller.history();
        let position = history.current();
        let finished = position.is_terminal();

        let players = self.controller.players();
        // One row per side: disc, count and who plays it. The side to move is
        // marked by a bar on the left and the brighter count.
        let row = |color: Color| {
            let to_move = !finished && position.side_to_move() == color;
            let name: SharedString = match players[if color == Color::Black { 0 } else { 1 }] {
                Player::Ai(strength) => {
                    format!("{} · {}", s.ai, strength_label(s, strength)).into()
                }
                Player::Human => s.human.into(),
            };
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .pl(px(8.))
                .py(px(2.))
                .border_l_2()
                .border_color(if to_move {
                    theme::accent()
                } else {
                    theme::background()
                })
                .child(disc(18., color))
                .child(
                    div()
                        .w(px(30.))
                        .text_xl()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if to_move {
                            theme::text()
                        } else {
                            theme::text_secondary()
                        })
                        .child(position.disc_count(color).to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .whitespace_nowrap()
                        .child(name),
                )
        };
        let state: SharedString = if finished {
            match position.final_score(Color::Black) {
                0 => s.draw.into(),
                d if d > 0 => s.wins_by(Color::Black, d).into(),
                d => s.wins_by(Color::White, -d).into(),
            }
        } else {
            s.move_number(move_number(position)).into()
        };

        let cursor = history.cursor();
        // Only a game from the standard start can follow a named opening.
        let opening = (history.position(0) == Position::initial())
            .then(|| openings::name(&history.actions()[..cursor]))
            .flatten();
        // In analysis mode, moves that lost enough are marked with the loss.
        let analyzer = self.controller.analyzer();
        let threshold = self.settings.analysis.loss_threshold;
        let analyzing = self.controller.analyzing();
        let copy_link = widgets::link(
            "copy-record",
            if self.record_copied {
                s.record_copied
            } else {
                s.copy
            },
            widgets::handler(|this, cx| this.copy_record(cx)),
            cx,
        )
        .text_xs();
        // In study mode the record shown is the line studied.
        let banner = self.controller.studying().then(|| {
            div()
                .mb(px(12.))
                .px(px(8.))
                .py(px(6.))
                .rounded(px(6.))
                .bg(theme::selected())
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::accent())
                        .child(s.studying),
                )
                .child(div().flex().child(widgets::link(
                    "back-to-main",
                    s.back_to_main,
                    widgets::handler(|this, cx| this.run(Command::SetStudy(false), cx)),
                    cx,
                )))
        });
        // The move about to be played is marked, and the board shows where;
        // at the end of the record, the last move played.
        let marked = if cursor < history.actions().len() {
            Some(cursor)
        } else {
            cursor.checked_sub(1)
        };
        let entries = history.actions().iter().enumerate().map(|(i, action)| {
            let current = marked == Some(i);
            let (number, name): (SharedString, SharedString) = match action {
                Action::Place(square) => (
                    move_number(history.position(i)).to_string().into(),
                    square.to_string().to_lowercase().into(),
                ),
                Action::Pass => ("".into(), s.pass.into()),
            };
            let mover = history.position(i).side_to_move();
            div()
                .id(("record", i))
                .w_1_2()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.))
                .px(px(4.))
                .py(px(1.))
                .rounded(px(3.))
                .cursor_pointer()
                .when(current, |el| el.bg(theme::selected()))
                .hover(|style| style.bg(theme::hover()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.run(Command::GoTo(i), cx)
                }))
                .child(
                    div()
                        .w(px(18.))
                        .flex()
                        .justify_end()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(number),
                )
                .child(disc(9., mover))
                .child(match (current, action) {
                    (true, _) => div()
                        .text_color(theme::accent())
                        .font_weight(FontWeight::MEDIUM)
                        .child(name),
                    (false, Action::Pass) => div().text_color(theme::text_muted()).child(name),
                    (false, _) => div().child(name),
                })
                .children(
                    analyzing
                        .then(|| analyzer.loss(history, i))
                        .flatten()
                        .filter(|&loss| loss >= threshold)
                        .map(|loss| {
                            div()
                                .text_xs()
                                .text_color(theme::danger())
                                .child(format!("−{loss}"))
                        }),
                )
        });

        div()
            .flex()
            .flex_col()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(row(Color::Black))
                    .child(row(Color::White)),
            )
            .child(
                div()
                    .mt(px(10.))
                    .pl(px(10.))
                    .mb(px(16.))
                    .text_color(theme::text_secondary())
                    .child(state),
            )
            .children(banner)
            .children(opening.map(|name| {
                div()
                    .mb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(heading(s.opening_heading))
                    .child(div().font_weight(FontWeight::MEDIUM).child(name))
            }))
            .child(
                card()
                    .flex_1()
                    .min_h_0()
                    .p(px(8.))
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .px(px(4.))
                            .mb(px(2.))
                            .child(heading(s.record_heading).flex_1())
                            .child(copy_link),
                    )
                    .child(
                        div()
                            .id("record-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .content_start()
                            .children(entries),
                    ),
            )
    }
}
