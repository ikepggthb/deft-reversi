//! deft-reversi desktop. P0 prototype: a window with a playable board while
//! the engine loads in the background; see `docs/desktop-plan.md`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod board;
mod engine_loader;

use board::BoardView;
use gpui::{App, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use std::time::Instant;

fn main() {
    let launched = Instant::now();
    // Before the window, so the model reads while the platform starts.
    let model_path = engine_loader::find_model();
    if model_path.is_none() {
        eprintln!(
            "model not found: put {} in data/eval/ or set DEFT_REVERSI_MODEL",
            engine_loader::MODEL_FILE
        );
    }
    let loading = model_path.map(engine_loader::spawn);

    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(880.), px(640.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(480.), px(400.))),
            ..Default::default()
        };
        cx.open_window(options, |_, cx| {
            cx.new(|cx| BoardView::new(launched, loading, cx))
        })
        .expect("failed to open the main window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
