//! Saving and opening: autosave, settings, the file menu, open and export
//! through the OS dialogs, and the list of saved games.
use super::{
    new_game::strength_label,
    root::{Dialog, RootView},
    theme, widgets,
};
use crate::{
    app::Command,
    domain::Player,
    engine_loader,
    persistence::{
        record::{EngineInfoV1, RecordV1},
        settings_file::SettingsV1,
    },
    services::storage::{StorageCommand, StorageEvent},
};
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, PathPromptOptions, SharedString, div, prelude::*,
    px,
};

impl RootView {
    pub(super) fn send_storage(&self, command: StorageCommand) {
        if let Some(storage) = &self.storage {
            storage.send(command);
        }
    }

    fn record(&self) -> RecordV1 {
        let c = &self.controller;
        RecordV1::new(
            c.game(),
            c.players(),
            c.history(),
            EngineInfoV1 {
                version: env!("CARGO_PKG_VERSION").into(),
                model: engine_loader::MODEL_FILE.into(),
            },
        )
    }

    /// Sends what changed since the last call; storage debounces the game.
    /// Called on every render, which follows every change.
    pub(super) fn persist(&mut self) {
        if self.storage.is_none() {
            return;
        }
        let revision = self.controller.revision();
        if revision != self.saved_revision {
            self.saved_revision = revision;
            self.send_storage(StorageCommand::SaveGame(self.record()));
        }
        if self.settings != self.saved_settings {
            self.saved_settings = self.settings.clone();
            self.send_storage(StorageCommand::SaveSettings(SettingsV1::from(
                self.settings.clone(),
            )));
        }
    }

    pub(super) fn on_storage(&mut self, event: StorageEvent, cx: &mut Context<Self>) {
        let s = self.settings.lang.strings();
        match event {
            StorageEvent::Restored(Some(game)) => {
                // Only an untouched start gives way to the last game.
                if self.controller.revision() == 0 {
                    self.run(Command::Load(Box::new(game)), cx);
                    self.saved_revision = self.controller.revision();
                }
            }
            StorageEvent::Restored(None) | StorageEvent::Saved => {}
            StorageEvent::SaveFailed { id, error } => {
                if id == self.controller.game().id {
                    self.notice = Some(s.save_failed(&error).into());
                }
            }
            StorageEvent::Loaded(Ok(game)) => {
                self.dialog = None;
                self.run(Command::Load(Box::new(game)), cx);
            }
            StorageEvent::Loaded(Err(error)) => {
                self.notice = Some(s.load_failed(&error).into());
            }
            StorageEvent::Exported(Ok(path)) => {
                self.notice = Some(s.exported(&path.display().to_string()).into());
            }
            StorageEvent::Exported(Err(error)) => {
                self.notice = Some(s.save_failed(&error).into());
            }
            StorageEvent::Games(games) => self.games = Some(games),
        }
        cx.notify();
    }

