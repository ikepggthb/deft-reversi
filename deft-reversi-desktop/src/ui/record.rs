//! The left column: disc counts with the side to move, the move number or
//! the result, and the record.
use super::{root::RootView, theme};
use crate::app::Command;
use deft_reversi_engine::{Action, Color, Position};
use gpui::{ClickEvent, Context, Div, FontWeight, SharedString, div, prelude::*, px};

/// The number of the move that leads to `position`'s next disc; it counts
/// discs, so it also works from an edited start.
fn move_number(position: Position) -> usize {
    let discs = position.disc_count(Color::Black) + position.disc_count(Color::White);
    discs as usize - 3
}

impl RootView {
    pub(super) fn render_record(&self, cx: &mut Context<Self>) -> Div {
        let s = self.lang.strings();
        let history = self.controller.history();
        let position = history.current();
        let finished = position.is_terminal();

        let count = |color: Color, disc| {
            // The side to move is underlined instead of being named.
            let to_move = !finished && position.side_to_move() == color;
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .pb(px(2.))
                .border_b_2()
                .border_color(if to_move {
                    theme::text()
                } else {
                    theme::background()
                })
                // On the board's green, as on the board; a black disc on the
                // dark page would read as an outline.
                .child(
                    div()
                        .size(px(20.))
                        .rounded(px(3.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme::board())
                        .child(div().size(px(14.)).rounded_full().bg(disc)),
                )
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::MEDIUM)
                        .child(position.disc_count(color).to_string()),
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
        let entries = history.actions().iter().enumerate().map(|(i, action)| {
            let current = i + 1 == cursor;
            let (number, name): (SharedString, SharedString) = match action {
                Action::Place(square) => (
                    move_number(history.position(i)).to_string().into(),
                    square.to_string().to_lowercase().into(),
                ),
                Action::Pass => ("".into(), s.pass.into()),
            };
            div()
                .id(("record", i))
                .w_1_2()
                .flex()
                .flex_row()
                .gap(px(6.))
                .px(px(4.))
                .rounded(px(3.))
                .cursor_pointer()
                .hover(|style| style.bg(theme::hover()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.run(Command::GoTo(i + 1), cx)
                }))
                .child(
                    div()
                        .w(px(18.))
                        .text_color(theme::text_muted())
                        .child(number),
                )
                .child(match (current, action) {
                    (true, _) => div()
                        .text_color(theme::accent())
                        .font_weight(FontWeight::MEDIUM)
                        .child(name),
                    (false, Action::Pass) => div().text_color(theme::text_muted()).child(name),
                    (false, _) => div().child(name),
                })
        });

        div()
            .flex()
            .flex_col()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(14.))
                    .child(count(Color::Black, theme::black_disc()))
                    .child(count(Color::White, theme::white_disc())),
            )
            .child(
                div()
                    .mt(px(4.))
                    .mb(px(18.))
                    .text_color(theme::text_secondary())
                    .child(state),
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
            )
    }
}
