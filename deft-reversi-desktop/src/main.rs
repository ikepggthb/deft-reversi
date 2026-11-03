//! deft-reversi desktop: the window opens while the engine loads in the
//! background; see `docs/desktop-plan.md`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod domain;
mod engine_loader;
mod i18n;
mod persistence;
mod services;
mod ui;

use domain::Settings;
use gpui::{
    App, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use persistence::{paths::Paths, settings_file};
use services::{engine::EngineService, storage::StorageService};
use std::{
    fs::{self, File, OpenOptions},
    time::Instant,
};
use ui::RootView;

/// Whether another instance holds the data folder. Without a folder or a
/// working lock the app still starts.
fn lock_data(paths: &Paths) -> Result<Option<File>, ()> {
    if let Some(dir) = paths.lock.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let Ok(file) = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&paths.lock)
    else {
        return Ok(None);
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err(()),
        Err(std::fs::TryLockError::Error(error)) => {
            eprintln!("[startup] lock {}: {error}", paths.lock.display());
            Ok(None)
        }
    }
}

struct AlreadyRunning(Settings);

impl Render for AlreadyRunning {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x1f2023))
            .text_color(rgb(0xe8e8e8))
            .child(self.0.lang.strings().already_running)
    }
}

fn main() {
    let launched = Instant::now();
    let paths = Paths::from_env();
    let settings = paths
        .as_ref()
        .map_or_else(Settings::default, |p| settings_file::load(&p.settings));

    let lock = match paths.as_ref().map(lock_data) {
        Some(Err(())) => {
            gpui_platform::application().run(move |cx: &mut App| {
                let bounds = Bounds::centered(None, size(px(420.), px(140.)), cx);
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                };
                cx.open_window(options, |_, cx| cx.new(|_| AlreadyRunning(settings)))
                    .expect("failed to open a window");
                cx.on_window_closed(|cx, _| cx.quit()).detach();
                cx.activate(true);
            });
            return;
        }
        Some(Ok(lock)) => lock,
        None => None,
    };

    // Before the window, so the model reads while the platform starts.
    let model_path = engine_loader::find_model();
    if model_path.is_none() {
        eprintln!(
            "model not found: put {} in data/eval/ or set DEFT_REVERSI_MODEL",
            engine_loader::MODEL_FILE
        );
    }
    let resources = settings.resources;
    let engine = model_path.map(|path| {
        EngineService::spawn(move || {
            engine_loader::backend(path, resources).map(|backend| (backend, resources))
        })
    });
    let storage = paths.map(|paths| {
        let (service, events) = StorageService::spawn(paths.clone());
        (service, paths, events)
    });

    gpui_platform::application().run(move |cx: &mut App| {
        // Held until the process ends; the OS releases it then.
        std::mem::forget(lock);
        ui::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(800.), px(560.))),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            cx.new(|cx| RootView::new(launched, engine, storage, settings, window, cx))
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
