//! The window: toolbar, the record on the left, the board in the middle and
//! the AI on the right (§9.2 of the plan), and the dialogs over them.
use super::{theme, widgets};
use crate::{
    app::{Command, Controller, Effect, Status},
    domain::Settings,
    persistence::paths::Paths,
    services::{
        engine::{EngineCommand, EngineEvent, EngineService},
        storage::{GameSummary, StorageEvent, StorageService},
    },
};
use deft_reversi_engine::Square;
use gpui::{
    App, Context, FocusHandle, KeyBinding, Pixels, SharedString, Subscription, Window, actions,
    div, prelude::*, px,
};
use std::time::{Duration, Instant};

actions!(
    deft,
    [
        NewGame,
        OpenFile,
        ExportFile,
        Undo,
        Redo,
        StepBack,
        StepForward,
        First,
        Last,
        TogglePause,
        MoveNow,
        ToggleAnalysis,
        FocusBoard,
        BoardLeft,
        BoardRight,
        BoardUp,
        BoardDown,
        BoardPlay,
        LeaveBoard,
        CloseDialog,
    ]
);

/// "Board" sits inside "Workspace", so its arrow keys win while the board
/// has focus and the record's keys work everywhere else. A dialog has its
/// own context, outside the workspace.
pub fn bind_keys(cx: &mut App) {
    let workspace = Some("Workspace");
    let board = Some("Board");
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewGame, workspace),
        KeyBinding::new("ctrl-o", OpenFile, workspace),
        KeyBinding::new("ctrl-shift-s", ExportFile, workspace),
        KeyBinding::new("ctrl-z", Undo, workspace),
        KeyBinding::new("ctrl-y", Redo, workspace),
        KeyBinding::new("ctrl-shift-z", Redo, workspace),
        KeyBinding::new("left", StepBack, workspace),
        KeyBinding::new("right", StepForward, workspace),
        KeyBinding::new("home", First, workspace),
        KeyBinding::new("end", Last, workspace),
        KeyBinding::new("space", TogglePause, workspace),
        KeyBinding::new("ctrl-enter", MoveNow, workspace),
        KeyBinding::new("f2", ToggleAnalysis, workspace),
        KeyBinding::new("tab", FocusBoard, workspace),
        KeyBinding::new("left", BoardLeft, board),
        KeyBinding::new("right", BoardRight, board),
        KeyBinding::new("up", BoardUp, board),
        KeyBinding::new("down", BoardDown, board),
        KeyBinding::new("enter", BoardPlay, board),
        KeyBinding::new("space", BoardPlay, board),
        KeyBinding::new("escape", LeaveBoard, board),
        KeyBinding::new("tab", LeaveBoard, board),
        KeyBinding::new("escape", CloseDialog, Some("Dialog")),
    ]);
}

pub(super) enum Dialog {
    NewGame,
    Settings,
    Games,
}

const LEFT_WIDTH: f32 = 150.;
const RIGHT_WIDTH: f32 = 220.;
const GAP: f32 = 28.;
const TOOLBAR_HEIGHT: f32 = 36.;

pub struct RootView {
    pub(super) controller: Controller,
    /// None when the model was not found.
    engine: Option<EngineService>,
    pub(super) settings: Settings,
    pub(super) dialog: Option<Dialog>,
    /// None when the OS gives no folders; then nothing is saved.
    pub(super) storage: Option<StorageService>,
    pub(super) paths: Option<Paths>,
    /// The controller revision last sent to storage; 0 is an untouched game.
    pub(super) saved_revision: u64,
    pub(super) saved_settings: Settings,
    /// A message about files, until the next one.
    pub(super) notice: Option<SharedString>,
    pub(super) menu_open: bool,
    /// The saved games while the list is open.
    pub(super) games: Option<Vec<GameSummary>>,
    /// A game whose delete button was pressed once.
    pub(super) delete_pending: Option<String>,
    _quit: Option<Subscription>,
    focus: FocusHandle,
    dialog_focus: FocusHandle,
    pub(super) board_focus: FocusHandle,
    pub(super) board_cursor: Square,
    launched: Instant,
    first_frame: Option<Duration>,
}