    pub(super) fn open_file(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let picked = paths.await;
            this.update(cx, |this, cx| match picked {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.send_storage(StorageCommand::Load(path));
                    }
                }
                Ok(Err(error)) => {
                    let s = this.settings.lang.strings();
                    this.notice = Some(s.load_failed(&error.to_string()).into());
                    cx.notify();
                }
                // Cancelled.
                _ => {}
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn export_file(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        let record = self.record();
        let directory = dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| ".".into());
        let name = format!(
            "deft-reversi-{}-{}.dreversi",
            record.created_at.get(..10).unwrap_or("game"),
            record.game_id
        );
        let path = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |this, cx| {
            let picked = path.await;
            this.update(cx, |this, cx| match picked {
                Ok(Ok(Some(path))) => this.send_storage(StorageCommand::Export { path, record }),
                Ok(Err(error)) => {
                    let s = this.settings.lang.strings();
                    this.notice = Some(s.save_failed(&error.to_string()).into());
                    cx.notify();
                }
                _ => {}
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Opens under the toolbar's File item; a click elsewhere closes it.
    pub(super) fn render_file_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lang.strings();
        let item = |id: &'static str,
                    label: &'static str,
                    shortcut: &'static str,
                    f: widgets::Handler,
                    cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex()
                .flex_row()
                .gap(px(24.))
                .px(px(10.))
                .py(px(5.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|style| style.bg(theme::hover()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.menu_open = false;
                    f(this, cx);
                    cx.notify();
                }))
                .child(div().flex_1().child(label))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(shortcut),
                )
        };
        div()
            .id("file-menu-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.menu_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .id("file-menu")
                    .absolute()
                    .top(px(34.))
                    .left(px(10.))
                    .w(px(220.))
                    .p(px(4.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::border())
                    .rounded(px(6.))
                    // Clicks inside the menu do not reach the backdrop.
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(item(
                        "menu-open",
                        s.open,
                        "Ctrl+O",
                        widgets::handler(|this, cx| this.open_file(cx)),
                        cx,
                    ))
                    .child(item(
                        "menu-export",
                        s.export,
                        "Ctrl+Shift+S",
                        widgets::handler(|this, cx| this.export_file(cx)),
                        cx,
                    ))
                    .child(item(
                        "menu-copy-record",
                        s.copy_record,
                        "Ctrl+C",
                        widgets::handler(|this, cx| this.copy_record(cx)),
                        cx,
                    ))
                    .child(item(
                        "menu-enter-record",
                        s.enter_record,
                        "Ctrl+V",
                        widgets::handler(|this, cx| this.enter_record(false, cx)),
                        cx,
                    ))
                    .child(item(
                        "menu-games",
                        s.games,
                        "",
                        widgets::handler(|this, cx| this.open_dialog(Dialog::Games, cx)),
                        cx,
                    )),
            )
            .into_any_element()
    }

    pub(super) fn render_games(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let current = self.controller.game().id.clone();
        let player = |player: Player| -> SharedString {
            match player {
                Player::Human => s.human.into(),
                Player::Ai(strength) => format!("{} {}", s.ai, strength_label(s, strength)).into(),
            }
        };
        let rows: Vec<AnyElement> = match &self.games {
            None => Vec::new(),
            Some(games) if games.is_empty() => vec![
                div()
                    .py(px(12.))
                    .text_color(theme::text_muted())
                    .child(s.no_games)
                    .into_any_element(),
            ],
            Some(games) => games
                .iter()
                .enumerate()
                .map(|(i, game)| {
                    let id = game.id.clone();
                    let is_current = game.id == current;
                    let pending = self.delete_pending.as_deref() == Some(game.id.as_str());
                    let when = local_time(&game.updated_at);
                    let result: SharedString = match game.outcome {
                        Some((black, white)) => format!("{black} – {white}").into(),
                        None => s.in_progress.into(),
                    };
                    let open_id = id.clone();
                    let delete = (!is_current).then(|| {
                        let delete_id = id.clone();
                        widgets::link(
                            ("delete-game", i),
                            if pending { s.delete_confirm } else { s.delete },
                            widgets::handler(move |this, _| {
                                if this.delete_pending.as_deref() == Some(delete_id.as_str()) {
                                    this.delete_pending = None;
                                    this.send_storage(StorageCommand::DeleteGame(
                                        delete_id.clone(),
                                    ));
                                } else {
                                    this.delete_pending = Some(delete_id.clone());
                                }
                            }),
                            cx,
                        )
                        .when(pending, |el| el.text_color(theme::danger()))
                    });
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .id(("open-game", i))
                                .flex_1()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(12.))
                                .px(px(8.))
                                .py(px(6.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(|style| style.bg(theme::hover()))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if let Some(paths) = &this.paths {
                                        let path = paths.game(&open_id);
                                        this.send_storage(StorageCommand::Load(path));
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .w(px(150.))
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(when),
                                )
                                .child(div().flex_1().child(format!(
                                    "{} – {}",
                                    player(game.players[0]),
                                    player(game.players[1])
                                )))
                                .child(
                                    div()
                                        .w(px(50.))
                                        .text_color(theme::text_secondary())
                                        .child(s.moves(game.moves)),
                                )
                                .child(
                                    div()
                                        .w(px(70.))
                                        .text_color(theme::text_secondary())
                                        .when(is_current, |el| el.text_color(theme::accent()))
                                        .child(result),
                                ),
                        )
                        .child(div().w(px(64.)).flex().justify_end().children(delete))
                        .into_any_element()
                })
                .collect(),
        };
        div()
            .w(px(640.))
            .max_h(px(520.))
            .flex()
            .flex_col()
            .px(px(20.))
            .py(px(16.))
            .bg(theme::surface())
            .border_1()
            .border_color(theme::border())
            .rounded(px(10.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .mb(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_base()
                            .font_weight(FontWeight::MEDIUM)
                            .child(s.games),
                    )
                    .child(widgets::link(
                        "close-games",
                        s.close,
                        widgets::handler(|this, cx| this.close_dialog(cx)),
                        cx,
                    )),
            )
            .child(widgets::scroll_area(
                "games-list",
                &self.games_scroll,
                div().flex().flex_col().children(rows),
                cx,
            ))
    }
}

/// "2026-11-03T09:25:05Z" in the local time zone, such as
/// "2026-11-03 18:25".
fn local_time(rfc3339: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(rfc3339) {
        Ok(time) => time
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        Err(_) => rfc3339.get(..16).unwrap_or(rfc3339).replace('T', " "),
    }
}
