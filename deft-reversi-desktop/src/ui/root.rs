//! アプリケーションのルートビュー（[`RootView`]）とキーバインドを定義するモジュール。
//!
//! ツールバー・左カラム（棋譜/エディタツール）・中央（盤面/ナビゲーション/評価値グラフ）・
//! 右カラム（解析パネル/AI ステータス）、および各種モーダルダイアログを統合し、
//! GPUI の非同期イベントループ上でエンジン・ストレージスレッドとの連携を仲介する。
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
        engine::{BookStatus, EngineCommand, EngineEvent, EngineService, ModelInfo},
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

/// キーボードショートカットを GPUI のキーコンテキストへ登録する。
///
/// `"Board"` コンテキストは `"Workspace"` の内側に配置されるため、盤面にフォーカスがある間は
/// 矢印キー等が盤面カーソル移動を優先し、それ以外では棋譜送りとして機能する。
/// ダイアログは `"Workspace"` の外側に独立した `"Dialog"` コンテキストを持つ。
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

/// 現在開いているモーダルダイアログの種類。
pub(super) enum Dialog {
    /// 新規対局設定ダイアログ。
    NewGame,
    /// アプリケーション設定ダイアログ。
    Settings,
    /// 保存済み対局一覧ダイアログ。
    Games,
    /// 棋譜文字列入力ダイアログ。
    EnterRecord,
    /// AI 思考ログ一覧ダイアログ。
    AiLog,
    /// Book 作成・編集ダイアログ。
    BookWork,
}

/// パス通知オーバーレイをクリックなしで自動消去するまでの表示時間。
const PASS_NOTICE: Duration = Duration::from_millis(600);

const LEFT_WIDTH: f32 = 170.;
const RIGHT_WIDTH: f32 = 240.;
const GAP: f32 = 28.;
/// ツールバーの高さ（論理ピクセル）。
pub(super) const TOOLBAR_HEIGHT: f32 = 36.;
/// モーダルダイアログの上端配置オフセット（高さが伸縮しても上端が動かないように固定）。
pub(super) const DIALOG_TOP: f32 = 40.;
/// 盤面上下の固定 UI 高さ合計（ツールバー＋ページ上下余白＋座標ラベル＋ナビゲーション行）。
pub(super) const BOARD_CHROME: f32 = TOOLBAR_HEIGHT + 32. + 18. + 56.;

/// デスクトップアプリケーションのルートビュー（GPUI が管理する画面全体の `Entity`）。
///
/// GPUI では `Render` トレイトを実装した構造体を `Entity` として登録し、
/// 状態が変わったときに `cx.notify()` を呼ぶことで `render` が再実行されて画面が更新される。
/// `Context<Self>` は非同期タスクの起動や再描画通知を担い、`Window` はフォーカスやアニメーションフレームを管理する。
pub struct RootView {
    pub(super) controller: Controller,
    /// 評価モデルが見つからない場合は `None`。
    engine: Option<EngineService>,
    /// エンジンスレッドから受け取った Book の状態（読み込み中は `None`）。
    pub(super) book_status: Option<BookStatus>,
    /// エンジン読み込み後に取得した評価モデルのメタ情報。
    pub(super) model_info: Option<ModelInfo>,
    /// 有効な対局用 Book が切り替わるたびに増加する世代番号。
    pub(super) book_selection_gen: u64,
    pub(super) book_work: BookWork,
    pub(super) settings: Settings,
    pub(super) dialog: Option<Dialog>,
    /// OS から保存用ディレクトリを取得できない場合は `None`（永続化無効）。
    pub(super) storage: Option<StorageService>,
    pub(super) paths: Option<Paths>,
    /// 最後にストレージへ保存要求を送ったコントローラのリビジョン番号（0 は未操作の初期状態）。
    pub(super) saved_revision: u64,
    pub(super) saved_settings: Settings,
    /// ファイル操作エラーやエクスポート完了などを知らせる一時通知。
    pub(super) notice: Option<SharedString>,
    pub(super) menu_open: bool,
    /// 対局一覧ダイアログ表示中の保存済み対局リスト。
    pub(super) games: Option<Vec<GameSummary>>,
    /// 削除確認待ち（1 回目の削除クリック済み）の対局 ID。
    pub(super) delete_pending: Option<String>,
    /// 局面エディタが開いている間の編集状態。
    pub(super) editor: Option<EditorState>,
    /// 直前の着手による石の反転アニメーション（盤面上に重ね描きする）。
    pub(super) animation: Option<FlipAnimation>,
    /// 実行中のアニメーションが AI の着手によるものか。
    animation_by_ai: bool,
    /// 前の人間手のアニメーション完了を待っている AI 着手イベント。
    held_move: Option<EngineEvent>,
    /// ポインタが乗っている評価値グラフ上の列インデックス。
    pub(super) graph_hover: Option<usize>,
    /// 評価値グラフのリサイズグリップドラッグ開始位置 `(pointer_y, initial_height)` とドラッグ中の仮高さ。
    pub(super) graph_drag: Option<(Pixels, u16)>,
    pub(super) graph_live: Option<u16>,
    /// 盤面上に表示中のパス通知（パスした色と表示開始時刻）。
    pub(super) pass_notice: Option<(Color, Instant)>,
    /// 棋譜入力ダイアログの入力状態（確定済みの着手文字列・手数単位キャレット・全選択フラグ・入力途中の 1 文字）。
    pub(super) record_input: String,
    pub(super) record_caret: usize,
    pub(super) record_all: bool,
    pub(super) record_pending: Option<char>,
    /// 棋譜入力ダイアログが対局読み込みではなく Book 展開起点の指定用として開かれているか。
    pub(super) record_for_book: bool,
    /// 棋譜コピー完了表示の重複タイマーを識別するためのトークン。
    pub(super) record_copied: Option<u64>,
    /// 現在開いているモーダルダイアログ間で共有するスクロールハンドル（開くたびにリセット）。
    pub(super) dialog_scroll: ScrollHandle,
    /// 新規対局ダイアログで選択中の Book 使用フラグ（設定から初期値を引き継ぐ）。
    pub(super) new_game_book: bool,
    pub(super) scroll_drag: Option<widgets::ScrollDrag>,
    audio: AudioService,
    /// 前フレームのウィンドウサイズ（終了時に設定へ保存する）。
    pub(super) viewport: Option<(u32, u32)>,
    /// `trace` フィーチャ有効時にアニメーション中のフレーム時刻を記録し、標準エラーへ出力する。
    trace: Option<Vec<Instant>>,
    _quit: Subscription,
    _appearance: Subscription,
    focus: FocusHandle,
    dialog_focus: FocusHandle,
    pub(super) board_focus: FocusHandle,
    pub(super) board_cursor: Square,
    launched: Instant,
    first_frame: Option<Duration>,
}

