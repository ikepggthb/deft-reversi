//! GPUI によるデスクトップ UI ビュー群。
//!
//! 画面全体の状態は GPUI が管理するエンティティ（[`RootView`]）に集約されている。
//! 各サブモジュールは `impl RootView` としてパーツごとの描画メソッド（`Window` と `Context<RootView>` を受け取る）を提供し、
//! ユーザー操作に応じて [`Command`](crate::app::Command) をコントローラへ発行する。
mod ai_log;
mod ai_panel;
mod analysis_panel;
mod animation;
mod board;
mod book;
mod book_work;
mod editor;
mod files;
mod format;
mod new_game;
mod record;
mod record_io;
mod root;
mod settings_view;
mod theme;
mod widgets;

pub use root::{RootView, bind_keys};
