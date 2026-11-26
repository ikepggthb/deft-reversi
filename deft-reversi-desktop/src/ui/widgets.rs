//! ダイアログや設定画面・パネル間で共有する汎用 UI 部品（ボタン、トグル、ステッパー、スクロール領域等）を定義するモジュール。
use super::{root::RootView, theme};
use gpui::{
    ClickEvent, Context, Div, ElementId, FontWeight, MouseButton, MouseDownEvent, Pixels,
    ScrollHandle, SharedString, Stateful, div, point, prelude::*, px,
};
use std::rc::Rc;

/// `RootView` に対するクリック等のイベントハンドラ型。
pub(super) type Handler = Rc<dyn Fn(&mut RootView, &mut Context<RootView>)>;

/// クロージャを [`Handler`] へラップするヘルパー関数。
pub(super) fn handler(f: impl Fn(&mut RootView, &mut Context<RootView>) + 'static) -> Handler {
    Rc::new(f)
}

fn on_click(el: Stateful<Div>, f: Handler, cx: &mut Context<RootView>) -> Stateful<Div> {
    el.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
        f(this, cx);
        cx.notify();
    }))
}

/// 真偽値を切り替える 2 択のセグメントコントロールを生成する。
pub(super) fn toggle(
    id: &'static str,
    on_label: &'static str,
    off_label: &'static str,
    value: bool,
    on_change: impl Fn(&mut RootView, bool, &mut Context<RootView>) + 'static,
    cx: &mut Context<RootView>,
) -> Div {
    let on_change = Rc::new(on_change);
    let set_on = on_change.clone();
    segmented(
        id,
        vec![
            (
                on_label.into(),
                value,
                handler(move |this, cx| set_on(this, true, cx)),
            ),
            (
                off_label.into(),
                !value,
                handler(move |this, cx| on_change(this, false, cx)),
            ),
        ],
        cx,
    )
}

/// 選択中の項目を塗りつぶして表示する枠付きセグメントコントロールを生成する。
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
            let item = on_click(
                div()
                    .id((id, i))
                    .flex_1()
                    .flex()
                    .justify_center()
                    .px(px(10.))
                    .py(px(4.))
                    .whitespace_nowrap()
                    .cursor_pointer(),
                f,
                cx,
            )
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

/// クリック可能なテキストリンク要素を生成する。
pub(super) fn link(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    f: Handler,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    on_click(
        div()
            .id(id)
            .px(px(6.))
            .py(px(2.))
            .rounded(px(4.))
            .text_color(theme::text_secondary())
            .cursor_pointer()
            .hover(|style| style.bg(theme::hover()).text_color(theme::text())),
        f,
        cx,
    )
    .child(label.into())
}

/// `primary` と同じサイズの枠付き標準ボタンを生成する。
pub(super) fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    f: Handler,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    on_click(
        div()
            .id(id)
            .flex_none()
            .px(px(12.))
            .py(px(3.))
            .border_1()
            .border_color(theme::border())
            .rounded(px(5.))
            .whitespace_nowrap()
            .text_color(theme::text())
            .cursor_pointer()
            .hover(|style| style.bg(theme::hover())),
        f,
        cx,
    )
    .child(label.into())
}

/// ダイアログ内の主要な確定アクション用の塗りつぶしボタンを生成する。
pub(super) fn primary(
    id: &'static str,
    label: impl Into<SharedString>,
    f: Handler,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    on_click(
        div()
            .id(id)
            .flex_none()
            .px(px(14.))
            .py(px(3.))
            .border_1()
            .border_color(theme::text())
            .rounded(px(5.))
            .whitespace_nowrap()
            .bg(theme::text())
            .text_color(theme::background())
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer(),
        f,
        cx,
    )
    .child(label.into())
}

/// 左右に `−` / `+` ボタンを備えた数値ステッパーを生成する。
pub(super) fn stepper(
    id: &'static str,
    value: impl Into<SharedString>,
    down: Handler,
    up: Handler,
    cx: &mut Context<RootView>,
) -> Div {
    let button = |i: usize, glyph: &'static str, f: Handler, cx: &mut Context<RootView>| {
        on_click(
            div()
                .id((id, i))
                .px(px(9.))
                .py(px(3.))
                .text_color(theme::text_secondary())
                .cursor_pointer()
                .hover(|style| style.bg(theme::hover())),
            f,
            cx,
        )
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
                .min_w(px(64.))
                .px(px(8.))
                .flex()
                .justify_center()
                .font_weight(FontWeight::MEDIUM)
                .child(value.into()),
        )
        .child(button(1, "+", up, cx))
}

/// 薄い枠線と背景色を持つカードコンテナを生成する。
pub(super) fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .p(px(12.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::hairline())
        .rounded(px(8.))
}

/// カード内の小見出しを生成する。
pub(super) fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::text_secondary())
        .child(text.into())
}

