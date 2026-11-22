//! The window: toolbar, the record on the left, the board in the middle and
//! the AI on the right (§9.2 of the plan), and the dialogs over them.
use super::{animation::FlipAnimation, book_work::BookWork, editor::EditorState, theme, widgets};
use crate::{
    app::{Command, Controller, Effect, Sound, Status},
    domain::{
        Settings,
        settings::{Animation, ThemeChoice},
    },
    persistence::paths::Paths,
    services::{
        audio::AudioService,
        engine::{BookStatus, EngineCommand, EngineEvent, EngineService},
        storage::{GameSummary, StorageEvent, StorageService},
    },
};
use deft_reversi_engine::{Action, Color, Square};
use gpui::{
    App, Context, FocusHandle, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, ScrollHandle, SharedString, Subscription, Window,
    WindowAppearance, actions, div, prelude::*, px,
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
        OpenEditor,
        CancelEditor,
        FocusBoard,
        BoardLeft,
        BoardRight,
        BoardUp,
        BoardDown,
        BoardPlay,
        LeaveBoard,
        CloseDialog,
        ConfirmDialog,
        CopyRecord,
        PasteRecord,
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
        KeyBinding::new("ctrl-c", CopyRecord, workspace),
        KeyBinding::new("ctrl-v", PasteRecord, workspace),
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
        KeyBinding::new("f3", OpenEditor, workspace),
        KeyBinding::new("escape", CancelEditor, workspace),
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
        KeyBinding::new("enter", ConfirmDialog, Some("Dialog")),
    ]);
}

pub(super) enum Dialog {
    NewGame,
    Settings,
    Games,
    EnterRecord,
    AiLog,
    BookWork,
}

/// How long "Black passes" stays up unless clicked away.
const PASS_NOTICE: Duration = Duration::from_millis(600);

const LEFT_WIDTH: f32 = 170.;
const RIGHT_WIDTH: f32 = 240.;
const GAP: f32 = 28.;
const TOOLBAR_HEIGHT: f32 = 36.;
/// Dialogs hang from here, so one that grows does not move what is above.
pub(super) const DIALOG_TOP: f32 = 40.;
/// Above and below the board: the toolbar, the page padding, the
/// coordinates and the navigation row.
pub(super) const BOARD_CHROME: f32 = TOOLBAR_HEIGHT + 32. + 18. + 56.;

