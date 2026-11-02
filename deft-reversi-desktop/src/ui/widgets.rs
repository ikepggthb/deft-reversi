//! Small controls shared by the dialogs.
use super::{root::RootView, theme};
use gpui::{ClickEvent, Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px};
use std::rc::Rc;

pub(super) type Handler = Rc<dyn Fn(&mut RootView, &mut Context<RootView>)>;

pub(super) fn handler(f: impl Fn(&mut RootView, &mut Context<RootView>) + 'static) -> Handler {
    Rc::new(f)
}

/// A framed row of choices with the chosen one filled.
pub(super) fn segmented(
    id: &'static str,
    items: Vec<(SharedString, bool, Handler)>,
    cx: &mut Context<RootView>,
) -> Div {
    div()
        .flex()
        .flex_row()
        .border_1()
        .border_color(theme::border())
        .rounded(px(5.))
        .overflow_hidden()
        .children(items.into_iter().enumerate().map(|(i, (label, on, f))| {
            let item = div()
                .id((id, i))
                .flex_1()
                .flex()
                .justify_center()
                .px(px(10.))
                .py(px(4.))
                .whitespace_nowrap()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    f(this, cx);
                    cx.notify();
                }))
                .child(label);
            if on {
                item.bg(theme::selected())
                    .text_color(theme::accent())
                    .font_weight(FontWeight::MEDIUM)
            } else {
                item.text_color(theme::text_secondary())
                    .hover(|style| style.bg(theme::hover()))
            }
        }))
}

/// Plain text that acts on click.
pub(super) fn link(
    id: &'static str,
    label: impl Into<SharedString>,
    f: Handler,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(6.))
        .py(px(2.))
        .rounded(px(4.))
        .text_color(theme::text_secondary())
        .cursor_pointer()
        .hover(|style| style.bg(theme::hover()).text_color(theme::text()))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            f(this, cx);
            cx.notify();
        }))
        .child(label.into())
}

/// The one filled button of a dialog.
pub(super) fn primary(
    id: &'static str,
    label: impl Into<SharedString>,
    f: Handler,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(16.))
        .py(px(6.))
        .rounded(px(5.))
        .bg(theme::text())
        .text_color(theme::background())
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            f(this, cx);
            cx.notify();
        }))
        .child(label.into())
}

/// A number with − and + on either side.
pub(super) fn stepper(
    id: &'static str,
    value: impl Into<SharedString>,
    down: Handler,
    up: Handler,
    cx: &mut Context<RootView>,
) -> Div {
    let button = |i: usize, glyph: &'static str, f: Handler, cx: &mut Context<RootView>| {
        div()
            .id((id, i))
            .px(px(9.))
            .py(px(3.))
            .text_color(theme::text_secondary())
            .cursor_pointer()
            .hover(|style| style.bg(theme::hover()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                f(this, cx);
                cx.notify();
            }))
            .child(glyph)
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .border_1()
        .border_color(theme::border())
        .rounded(px(5.))
        .overflow_hidden()
        .child(button(0, "−", down, cx))
        .child(
            div()
                .min_w(px(40.))
                .px(px(8.))
                .flex()
                .justify_center()
                .font_weight(FontWeight::MEDIUM)
                .child(value.into()),
        )
        .child(button(1, "+", up, cx))
}
