//! The board with its coordinates, and the record navigation below it.
use super::{root::RootView, theme};
use crate::app::Command;
use deft_reversi_engine::Square;
use gpui::{ClickEvent, Context, MouseButton, Pixels, Rgba, Window, div, prelude::*, px};

const LABEL: f32 = 18.;

impl RootView {
    pub(super) fn render_board(
        &self,
        cell: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let history = self.controller.history();
        let position = history.current();
        let legal = position.legal_moves();
        let last_move = history.last_move();
        let cursor = self
            .board_focus
            .is_focused(window)
            .then_some(self.board_cursor);

        let label = |text: String| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(theme::text_muted())
                .child(text)
        };
        let files = div()
            .flex()
            .flex_row()
            .pl(px(LABEL + 2.))
            .gap(px(1.))
            .children(
                (0..8u8).map(|x| label(((b'a' + x) as char).to_string()).w(cell).h(px(LABEL))),
            );
        let rows = (0..8u8).map(|y| {
            let squares = (0..8u8).map(|x| {
                let square = Square::from_xy(x, y).expect("x and y are 0..8");
                let disc = if position.black() & square.bit() != 0 {
                    Some(theme::black_disc())
                } else if position.white() & square.bit() != 0 {
                    Some(theme::white_disc())
                } else {
                    None
                };
                let mut el = div()
                    .id(("square", usize::from(square.index())))
                    .relative()
                    .size(cell)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme::board());
                if cursor == Some(square) {
                    el = el.border_2().border_color(theme::focus_ring());
                }
                if let Some(color) = disc {
                    el = el.child(disc_element(cell, color));
                    if last_move == Some(square) {
                        let dot = cell * 0.16;
                        el = el.child(
                            div()
                                .absolute()
                                .right(cell * 0.06)
                                .bottom(cell * 0.06)
                                .size(dot)
                                .rounded_full()
                                .bg(theme::last_move_dot()),
                        );
                    }
                } else if legal.contains(square) {
                    el = el
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::board_hover()))
                        .child(div().size(cell * 0.2).rounded_full().bg(theme::legal_dot()))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.run(Command::Play(square), cx)
                        }));
                }
                el
            });
            div().flex().flex_row().gap(px(1.)).children(squares)
        });
        let ranks = div()
            .flex()
            .flex_col()
            .pt(px(2.))
            .gap(px(1.))
            .children((1..=8).map(|rank| label(rank.to_string()).w(px(LABEL)).h(cell)));
        let frame = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .p(px(2.))
            .bg(theme::board_line())
            .children(rows);
        // A click plays without taking keyboard focus, so the arrow keys keep
        // moving through the record; Tab gives the board its focus.
        div()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(files)
            .child(div().flex().flex_row().child(ranks).child(frame))
    }

    pub(super) fn render_navigation(
        &self,
        cell: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let history = self.controller.history();
        let back = history.cursor() > 0;
        let forward = history.can_redo();
        let buttons = [
            ("first", "⇤", back, Command::First),
            ("back-2", "«", back, Command::Step(-2)),
            ("back-1", "‹", back, Command::Step(-1)),
            ("forward-1", "›", forward, Command::Step(1)),
            ("forward-2", "»", forward, Command::Step(2)),
            ("last", "⇥", forward, Command::Last),
        ];
        div()
            .flex()
            .flex_row()
            .justify_center()
            .gap(px(4.))
            .mt(px(10.))
            .pl(px(LABEL))
            .w(cell * 8.)
            .children(buttons.map(|(id, glyph, enabled, command)| {
                let el = div()
                    .id(id)
                    .w(px(36.))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .text_lg()
                    .child(glyph);
                if enabled {
                    el.text_color(theme::text_secondary())
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::hover()).text_color(theme::text()))
                        .on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.run(command, cx)),
                        )
                } else {
                    el.text_color(theme::text_muted())
                }
            }))
    }
}

fn disc_element(cell: Pixels, color: Rgba) -> impl IntoElement {
    div().size(cell * 0.8).rounded_full().bg(color)
}