impl RootView {
    pub fn new(
        launched: Instant,
        engine: Option<(EngineService, async_channel::Receiver<EngineEvent>)>,
        storage: Option<(StorageService, Paths, async_channel::Receiver<StorageEvent>)>,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (storage, paths, quit) = match storage {
            None => (None, None, None),
            Some((service, paths, events)) => {
                cx.spawn(async move |this, cx| {
                    while let Ok(event) = events.recv().await {
                        if this
                            .update(cx, |this, cx| this.on_storage(event, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
                // The last save is written before the process ends.
                let quit = cx.on_app_quit(|this: &mut Self, _| {
                    this.persist();
                    if let Some(storage) = this.storage.as_mut() {
                        storage.shutdown();
                    }
                    async {}
                });
                (Some(service), Some(paths), Some(quit))
            }
        };
        let engine = engine.map(|(service, events)| {
            cx.spawn(async move |this, cx| {
                while let Ok(event) = events.recv().await {
                    let delivered = this.update(cx, |this, cx| {
                        if let EngineEvent::Ready {
                            resources,
                            load_time,
                        } = &event
                        {
                            eprintln!(
                                "[startup] engine ready ({} threads, TT {} MiB) in {} ms, at {} ms",
                                resources.threads,
                                resources.tt_mib,
                                load_time.as_millis(),
                                this.launched.elapsed().as_millis()
                            );
                        }
                        let effects = this.controller.on_engine(event);
                        this.dispatch(effects, cx);
                        cx.notify();
                    });
                    if delivered.is_err() {
                        break;
                    }
                }
            })
            .detach();
            service
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            controller: {
                let mut controller = Controller::default();
                controller.apply(Command::SetAnalysisSettings(settings.analysis));
                controller
            },
            engine,
            settings,
            dialog: None,
            storage,
            paths,
            saved_revision: 0,
            saved_settings: settings,
            notice: None,
            menu_open: false,
            games: None,
            delete_pending: None,
            _quit: quit,
            focus,
            dialog_focus: cx.focus_handle(),
            board_focus: cx.focus_handle(),
            board_cursor: "d3".parse().expect("a square name"),
            launched,
            first_frame: None,
        }
    }

    pub(super) fn has_engine(&self) -> bool {
        self.engine.is_some()
    }

    pub(super) fn run(&mut self, command: Command, cx: &mut Context<Self>) {
        self.notice = None;
        let effects = self.controller.apply(command);
        self.dispatch(effects, cx);
        cx.notify();
    }

    fn dispatch(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in effects {
            let command = match effect {
                Effect::StartSearch { job, delay } if !delay.is_zero() => {
                    // Sent later only if the controller still waits for it.
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(delay).await;
                        this.update(cx, |this, _| {
                            let waiting = this.controller.thinking().map(|t| t.id());
                            if waiting == Some(job.id)
                                && let Some(engine) = &this.engine
                            {
                                engine.send(EngineCommand::Start(job));
                            }
                        })
                        .ok();
                    })
                    .detach();
                    continue;
                }
                Effect::StartSearch { job, .. } => EngineCommand::Start(job),
                Effect::CancelSearch => EngineCommand::Cancel,
                Effect::MoveNow => EngineCommand::MoveNow,
                Effect::ClearTt => EngineCommand::ClearTt,
                Effect::Reconfigure(resources) => EngineCommand::Reconfigure(resources),
                // The audio service arrives in P6.
                Effect::Sound(_) => continue,
            };
            if let Some(engine) = &self.engine {
                engine.send(command);
            }
        }
    }

    pub(super) fn toggle_analysis(&mut self, cx: &mut Context<Self>) {
        let on = !self.controller.analyzing();
        self.run(Command::SetAnalysis(on), cx);
    }

    /// Focus follows on the next render.
    pub(super) fn open_dialog(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        if matches!(dialog, Dialog::Games) {
            self.games = None;
            self.delete_pending = None;
            self.send_storage(crate::services::storage::StorageCommand::ListGames);
        }
        self.menu_open = false;
        self.dialog = Some(dialog);
        cx.notify();
    }

    pub(super) fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        cx.notify();
    }

    fn move_board_cursor(&mut self, dx: i8, dy: i8, cx: &mut Context<Self>) {
        let x = (self.board_cursor.x() as i8 + dx).clamp(0, 7) as u8;
        let y = (self.board_cursor.y() as i8 + dy).clamp(0, 7) as u8;
        self.board_cursor = Square::from_xy(x, y).expect("clamped to the board");
        cx.notify();
    }

    /// The side of one square, in whole pixels so the grid lines stay even.
    fn cell_size(window: &Window) -> Pixels {
        let viewport = window.viewport_size();
        // Page padding, the coordinate labels, and the navigation row.
        let width = f32::from(viewport.width) - LEFT_WIDTH - RIGHT_WIDTH - 2. * GAP - 40. - 18.;
        let height = f32::from(viewport.height) - TOOLBAR_HEIGHT - 32. - 18. - 48.;
        // Eight squares, seven inner lines and the frame.
        let side = width.min(height) - 7. - 4.;
        px((side / 8.).floor().max(28.))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.settings.lang.strings();
        let open = |dialog: fn() -> Dialog| {
            widgets::handler(move |this, cx| this.open_dialog(dialog(), cx))
        };
        div()
            .h(px(TOOLBAR_HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .px(px(12.))
            .border_b_1()
            .border_color(theme::hairline())
            .child(widgets::link(
                "file-menu",
                s.file_menu,
                widgets::handler(|this, _| this.menu_open = !this.menu_open),
                cx,
            ))
            .child(widgets::link(
                "new-game",
                s.new_game,
                open(|| Dialog::NewGame),
                cx,
            ))
            .child(
                widgets::link(
                    "analysis",
                    s.analysis,
                    widgets::handler(|this, cx| this.toggle_analysis(cx)),
                    cx,
                )
                .when(self.controller.analyzing(), |el| {
                    el.bg(theme::selected()).text_color(theme::accent())
                }),
            )
            .child(div().flex_1())
            .child(widgets::link(
                "settings",
                s.settings,
                open(|| Dialog::Settings),
                cx,
            ))
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.first_frame.is_none() {
            let t = self.launched.elapsed();
            eprintln!("[startup] first frame at {} ms", t.as_millis());
            self.first_frame = Some(t);
        }
        self.persist();
        // Handlers have no window, so focus follows the dialog here.
        if self.dialog.is_some() {
            if !self.dialog_focus.contains_focused(window, cx) {
                window.focus(&self.dialog_focus, cx);
            }
        } else if !self.focus.contains_focused(window, cx) {
            window.focus(&self.focus, cx);
        }
        let cell = Self::cell_size(window);
        let workspace = div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &NewGame, _, cx| this.open_dialog(Dialog::NewGame, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenFile, _, cx| this.open_file(cx)))
            .on_action(cx.listener(|this, _: &ExportFile, _, cx| this.export_file(cx)))
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.run(Command::Undo, cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.run(Command::Redo, cx)))
            .on_action(cx.listener(|this, _: &StepBack, _, cx| this.run(Command::Step(-1), cx)))
            .on_action(cx.listener(|this, _: &StepForward, _, cx| this.run(Command::Step(1), cx)))
            .on_action(cx.listener(|this, _: &First, _, cx| this.run(Command::First, cx)))
            .on_action(cx.listener(|this, _: &Last, _, cx| this.run(Command::Last, cx)))
            .on_action(cx.listener(|this, _: &TogglePause, _, cx| {
                let command = match this.controller.status() {
                    Status::Playing => Command::Pause,
                    Status::Paused => Command::Resume,
                };
                this.run(command, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveNow, _, cx| this.run(Command::MoveNow, cx)))
            .on_action(cx.listener(|this, _: &ToggleAnalysis, _, cx| this.toggle_analysis(cx)))
            .on_action(cx.listener(|this, _: &FocusBoard, window, cx| {
                window.focus(&this.board_focus, cx);
                cx.notify();
            }))
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .gap(px(GAP))
                    .px(px(20.))
                    .py(px(16.))
                    .child(self.render_record(cx).w(px(LEFT_WIDTH)))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                div()
                                    .key_context("Board")
                                    .track_focus(&self.board_focus)
                                    .on_action(cx.listener(|this, _: &BoardLeft, _, cx| {
                                        this.move_board_cursor(-1, 0, cx)
                                    }))
                                    .on_action(cx.listener(|this, _: &BoardRight, _, cx| {
                                        this.move_board_cursor(1, 0, cx)
                                    }))
                                    .on_action(cx.listener(|this, _: &BoardUp, _, cx| {
                                        this.move_board_cursor(0, -1, cx)
                                    }))
                                    .on_action(cx.listener(|this, _: &BoardDown, _, cx| {
                                        this.move_board_cursor(0, 1, cx)
                                    }))
                                    .on_action(cx.listener(|this, _: &BoardPlay, _, cx| {
                                        this.run(Command::Play(this.board_cursor), cx)
                                    }))
                                    .on_action(cx.listener(|this, _: &LeaveBoard, window, cx| {
                                        window.focus(&this.focus, cx);
                                        cx.notify();
                                    }))
                                    .child(self.render_board(cell, window, cx)),
                            )
                            .child(self.render_navigation(cell, cx)),
                    )
                    .child(self.render_ai(cx).w(px(RIGHT_WIDTH))),
            );
        let dialog = self.dialog.as_ref().map(|dialog| {
            let panel = match dialog {
                Dialog::NewGame => self.render_new_game(cx).into_any_element(),
                Dialog::Settings => self.render_settings(cx).into_any_element(),
                Dialog::Games => self.render_games(cx).into_any_element(),
            };
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.45))
                .child(
                    div()
                        .key_context("Dialog")
                        .track_focus(&self.dialog_focus)
                        .on_action(
                            cx.listener(|this, _: &CloseDialog, _, cx| this.close_dialog(cx)),
                        )
                        .child(panel),
                )
        });
        div()
            .relative()
            .size_full()
            .bg(theme::background())
            .text_color(theme::text())
            .text_sm()
            .child(workspace)
            .children(self.menu_open.then(|| self.render_file_menu(cx)))
            .children(dialog)
    }
}