pub struct RootView {
    pub(super) controller: Controller,
    /// None when the model was not found.
    engine: Option<EngineService>,
    /// What the engine thread said of the book; None while it reads one.
    pub(super) book_status: Option<BookStatus>,
    pub(super) book_work: BookWork,
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
    /// The position editor, while it is open.
    pub(super) editor: Option<EditorState>,
    /// The last move turning over, drawn on top of the position.
    pub(super) animation: Option<FlipAnimation>,
    /// Whether that move was the AI's.
    animation_by_ai: bool,
    /// The AI's move, waiting for the animation of the move before it.
    held_move: Option<EngineEvent>,
    /// The graph column under the pointer.
    pub(super) graph_hover: Option<usize>,
    /// The graph's grip being dragged: where the pointer and the height
    /// were when it began, and the height meanwhile.
    pub(super) graph_drag: Option<(Pixels, u16)>,
    pub(super) graph_live: Option<u16>,
    /// Who passed and when, while it is shown over the board.
    pub(super) pass_notice: Option<(Color, Instant)>,
    /// The record being typed in its dialog (whole moves), its caret (in
    /// moves), the first half of a move being typed there, and
    /// whether all of it is selected.
    pub(super) record_input: String,
    pub(super) record_caret: usize,
    pub(super) record_all: bool,
    pub(super) record_pending: Option<char>,
    /// The record dialog chooses where a book grows from, not the game.
    pub(super) record_for_book: bool,
    /// The record was just copied; the link says so for a moment.
    pub(super) record_copied: bool,
    pub(super) games_scroll: ScrollHandle,
    pub(super) log_scroll: ScrollHandle,
    pub(super) settings_scroll: ScrollHandle,
    pub(super) book_scroll: ScrollHandle,
    pub(super) new_game_scroll: ScrollHandle,
    /// The new game dialog's choice of book moves, from the settings.
    pub(super) new_game_book: bool,
    pub(super) scroll_drag: Option<widgets::ScrollDrag>,
    audio: AudioService,
    /// The window size at the last frame, saved on quit.
    pub(super) viewport: Option<(u32, u32)>,
    /// `DEFT_DESKTOP_TRACE=1`: frame times while discs turn and search
    /// results go to stderr, for the measurements of §11.
    trace: Option<Vec<Instant>>,
    _quit: Option<Subscription>,
    _appearance: Subscription,
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
                    if this.viewport.is_some() {
                        this.settings.window_size = this.viewport;
                    }
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
                        if this.trace.is_some()
                            && let EngineEvent::Finished { report, .. } = &event
                        {
                            eprintln!(
                                "[trace] search {} nodes {} ms",
                                report.nodes,
                                report.elapsed.as_millis()
                            );
                        }
                        if let EngineEvent::Book(status) = &event {
                            this.book_status = Some(status.clone());
                        }
                        match &event {
                            EngineEvent::BookProgress(progress) => this.on_book_progress(*progress),
                            EngineEvent::BookDone(done) => this.on_book_done(done.clone(), cx),
                            _ => {}
                        }
                        // A move found while a person's move still turns its
                        // discs (a book move comes at once) waits for it; one
                        // after the AI's own move cuts that animation short,
                        // so an AI game runs at the AI's pace.
                        if this.animation.is_some()
                            && !this.animation_by_ai
                            && this.controller.is_ai_move(&event)
                        {
                            this.held_move = Some(event);
                            return;
                        }
                        let effects = this.controller.on_engine(event);
                        this.dispatch(effects);
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
        let mut view = Self {
            controller: {
                let mut controller = Controller::default();
                controller.apply(Command::SetAnalysisSettings(settings.analysis));
                controller
            },
            engine,
            settings: settings.clone(),
            dialog: None,
            storage,
            paths,
            saved_revision: 0,
            saved_settings: settings,
            notice: None,
            menu_open: false,
            games: None,
            delete_pending: None,
            editor: None,
            animation: None,
            animation_by_ai: false,
            held_move: None,
            graph_hover: None,
            graph_drag: None,
            graph_live: None,
            pass_notice: None,
            record_input: String::new(),
            record_caret: 0,
            record_all: false,
            record_pending: None,
            record_for_book: false,
            record_copied: false,
            games_scroll: ScrollHandle::new(),
            log_scroll: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            book_scroll: ScrollHandle::new(),
            new_game_scroll: ScrollHandle::new(),
            new_game_book: true,
            scroll_drag: None,
            audio: AudioService::spawn(),
            viewport: None,
            trace: std::env::var_os("DEFT_DESKTOP_TRACE").map(|_| Vec::new()),
            _quit: quit,
            _appearance: cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            focus,
            dialog_focus: cx.focus_handle(),
            board_focus: cx.focus_handle(),
            board_cursor: "d3".parse().expect("a square name"),
            launched,
            first_frame: None,
            book_status: None,
            book_work: BookWork::default(),
        };
        view.apply_book();
        view
    }

    pub(super) fn has_engine(&self) -> bool {
        self.engine.is_some()
    }

    pub(super) fn run(&mut self, command: Command, cx: &mut Context<Self>) {
        // The game waits while the editor is open.
        if self.editor.is_some() {
            return;
        }
        self.notice = None;
        // Anything but a new move ends the running animation at once.
        self.animation = None;
        self.pass_notice = None;
        let effects = self.controller.apply(command);
        self.dispatch(effects);
        cx.notify();
    }

    fn dispatch(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            let command = match effect {
                Effect::StartSearch { job } => EngineCommand::Start(job),
                Effect::CancelSearch => EngineCommand::Cancel,
                Effect::MoveNow => EngineCommand::MoveNow,
                Effect::ClearTt => EngineCommand::ClearTt,
                Effect::Reconfigure(resources) => EngineCommand::Reconfigure(resources),
                Effect::Sound(sound) => {
                    if self.settings.sound && self.settings.volume > 0 {
                        self.audio
                            .play(sound, f32::from(self.settings.volume) / 100.);
                    }
                    self.start_animation();
                    // A person playing sees who passed; AI games go on.
                    let history = self.controller.history();
                    if sound == Sound::Pass
                        && self.controller.players().iter().any(|p| !p.is_ai())
                        && let Some(index) = history.cursor().checked_sub(1)
                    {
                        let passer = history.position(index).side_to_move();
                        self.pass_notice = Some((passer, Instant::now()));
                    }
                    continue;
                }
            };
            self.send_engine(command);
        }
    }

    pub(super) fn send_engine(&self, command: EngineCommand) {
        if let Some(engine) = &self.engine {
            engine.send(command);
        }
    }

    pub(super) fn toggle_analysis(&mut self, cx: &mut Context<Self>) {
        let on = !self.controller.analyzing();
        self.run(Command::SetAnalysis(on), cx);
    }

    /// Frame intervals of the animation that just ended, when tracing.
    fn report_frames(&mut self) {
        let Some(frames) = self.trace.as_mut() else {
            return;
        };
        let mut gaps: Vec<f64> = frames
            .windows(2)
            .map(|w| (w[1] - w[0]).as_secs_f64() * 1000.)
            .collect();
        frames.clear();
        if gaps.is_empty() {
            return;
        }
        gaps.sort_by(f64::total_cmp);
        let p95 = gaps[(gaps.len() * 95 / 100).min(gaps.len() - 1)];
        eprintln!(
            "[trace] frames {} median {:.1} ms p95 {p95:.1} ms max {:.1} ms",
            gaps.len() + 1,
            gaps[gaps.len() / 2],
            gaps[gaps.len() - 1]
        );
    }

    /// Animates the move just recorded; the previous animation ends.
    fn start_animation(&mut self) {
        let scale = match self.settings.animation {
            Animation::Normal => 1.,
            Animation::Short => 0.5,
            Animation::Off => 0.,
        };
        let history = self.controller.history();
        let actions = &history.actions()[..history.cursor()];
        // A forced pass may follow the move.
        let index = actions.iter().rposition(|a| matches!(a, Action::Place(_)));
        self.animation = index.and_then(|i| match actions[i] {
            Action::Place(square) => FlipAnimation::new(history.position(i), square, scale),
            Action::Pass => None,
        });
        let players = self.controller.players();
        self.animation_by_ai = index.is_some_and(|i| {
            let mover = history.position(i).side_to_move();
            players[usize::from(mover == Color::White)].is_ai()
        });
    }

    /// Focus follows on the next render.
    pub(super) fn open_dialog(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        if self.editor.is_some() || self.book_work.running.is_some() {
            return;
        }
        if matches!(dialog, Dialog::Games) {
            self.games = None;
            self.delete_pending = None;
            self.send_storage(crate::services::storage::StorageCommand::ListGames);
        }
        if matches!(dialog, Dialog::NewGame) {
            self.new_game_book = self.settings.book.enabled;
        }
        if matches!(dialog, Dialog::AiLog) {
            self.log_scroll.scroll_to_bottom();
        }
        self.scroll_drag = None;
        self.menu_open = false;
        self.dialog = Some(dialog);
        cx.notify();
    }

    pub(super) fn close_dialog(&mut self, cx: &mut Context<Self>) {
        // A growing book keeps its dialog open until it is stopped.
        if self.book_work.running.is_some() {
            return;
        }
        // A record for the book goes back to the book builder.
        if matches!(self.dialog, Some(Dialog::EnterRecord)) && self.record_for_book {
            self.record_for_book = false;
            if self.book_work.record.is_none() {
                self.book_work.root = super::book_work::RootChoice::Current;
            }
            self.dialog = Some(Dialog::BookWork);
            cx.notify();
            return;
        }
        self.dialog = None;
        cx.notify();
    }

    /// The arrows move the cursor as the board is shown, however it is turned.
    fn move_board_cursor(&mut self, dx: i8, dy: i8, cx: &mut Context<Self>) {
        let view = self.settings.board_view;
        let (x, y) = view.place_of(self.board_cursor);
        let x = (x as i8 + dx).clamp(0, 7) as u8;
        let y = (y as i8 + dy).clamp(0, 7) as u8;
        self.board_cursor = view.square_at(x, y);
        cx.notify();
    }

    /// The side of one square, in whole pixels so the grid lines stay even.
    fn cell_size(window: &Window, graph: Option<f32>) -> Pixels {
        let viewport = window.viewport_size();
        // Page padding, the coordinate labels, the navigation row, and the
        // value graph in analysis mode.
        let width = f32::from(viewport.width) - LEFT_WIDTH - RIGHT_WIDTH - 2. * GAP - 40. - 18.;
        let graph = graph.map_or(0., |plot| {
            plot + super::analysis_panel::GRAPH_CHROME + super::analysis_panel::GRAPH_GRIP
        });
        let height = f32::from(viewport.height) - BOARD_CHROME - graph;
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
            .child(widgets::link(
                "edit-position",
                s.edit_position,
                widgets::handler(|this, cx| this.open_editor(cx)),
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
            .child(
                widgets::link(
                    "study",
                    s.study,
                    widgets::handler(|this, cx| {
                        let on = !this.controller.studying();
                        this.run(Command::SetStudy(on), cx)
                    }),
                    cx,
                )
                .when(self.controller.studying(), |el| {
                    el.bg(theme::selected()).text_color(theme::accent())
                }),
            )
            .child(
                widgets::link(
                    "book-work",
                    s.book_work,
                    widgets::handler(|this, cx| this.open_book_work_dialog(cx)),
                    cx,
                )
                .when(self.book_work.running.is_some(), |el| {
                    el.text_color(theme::accent())
                }),
            )
            .child(div().flex_1())
            // How the board is shown; kept in the settings.
            .child(widgets::link(
                "turn-board",
                s.turn_board,
                widgets::handler(|this, _| {
                    this.settings.board_view = this.settings.board_view.turned()
                }),
                cx,
            ))
            .child(widgets::link(
                "mirror-board",
                s.mirror_board,
                widgets::handler(|this, _| {
                    this.settings.board_view = this.settings.board_view.mirrored()
                }),
                cx,
            ))
            .child(widgets::link(
                "ai-log",
                s.ai_log,
                open(|| Dialog::AiLog),
                cx,
            ))
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
        let dark = match self.settings.theme {
            ThemeChoice::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
        };
        theme::set_mode(if dark {
            theme::Mode::Dark
        } else {
            theme::Mode::Light
        });
        // Handlers have no window, so focus follows the dialog here.
        if self.dialog.is_some() {
            if !self.dialog_focus.contains_focused(window, cx) {
                window.focus(&self.dialog_focus, cx);
            }
        } else if !self.focus.contains_focused(window, cx) {
            window.focus(&self.focus, cx);
        }
        // Frames are asked for only while discs are turning.
        if self.animation.as_ref().is_some_and(FlipAnimation::done) {
            self.animation = None;
            self.report_frames();
        }
        // The AI's move held back for the last animation is played now; a
        // search cancelled meanwhile (Undo, a new game) is ignored by it.
        if self.animation.is_none()
            && let Some(event) = self.held_move.take()
        {
            let effects = self.controller.on_engine(event);
            self.dispatch(effects);
        }
        if self.animation.is_some()
            && let Some(frames) = self.trace.as_mut()
        {
            frames.push(Instant::now());
        }
        if self
            .pass_notice
            .is_some_and(|(_, shown)| shown.elapsed() >= PASS_NOTICE)
        {
            self.pass_notice = None;
        }
        if self.animation.is_some() || self.pass_notice.is_some() {
            window.request_animation_frame();
        }
        let size = window.viewport_size();
        self.viewport = Some((f32::from(size.width) as u32, f32::from(size.height) as u32));
        let graph =
            (self.controller.analyzing() && self.editor.is_none()).then(|| self.graph_height());
        let cell = Self::cell_size(window, graph);
        let workspace = div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &NewGame, _, cx| this.open_dialog(Dialog::NewGame, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenFile, _, cx| this.open_file(cx)))
            .on_action(cx.listener(|this, _: &ExportFile, _, cx| this.export_file(cx)))
            .on_action(cx.listener(|this, _: &CopyRecord, _, cx| this.copy_record(cx)))
            .on_action(cx.listener(|this, _: &PasteRecord, _, cx| this.enter_record(true, cx)))
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
            .on_action(cx.listener(|this, _: &OpenEditor, _, cx| this.open_editor(cx)))
            .on_action(cx.listener(|this, _: &CancelEditor, _, cx| this.cancel_editor(cx)))
            .on_action(cx.listener(|this, _: &FocusBoard, window, cx| {
                window.focus(&this.board_focus, cx);
                cx.notify();
            }))
            // The graph's grip follows the pointer anywhere in the window.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.graph_drag.is_some() {
                    if event.dragging() {
                        this.drag_graph(event.position.y, cx);
                    } else {
                        this.end_graph_drag(cx);
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.end_graph_drag(cx)),
            )
            .size_full()
            .flex()
            .flex_col()
            .child(if self.editor.is_some() {
                div()
                    .h(px(TOOLBAR_HEIGHT))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(theme::hairline())
                    .child(self.render_editor_bar(cx).flex_1())
                    .into_any_element()
            } else {
                self.render_toolbar(cx).into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .gap(px(GAP))
                    .px(px(20.))
                    .py(px(16.))
                    .child(
                        if self.editor.is_some() {
                            self.render_editor_tools(cx)
                        } else {
                            self.render_record(cx)
                        }
                        .w(px(LEFT_WIDTH)),
                    )
                    .when(self.editor.is_some(), |el| {
                        el.child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .items_center()
                                .child(self.render_editor_board(cell, cx)),
                        )
                        // The AI column stays empty, so the board does not move.
                        .child(div().w(px(RIGHT_WIDTH)))
                    })
                    .when(self.editor.is_none(), |el| {
                        el.child(
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
                                        .on_action(cx.listener(
                                            |this, _: &LeaveBoard, window, cx| {
                                                window.focus(&this.focus, cx);
                                                cx.notify();
                                            },
                                        ))
                                        .child(self.render_board(cell, window, cx)),
                                )
                                .child(self.render_navigation(cell, cx))
                                .when(self.controller.analyzing(), |el| {
                                    // Under the board, from its coordinates to its edge.
                                    let board = px(18.) + cell * 8. + px(7. + 4.);
                                    el.child(self.render_graph(board, cx))
                                }),
                        )
                        .child(self.render_ai(cx).w(px(RIGHT_WIDTH)))
                    }),
            );
        let dialog = self.dialog.as_ref().map(|dialog| {
            let panel = match dialog {
                Dialog::NewGame => self.render_new_game(cx).into_any_element(),
                Dialog::Settings => self.render_settings(cx).into_any_element(),
                Dialog::Games => self.render_games(cx).into_any_element(),
                Dialog::EnterRecord => self.render_record_input(cx).into_any_element(),
                Dialog::AiLog => self.render_ai_log(cx).into_any_element(),
                Dialog::BookWork => self.render_book_work(cx).into_any_element(),
            };
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(DIALOG_TOP))
                .bg(gpui::black().opacity(0.45))
                // A press outside the dialog closes it; one inside stops there.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_dialog(cx)),
                )
                // The backdrop covers the window, so a held scroll bar follows
                // the pointer anywhere.
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if event.dragging() {
                        this.drag_scroll(event.position.y, cx);
                    } else {
                        this.scroll_drag = None;
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, _| this.scroll_drag = None),
                )
                .child(
                    div()
                        .key_context("Dialog")
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if matches!(this.dialog, Some(Dialog::EnterRecord)) {
                                this.record_key(event, cx);
                            }
                        }))
                        .track_focus(&self.dialog_focus)
                        .on_action(
                            cx.listener(|this, _: &CloseDialog, _, cx| this.close_dialog(cx)),
                        )
                        // Enter does what the dialog's main button does.
                        .on_action(cx.listener(
                            |this, _: &ConfirmDialog, _, cx| match this.dialog {
                                Some(Dialog::NewGame) => this.start_new_game(cx),
                                Some(Dialog::EnterRecord) => this.load_record(cx),
                                _ => this.close_dialog(cx),
                            },
                        ))
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
            .children(
                self.pass_notice
                    .map(|(color, _)| self.render_pass_notice(color, cx)),
            )
            .children(self.menu_open.then(|| self.render_file_menu(cx)))
            .children(dialog)
    }
}
