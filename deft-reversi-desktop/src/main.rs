//! deft-reversi desktop: the window opens while the engine loads in the
//! background; see `docs/desktop-plan.md`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod domain;
mod engine_loader;
mod i18n;
mod services;
mod ui;

use gpui::{App, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use services::engine::EngineService;
use std::time::Instant;
use ui::RootView;

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
    let engine = model_path.map(|path| {
        let resources = engine_loader::default_resources();
        EngineService::spawn(move || {
            engine_loader::backend(path, resources).map(|backend| (backend, resources))
        })
    });

    gpui_platform::application().run(move |cx: &mut App| {
        ui::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(800.), px(560.))),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            cx.new(|cx| RootView::new(launched, engine, window, cx))
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
