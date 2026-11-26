//! `deft-reversi` デスクトップ版の起動エントリポイント。
//!
//! 起動直後の体感速度を上げるため、まずはメインウィンドウをすぐに開き、
//! 時間のかかる評価モデルの読み込みや保存サービスの初期化はバックグラウンドスレッドで並行して進める。
//! 画面描画には Rust 製 UI フレームワークである GPUI を使用している。
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

/// 別の `deft-reversi` プロセスが同じデータフォルダを使っていないか確認し、排他ロックを取得する。
///
/// 同じデータフォルダを同時に開くと棋譜や設定ファイルが競合して破損する恐れがあるため、
/// 起動時にロックファイルで多重起動を防ぐ。
/// ロック用フォルダの作成やファイルを開けない環境では、起動自体は止めずに `Ok(None)` を返す。
/// すでに別プロセスがロックを保持している場合のみ `Err(())` を返す。
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

/// 多重起動が見つかったときに「すでに起動しています」と知らせるだけの専用画面。
///
/// GPUI の `Render` トレイトを実装することで、ウィンドウ内の描画内容として扱える。
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

/// アプリケーションのエントリポイント。
///
/// 1. 設定ファイル（`settings.json`）を読み込む
/// 2. ロックファイルで多重起動でないか確認する（起動済みなら警告ウィンドウだけを出して終了する）
/// 3. 評価モデルの読み込み（`EngineService`）と棋譜保存（`StorageService`）のバックグラウンドスレッドを起動する
/// 4. GPUI のイベントループを開始し、画面全体を表す `Entity`（[`RootView`]）を生成してウィンドウを開く
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

    // OS のウィンドウ初期化と並行して評価モデルを読み込めるよう、GPUI 起動前にスレッドを立ち上げる。
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
        // ロック用の File を drop するとロックが解除されるため、プロセス終了まで意図的に保持し続ける
        // （プロセス終了時に OS がファイルハンドルを閉じ、ロックも自動解放される）。
        std::mem::forget(lock);
        ui::bind_keys(cx);
        let (width, height) = settings.window_size.unwrap_or((1100, 720));
        let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);
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