impl RootView {
    /// ルートビューを初期化し、エンジン・ストレージのイベント受信タスクおよび終了時フックを登録する。
    pub fn new(
        launched: Instant,
        engine: Option<(EngineService, async_channel::Receiver<EngineEvent>)>,
        storage: Option<(StorageService, Paths, async_channel::Receiver<StorageEvent>)>,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (storage, paths) = match storage {
            None => (None, None),
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
                (Some(service), Some(paths))
            }
        };
        // アプリ終了前に未保存の棋譜・設定の書き出しと Book 生成・エンジンスレッドの終了を完了させる。
        let quit = cx.on_app_quit(|this: &mut Self, _| {
            if this.viewport.is_some() {
                this.settings.window_size = this.viewport;
            }
            this.persist();
            if let Some(storage) = this.storage.as_mut() {
                storage.shutdown();
            }
            if let Some(engine) = this.engine.as_mut() {
                engine.shutdown();
            }
            async {}
        });
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
                        if let EngineEvent::Model(info) = &event {
                            this.model_info = Some(info.clone());
                        }
                        match &event {
                            EngineEvent::BookProgress(progress) => this.on_book_progress(*progress),
                            EngineEvent::BookDone(done) => this.on_book_done(done.clone(), cx),
                            _ => {}
                        }
                        // 人間の手による石反転アニメーション中に AI の着手が返った場合（Book 即答など）は
                        // アニメーション完了まで着手を保留する。AI 同士の対局ではテンポを保つため前アニメを打ち切る。
                        if this.animation.is_some()
                            && !this.animation_by_ai
                            && this.controller.is_ai_move(&event)
                        {
                            this.held_move = Some(event);
                            return;
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
            record_copied: None,
            dialog_scroll: ScrollHandle::new(),
            new_game_book: true,
            scroll_drag: None,
            audio: AudioService::spawn(),
            viewport: None,
            trace: cfg!(feature = "trace").then(Vec::new),
            _quit: quit,
            _appearance: cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            focus,
            dialog_focus: cx.focus_handle(),
            board_focus: cx.focus_handle(),
            board_cursor: "d3".parse().expect("a square name"),
            launched,
            first_frame: None,
            book_status: None,
            model_info: None,
            book_selection_gen: 0,
            book_work: BookWork::default(),
        };
        view.apply_book();
        view
    }

    pub(super) fn has_engine(&self) -> bool {
        self.engine.is_some()
    }

    /// コントローラへコマンドを適用し、返された副作用を実行して再描画を通知する。
    pub(super) fn run(&mut self, command: Command, cx: &mut Context<Self>) {
        // 局面エディタが開いている間は本体の対局操作を抑止する。
        if self.editor.is_some() {
            return;
        }
        self.notice = None;
        // 新しい着手以外の操作では進行中のアニメーションを即座に終了する。
        self.animation = None;
        self.pass_notice = None;
        let effects = self.controller.apply(command);
        self.dispatch(effects, cx);
        cx.notify();
    }

    /// コントローラから返された副作用（エンジンコマンド・効果音・アニメーション・パス通知）を実行する。
    fn dispatch(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
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
                    // 人間が参加している対局では誰がパスしたかを画面中央に短時間表示する。
                    let history = self.controller.history();
                    if sound == Sound::Pass
                        && self.controller.players().iter().any(|p| !p.is_ai())
                        && let Some(index) = history.cursor().checked_sub(1)
                    {
                        let passer = history.position(index).side_to_move();
                        let shown = Instant::now();
                        self.pass_notice = Some((passer, shown));
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(PASS_NOTICE).await;
                            this.update(cx, |this, cx| {
                                if this.pass_notice == Some((passer, shown)) {
                                    this.pass_notice = None;
                                    cx.notify();
                                }
                            })
                            .ok();
                        })
                        .detach();
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

    /// `trace` フィーチャ有効時、終了した直前のアニメーションのフレーム間隔（中央値・p95・最大値）を標準エラーへ出力する。
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

    /// 直前に記録された着手に対する反転アニメーションを開始する（既存アニメーションは上書き）。
    fn start_animation(&mut self) {
        let scale = match self.settings.animation {
            Animation::Normal => 1.,
            Animation::Short => 0.5,
            Animation::Off => 0.,
        };
        let history = self.controller.history();
        let actions = &history.actions()[..history.cursor()];
        // 着手の直後に強制パスが連なっている場合があるため、末尾から直近の Place を探す。
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

    /// モーダルダイアログを開く（フォーカス切替は次の `render` で行われる）。
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
        self.dialog_scroll = ScrollHandle::new();
        if matches!(dialog, Dialog::AiLog) {
            self.dialog_scroll.scroll_to_bottom();
        }
        self.scroll_drag = None;
        self.menu_open = false;
        self.dialog = Some(dialog);
        cx.notify();
    }

    pub(super) fn close_dialog(&mut self, cx: &mut Context<Self>) {
        // Book の生成中は進行状況を確認できるよう、停止するまでダイアログを閉じられないようにする。
        if self.book_work.running.is_some() {
            return;
        }
        // Book の起点指定のために棋譜入力を開いていた場合は、閉じたあと Book 作成ダイアログへ戻る。
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

    /// 矢印キー入力に応じて、現在の盤面回転・反転の向き通りにボードカーソルを移動する。
    fn move_board_cursor(&mut self, dx: i8, dy: i8, cx: &mut Context<Self>) {
        let view = self.settings.board_view;
        let (x, y) = view.place_of(self.board_cursor);
        let x = (x as i8 + dx).clamp(0, 7) as u8;
        let y = (y as i8 + dy).clamp(0, 7) as u8;
        self.board_cursor = view.square_at(x, y);
        cx.notify();
    }

    /// ウィンドウサイズとグラフ高さから盤面 1 マスの辺長を算出する（罫線が均等になるよう整数ピクセルへ切り下げ）。
    fn cell_size(window: &Window, graph: Option<f32>) -> Pixels {
        let viewport = window.viewport_size();
        // 左右カラム幅・余白・座標ラベル・ナビゲーション行・解析モード時のグラフ高さを差し引いて算出する。
        let width = f32::from(viewport.width) - LEFT_WIDTH - RIGHT_WIDTH - 2. * GAP - 40. - 18.;
        let graph = graph.map_or(0., |plot| {
            plot + super::analysis_panel::GRAPH_CHROME + super::analysis_panel::GRAPH_GRIP
        });
        let height = f32::from(viewport.height) - BOARD_CHROME - graph;
        // 8 マス分に加えて、マス間の内側の線 7 本と外枠の太さ分を差し引く。
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
            // 盤面の回転・左右反転の向きを切り替え、設定として保存する。
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
        // フォーカスはレンダー時に同期する（アクションハンドラが Window を直接持たないため）。
        if self.dialog.is_some() {
            if !self.dialog_focus.contains_focused(window, cx) {
                window.focus(&self.dialog_focus, cx);
            }
        } else if !self.focus.contains_focused(window, cx) {
            window.focus(&self.focus, cx);
        }
        // アニメーション実行中のみ次フレームを要求して描画負荷を抑える。
        if self.animation.as_ref().is_some_and(FlipAnimation::done) {
            self.animation = None;
            self.report_frames();
        }
        // 直前のアニメーション終了を待って保留していた AI 着手イベントをここで適用する
        // （その間に Undo や新規対局でキャンセルされていれば Controller 側で安全に無視される）。
        if self.animation.is_none()
            && let Some(event) = self.held_move.take()
        {
            let effects = self.controller.on_engine(event);
            self.dispatch(effects, cx);
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
        if self.animation.is_some() {
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
            // グラフのリサイズ中は、ポインタがグリップから外れてもウィンドウ全体でドラッグを追従する。
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
                        // エディタ表示中も右カラム分の幅を空けておき、盤面の中央位置が左右にずれないようにする。
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
                                    // 盤面の左端（座標ラベルを含む）から右端までと同じ幅でグラフを配置する。
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
                // ダイアログ外側のクリックで閉じ、内側のクリックは伝播を止める。
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_dialog(cx)),
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
                        // Enter キーは各ダイアログの主要な確定アクションを実行する。
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
            .child(workspace)
            .children(
                self.pass_notice
                    .map(|(color, _)| self.render_pass_notice(color, cx)),
            )
            .children(self.menu_open.then(|| self.render_file_menu(cx)))
            .children(dialog)
    }
}
