//! The opening book in the settings: choosing the file, and telling the
//! engine thread and the controller (§24).
use super::root::RootView;
use crate::{app::Command, services::engine::EngineCommand};
use gpui::{Context, PathPromptOptions};
use std::path::PathBuf;

impl RootView {
    /// Sends the book settings out: the file to the engine thread, its use
    /// to the controller.
    pub(super) fn apply_book(&mut self) {
        let book = &self.settings.book;
        self.book_status = None;
        self.send_engine(EngineCommand::SetBook(book.path.clone()));
        let use_for_ai = book.use_for_ai();
        self.controller.apply(Command::SetBook(use_for_ai));
    }

    pub(super) fn set_book_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        self.settings.book.path = path;
        self.apply_book();
        cx.notify();
    }

    pub(super) fn set_book_use(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.book.enabled = enabled;
        let use_for_ai = self.settings.book.use_for_ai();
        self.run(Command::SetBook(use_for_ai), cx);
    }

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