/// 補足説明用の 1 行キャプションを生成する。
pub(super) fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .text_color(theme::text_muted())
        .child(text.into())
}

/// 設定画面やダイアログ内の「ラベル＋コントロール＋任意注記」の 1 行を生成する。
pub(super) fn form_row(
    label_width: f32,
    label: &'static str,
    control: Div,
    note: Option<SharedString>,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .min_h(px(34.))
        .child(div().w(px(label_width)).flex_none().child(label))
        .child(control.flex_none())
        .children(note.map(|n| caption(n).whitespace_nowrap()))
}

/// モーダルダイアログ共通の外枠コンテナを生成する。
pub(super) fn dialog_box(width: f32) -> Div {
    div()
        .w(px(width))
        .flex()
        .flex_col()
        .px(px(20.))
        .py(px(16.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border())
        .rounded(px(10.))
}

/// モーダルダイアログのタイトルバー（右端に任意の閉じるリンク付き）を生成する。
pub(super) fn dialog_header(
    title: &'static str,
    close: Option<(&'static str, &'static str)>,
    cx: &mut Context<RootView>,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .mb(px(10.))
        .child(
            div()
                .flex_1()
                .text_base()
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .children(
            close.map(|(id, label)| link(id, label, handler(|this, cx| this.close_dialog(cx)), cx)),
        )
}

/// スクロールバーのつまみドラッグ中に保持する開始位置状態。
pub(super) struct ScrollDrag {
    handle: ScrollHandle,
    start_y: Pixels,
    start_offset: f32,
}

/// 表示領域 `view` と全体長 `view + max` から、スクロールバーつまみの高さと移動可能範囲を計算する。
fn thumb(view: f32, max: f32) -> (f32, f32) {
    let height = (view * view / (view + max)).max(24.).min(view);
    (height, view - height)
}

/// 右端にドラッグ／クリック可能なカスタムスクロールバーを備えた縦スクロール領域を生成する。
///
/// GPUI の `ScrollHandle` は「直前の描画フレームでレイアウトされた領域サイズ」を保持するため、
/// ダイアログを開いた最初のフレームではスクロールバーの長さが確定せず、2 フレーム目から表示される。
pub(super) fn scroll_area(
    id: &'static str,
    handle: &ScrollHandle,
    content: Div,
    cx: &mut Context<RootView>,
) -> Div {
    let view = f32::from(handle.bounds().size.height);
    let max = f32::from(handle.max_offset().y);
    let offset = -f32::from(handle.offset().y);
    let bar = (max > 0.5 && view > 0.).then(|| {
        let (height, travel) = thumb(view, max);
        let top = travel * (offset / max).clamp(0., 1.);
        let thumb_handle = handle.clone();
        let track_handle = handle.clone();
        div()
            .id((id, 1usize))
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(10.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                    // トラック上のクリック位置が つまみの中央になるようスクロール位置を移動する。
                    let bounds = track_handle.bounds();
                    let view = f32::from(bounds.size.height);
                    let max = f32::from(track_handle.max_offset().y);
                    let (height, travel) = thumb(view, max);
                    let y = f32::from(event.position.y - bounds.top()) - height / 2.;
                    let offset = max * (y / travel.max(1.)).clamp(0., 1.);
                    track_handle.set_offset(point(px(0.), px(-offset)));
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id((id, 2usize))
                    .absolute()
                    .top(px(top))
                    .right(px(2.))
                    .w(px(6.))
                    .h(px(height))
                    .rounded(px(3.))
                    .bg(theme::border())
                    .hover(|style| style.bg(theme::text_muted()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.scroll_drag = Some(ScrollDrag {
                                handle: thumb_handle.clone(),
                                start_y: event.position.y,
                                start_offset: -f32::from(thumb_handle.offset().y),
                            });
                            cx.stop_propagation();
                        }),
                    ),
            )
    });
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        // 内部コンテンツに flex_none() を指定し、内容がはみ出すときに縮んで潰れるのではなく
        // 正しくスクロール領域として溢れるようにする。
        .child(
            div()
                .id(id)
                .track_scroll(handle)
                .overflow_y_scroll()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .pr(px(14.))
                .child(content.flex_none()),
        )
        .children(bar)
}

impl RootView {
    /// スクロールバーつまみのドラッグ中にポインタの Y 座標へ追従してスクロール位置を更新する。
    pub(super) fn drag_scroll(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = &self.scroll_drag else {
            return;
        };
        let view = f32::from(drag.handle.bounds().size.height);
        let max = f32::from(drag.handle.max_offset().y);
        let (_, travel) = thumb(view, max);
        let moved = f32::from(y - drag.start_y) * max / travel.max(1.);
        let offset = (drag.start_offset + moved).clamp(0., max);
        drag.handle.set_offset(point(px(0.), px(-offset)));
        cx.notify();
    }
}
