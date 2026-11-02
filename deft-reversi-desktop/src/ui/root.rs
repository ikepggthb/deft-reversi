//! The window: toolbar, the record on the left, the board in the middle and
//! the AI on the right (§9.2 of the plan).
use super::theme;
use crate::{
    app::{Command, Controller, Effect},
    engine_loader::Loaded,
    i18n::Lang,
};
use deft_reversi_engine::{EngineWorker, Square};
use gpui::{
    App, ClickEvent, Context, FocusHandle, KeyBinding, Pixels, Window, actions, div, prelude::*, px,
};
use std::time::{Duration, Instant};

actions!(
    deft,
    [
        NewGame,
        Undo,
        Redo,
        StepBack,
        StepForward,
        First,
        Last,
        FocusBoard,
        BoardLeft,
        BoardRight,
        BoardUp,
        BoardDown,
        BoardPlay,
        LeaveBoard,
    ]
);

/// "Board" sits inside "Workspace", so its arrow keys win while the board
/// has focus and the record's keys work everywhere else.
pub fn bind_keys(cx: &mut App) {
    let workspace = Some("Workspace");
    let board = Some("Board");
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewGame, workspace),
        KeyBinding::new("ctrl-z", Undo, workspace),
        KeyBinding::new("ctrl-y", Redo, workspace),
        KeyBinding::new("ctrl-shift-z", Redo, workspace),
        KeyBinding::new("left", StepBack, workspace),
        KeyBinding::new("right", StepForward, workspace),
        KeyBinding::new("home", First, workspace),
        KeyBinding::new("end", Last, workspace),
        KeyBinding::new("tab", FocusBoard, workspace),
        KeyBinding::new("left", BoardLeft, board),
        KeyBinding::new("right", BoardRight, board),
        KeyBinding::new("up", BoardUp, board),
        KeyBinding::new("down", BoardDown, board),
        KeyBinding::new("enter", BoardPlay, board),
        KeyBinding::new("space", BoardPlay, board),
        KeyBinding::new("escape", LeaveBoard, board),
        KeyBinding::new("tab", LeaveBoard, board),
    ]);
}

pub(super) enum EngineState {
    Missing,
    Loading,
    /// Searches start from P2.
    Ready {
        _worker: EngineWorker,
    },
    Failed,
}

const LEFT_WIDTH: f32 = 150.;
const RIGHT_WIDTH: f32 = 220.;
const GAP: f32 = 28.;
const TOOLBAR_HEIGHT: f32 = 36.;

pub struct RootView {
    pub(super) controller: Controller,
    pub(super) engine: EngineState,
    pub(super) lang: Lang,
    focus: FocusHandle,
    pub(super) board_focus: FocusHandle,
    pub(super) board_cursor: Square,
    launched: Instant,
    first_frame: Option<Duration>,
}

impl RootView {
    pub fn new(
        launched: Instant,
        loading: Option<async_channel::Receiver<Result<Loaded, String>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let engine = match loading {
            None => EngineState::Missing,
            Some(receiver) => {
                cx.spawn(async move |this, cx| {
                    let loaded = receiver
                        .recv()
                        .await
                        .unwrap_or_else(|_| Err("engine-load thread ended".into()));
                    this.update(cx, |this, cx| this.on_loaded(loaded, cx)).ok();
                })
                .detach();
                EngineState::Loading
            }
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            controller: Controller::default(),
            engine,
            lang: Lang::from_env(),
            focus,
            board_focus: cx.focus_handle(),
            board_cursor: "d3".parse().expect("a square name"),
            launched,
            first_frame: None,
        }
    }

    fn on_loaded(&mut self, loaded: Result<Loaded, String>, cx: &mut Context<Self>) {
        let ready_at = self.launched.elapsed();
        self.engine = match loaded {
            Ok(loaded) => {
                eprintln!(
                    "[startup] model {} ms, engine {} ms ({} threads, TT {} MiB), ready at {} ms from {}",
                    loaded.model_time.as_millis(),
                    loaded.engine_time.as_millis(),
                    loaded.threads,
                    loaded.tt_mib,
                    ready_at.as_millis(),
                    loaded.model_path.display(),
                );
                EngineState::Ready {
                    _worker: loaded.worker,
                }
            }
            Err(error) => {
                eprintln!("[startup] engine failed: {error}");
                EngineState::Failed
            }
        };
        cx.notify();
    }

    pub(super) fn run(&mut self, command: Command, cx: &mut Context<Self>) {
        for effect in self.controller.apply(command) {
            match effect {
                // The audio service arrives in P6.
                Effect::Sound(_) => {}
            }
        }
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
        let s = self.lang.strings();
        div()
            .h(px(TOOLBAR_HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .px(px(12.))
            .border_b_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .id("new-game")
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(4.))
                    .text_color(theme::text_secondary())
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::hover()).text_color(theme::text()))
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.run(Command::NewGame, cx)),
                    )
                    .child(s.new_game),
            )
    }

    fn render_ai(&self) -> impl IntoElement {
        let s = self.lang.strings();
        // Nothing is shown while the AI is ready and idle.
        let status = match self.engine {
            EngineState::Missing => Some(s.model_missing),
            EngineState::Loading => Some(s.ai_loading),
            EngineState::Failed => Some(s.ai_failed),
            EngineState::Ready { .. } => None,
        };
        div()
            .w(px(RIGHT_WIDTH))
            .flex()
            .flex_col()
            .justify_end()
            .text_xs()
            .text_color(theme::text_muted())
            .children(status)
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.first_frame.is_none() {
            let t = self.launched.elapsed();
            eprintln!("[startup] first frame at {} ms", t.as_millis());
            self.first_frame = Some(t);
        }
        let cell = Self::cell_size(window);
        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewGame, _, cx| this.run(Command::NewGame, cx)))
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.run(Command::Undo, cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.run(Command::Redo, cx)))
            .on_action(cx.listener(|this, _: &StepBack, _, cx| this.run(Command::Step(-1), cx)))
            .on_action(cx.listener(|this, _: &StepForward, _, cx| this.run(Command::Step(1), cx)))
            .on_action(cx.listener(|this, _: &First, _, cx| this.run(Command::First, cx)))
            .on_action(cx.listener(|this, _: &Last, _, cx| this.run(Command::Last, cx)))
            .on_action(cx.listener(|this, _: &FocusBoard, window, cx| {
                window.focus(&this.board_focus, cx);
                cx.notify();
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::background())
            .text_color(theme::text())
            .text_sm()
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex_1()
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
                    .child(self.render_ai()),
            )
    }
}
