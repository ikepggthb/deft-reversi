//! 設定画面における対局用 Book（`.dbook`）の選択・有効化と、
//! エンジンスレッドおよびコントローラへの設定反映を担うモジュール。
use super::root::RootView;
use crate::{
    app::Command,
    services::{
        book::{self, BookLine},
        engine::{BookStatus, EngineCommand},
    },
};
use deft_reversi_engine::Position;
use gpui::{Context, PathPromptOptions};
use std::path::PathBuf;

impl RootView {
    /// 現在の Book 設定を反映する（ファイルパスはエンジンスレッドへ、使用有無はコントローラへ通知）。
    pub(super) fn apply_book(&mut self) {
        let book = &self.settings.book;
        self.book_status = None;
        self.send_engine(EngineCommand::SetBook(book.path.clone()));
        let use_for_ai = book.use_for_ai();
        self.controller.apply(Command::SetBook(use_for_ai));
    }

    /// 選択された Book ファイルパスを更新し、エンジンへ読み込みを指示する。
    pub(super) fn set_book_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        self.book_selection_gen = self.book_selection_gen.wrapping_add(1);
        self.settings.book.path = path;
        self.apply_book();
        cx.notify();
    }

    /// AI が Book を使用するかの有効/無効を切り替える。
    pub(super) fn set_book_use(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.book.enabled = enabled;
        let use_for_ai = self.settings.book.use_for_ai();
        self.run(Command::SetBook(use_for_ai), cx);
    }

    /// ファイル選択ダイアログを開き、選ばれた `.dbook` ファイルを Book として設定する。
    pub(super) fn choose_book(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| this.set_book_path(Some(path), cx))
                    .ok();
            }
        })
        .detach();
    }
}

impl RootView {
    /// 読み込み済みの Book から `position` の登録手一覧を取得する（AI の使用有無に関わらず参照可能）。
    pub(super) fn book_lines(&self, position: Position) -> Option<Vec<BookLine>> {
        match &self.book_status {
            Some(BookStatus::Loaded { book, .. }) => book::lines(&book.0, position),
            _ => None,
        }
    }
}
