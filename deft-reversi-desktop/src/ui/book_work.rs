//! Book 作業ファイル（`.dbook-work`）の作成・読み込み・Book 展開・修復・手リンク編集を担うダイアログモジュール。
//!
//! 指定局面から停止されるまで（または許容損失内の展開が完了するまで）バックグラウンドで探索を進め、
//! 10 秒ごとに作業ファイルと対局用 `.dbook` ファイルを保存する。
use super::{
    format::{action_text, signed},
    root::{Dialog, RootView},
    theme,
    widgets::{self, Handler, caption, heading},
};
use crate::domain::History;
use crate::{
    app::{Command, EngineStatus},
    services::{
        book_gen::{self, BookDone, BookJob, BookProgress, BookSettings, SharedEditor},
        engine::EngineCommand,
    },
};
use deft_reversi_book::{BookEditor, Bound, GenerationStop, ReadLimits};
use deft_reversi_engine::{Action, Color, Position, StopHandle};
use gpui::{Context, Div, PathPromptOptions, SharedString, div, prelude::*, px};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// Book を展開する起点局面の選択方法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RootChoice {
    /// 現在盤面に表示中の局面から展開する。
    Current,
    /// 通常の初期局面から展開する。
    Initial,
    /// 入力された棋譜の最終局面から展開する（途中の手順も自動的にリンク登録する）。
    Record,
}

/// Book 作成ダイアログの UI 状態と共有エディタハンドル。
pub(super) struct BookWork {
    /// 対象の `.dbook-work` ファイルパス（新規作成または開くまで `None`）。
    pub path: Option<PathBuf>,
    /// Book 生成スレッドと共有する作業エディタ（生成中はスレッド側がロックを保持する）。
    pub editor: SharedEditor,
    /// 探索レベル。
    pub level: u8,
    /// 初期局面からの最大手数。
    pub max_plies: u8,
    /// 黒番の許容損失（石差）。
    pub black_loss: u8,
    /// 白番の許容損失（石差）。
    pub white_loss: u8,
    /// 変化に沿った累積損失の上限（0 は無制限）。
    pub cumulative_loss: u16,
    /// 末端手のリンク登録を許可する最大損失（`None` は無制限）。
    pub leaf_loss: Option<u8>,
    /// 次回実行時の展開起点。
    pub root: RootChoice,
    /// `RootChoice::Record` 用に入力された棋譜。
    pub record: Option<History>,
    /// 実行中のジョブが展開ではなく修復（`fix`）であるか。
    pub fixing: bool,
    /// 手リンク編集結果をバックグラウンドで保存中であれば `true`。
    pub saving: bool,
    /// 実行中の Book 生成を停止するためのハンドルと開始時刻。
    pub running: Option<(StopHandle, Instant)>,
    /// 最新のチェックポイント進捗。
    pub progress: Option<BookProgress>,
    /// 完了・エラー等の通知メッセージ。
    pub message: Option<SharedString>,
}

impl Default for BookWork {
    fn default() -> Self {
        let mut work = Self {
            path: None,
            editor: SharedEditor::default(),
            level: 16,
            max_plies: 0,
            black_loss: 0,
            white_loss: 0,
            cumulative_loss: 0,
            leaf_loss: None,
            root: RootChoice::Current,
            record: None,
            fixing: false,
            saving: false,
            running: None,
            progress: None,
            message: None,
        };
        work.take_settings(&BookSettings::of(None));
        work
    }
}

impl BookWork {
    fn settings(&self) -> BookSettings {
        BookSettings {
            level: self.level,
            max_plies: self.max_plies,
            black_loss: self.black_loss,
            white_loss: self.white_loss,
            cumulative_loss: (self.cumulative_loss > 0).then_some(self.cumulative_loss),
            leaf_loss: self.leaf_loss,
        }
    }

    /// 作業ファイルに記録されていた前回の生成パラメータをダイアログ状態へ取り込む。
    fn take_settings(&mut self, settings: &BookSettings) {
        self.level = settings.level;
        self.max_plies = settings.max_plies;
        self.black_loss = settings.black_loss;
        self.white_loss = settings.white_loss;
        self.cumulative_loss = settings.cumulative_loss.unwrap_or(0);
        self.leaf_loss = settings.leaf_loss;
    }
}

impl RootView {
    /// 新規 `.dbook-work` ファイルの保存先を選択する。
    pub(super) fn new_book_work(&mut self, cx: &mut Context<Self>) {
        let directory = dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| ".".into());
        let path = cx.prompt_for_new_path(&directory, Some("openings.dbook-work"));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = path.await {
                this.update(cx, |this, cx| {
                    this.book_work = BookWork {
                        path: Some(path),
                        ..BookWork::default()
                    };
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 既存の `.dbook-work` ファイルを選択し、バックグラウンドスレッドで読み込む。
    pub(super) fn open_book_work(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = path.clone();
            let loaded = cx
                .background_spawn(async move { BookEditor::load(&read, ReadLimits::default()) })
                .await;
            this.update(cx, |this, cx| {
                let s = this.settings.lang.strings();
                match loaded {
                    Ok(editor) => {
                        let mut work = BookWork {
                            path: Some(path),
                            ..BookWork::default()
                        };
                        work.take_settings(&BookSettings::of(Some(&editor)));
                        work.editor = SharedEditor::new(Some(editor).into());
                        this.book_work = work;
                    }
                    Err(error) => {
                        this.book_work.message = Some(s.book_error(&error.to_string()).into())
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 展開起点局面と、そこに至るまでに事前リンクすべき手順を返す。
    fn book_root(&self) -> Option<(Position, Vec<(Position, Action)>)> {
        match self.book_work.root {
            RootChoice::Current => Some((self.controller.history().current(), Vec::new())),
            RootChoice::Initial => Some((Position::initial(), Vec::new())),
            RootChoice::Record => {
                let record = self.book_work.record.as_ref()?;
                let line = record
                    .actions()
                    .iter()
                    .enumerate()
                    .map(|(i, &action)| (record.position(i), action))
                    .collect();
                Some((record.current(), line))
            }
        }
    }

    /// Book の自動展開（`fix == false`）または整合修復（`fix == true`）をバックグラウンドで開始する。
    pub(super) fn start_book(&mut self, fix: bool, cx: &mut Context<Self>) {
        let Some(path) = self.book_work.path.clone() else {
            return;
        };
        if self.book_work.running.is_some()
            || self.book_work.saving
            || *self.controller.engine() != EngineStatus::Ready
        {
            return;
        }
        let (root, line) = if fix {
            let can_fix = self
                .book_work
                .editor
                .try_lock()
                .ok()
                .is_some_and(|slot| slot.as_ref().is_some_and(|e| !e.is_empty()));
            if !can_fix {
                return;
            }
            (Position::initial(), Vec::new())
        } else {
            let Some((root, line)) = self.book_root() else {
                return;
            };
            if root.is_terminal() {
                return;
            }
            (root, line)
        };
        let stop = StopHandle::new();
        let job = BookJob {
            editor: self.book_work.editor.clone(),
            work: path,
            root,
            line,
            fix,
            settings: self.book_work.settings(),
            stop: stop.clone(),
        };
        self.book_work.fixing = fix;
        self.book_work.running = Some((stop, Instant::now()));
        self.book_work.progress = None;
        self.book_work.message = None;
        self.send_engine(EngineCommand::Generate(job));
        self.run(Command::GenerateBook, cx);
        // チェックポイント間も経過時間表示が止まらないよう、1 秒ごとに再描画をトリガする。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let running = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.book_work.running.is_some()
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        })
        .detach();
    }

    /// 実行中の Book 生成に停止シグナルを送る。
    pub(super) fn stop_book(&mut self, cx: &mut Context<Self>) {
        if let Some((stop, _)) = &self.book_work.running {
            stop.stop();
        }
        cx.notify();
    }

    /// Book 生成のチェックポイント進捗を更新する。
    pub(super) fn on_book_progress(&mut self, progress: BookProgress) {
        self.book_work.progress = Some(progress);
    }

    /// Book 生成・修復の完了を受け、対局用 `.dbook` を再読み込みして完了メッセージを表示する。
    pub(super) fn on_book_done(&mut self, done: Result<BookDone, String>, cx: &mut Context<Self>) {
        let s = self.settings.lang.strings();
        self.book_work.running = None;
        match done {
            Ok(done) => {
                self.book_work.progress = Some(done.progress);
                let why = match done.reason {
                    GenerationStop::Exhausted if self.book_work.fixing => s.book_fixed,
                    GenerationStop::Exhausted => s.book_exhausted,
                    GenerationStop::MemoryLimit => s.book_memory,
                    _ => s.book_stopped,
                };
                self.book_work.message = Some(format!("{why}{}", s.book_updated).into());
                self.set_book_path(Some(done.book), cx);
            }
            Err(error) if error == "model" => {
                self.book_work.message = Some(s.book_other_work.into())
            }
            Err(error) => self.book_work.message = Some(s.book_error(&error).into()),
        }
    }

    /// 現在表示中の局面における指定手のリンク登録・解除を行い、作業ファイルと対局用ブックをバックグラウンド保存する。
    fn set_link(&mut self, action: Action, link: bool, cx: &mut Context<Self>) {
        if self.book_work.running.is_some() || self.book_work.saving {
            return;
        }
        let Some(path) = self.book_work.path.clone() else {
            return;
        };
        let board = self.controller.history().current().board();
        let editor = self.book_work.editor.clone();
        let editor_bg = editor.clone();
        let book_gen = self.book_selection_gen;
        self.book_work.saving = true;
        self.book_work.message = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    let mut slot = editor_bg
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let Some(editor) = slot.as_mut() else {
                        return Ok(None);
                    };
                    if link {
                        editor.register_move(board, action)?;
                    } else {
                        editor.unregister_move(board, action)?;
                    }
                    editor.save(&path)?;
                    let play = book_gen::play_path(&path);
                    editor.export()?.save(&play)?;
                    Ok::<_, deft_reversi_book::BookError>(Some(play))
                })
                .await;
            this.update(cx, |this, cx| {
                let same_work = std::sync::Arc::ptr_eq(&this.book_work.editor, &editor);
                if same_work {
                    this.book_work.saving = false;
                }
                let s = this.settings.lang.strings();
                match outcome {
                    Ok(Some(play)) => {
                        if same_work && this.book_selection_gen == book_gen {
                            this.set_book_path(Some(play), cx);
                        } else if same_work {
                            cx.notify();
                        }
                    }
                    Ok(None) => {
                        if same_work {
                            cx.notify();
                        }
                    }
                    Err(error) => {
                        if same_work {
                            this.book_work.message = Some(s.book_error(&error.to_string()).into());
                            cx.notify();
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Book 作成モーダルダイアログを描画する。
    pub(super) fn render_book_work(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let work = &self.book_work;
        let running = work.running.is_some();
        let position = self.controller.history().current();
        // 生成スレッドが BookEditor のロックを保持している間は try_lock が失敗し None となる。
        let slot = work.editor.try_lock().ok();
        let editor = slot.as_ref().and_then(|slot| slot.as_ref());

        let row = |label: &'static str, control: Div, note: Option<SharedString>| {
            widgets::form_row(130., label, control, note)
        };
        // 生成設定は停止中のみ編集可能とする。
        let edit = |f: fn(&mut BookWork)| -> Handler {
            widgets::handler(move |this, _| {
                if this.book_work.running.is_none() {
                    f(&mut this.book_work)
                }
            })
        };

        let file_name = work.path.as_ref().map(|p| {
            p.file_name()
                .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
        });
        let positions = editor
            .map(|e| e.len())
            .or(work.progress.map(|p| p.positions));
        let file = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .max_w(px(220.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(if file_name.is_some() {
                        theme::text()
                    } else {
                        theme::text_muted()
                    })
                    .child(file_name.unwrap_or_else(|| s.book_none.into())),
            )
            .children(positions.map(|n| {
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(s.book_loaded(n))
            }))
            .when(!running, |el| {
                el.child(widgets::button(
                    "book-new",
                    s.book_new,
                    widgets::handler(|this, cx| this.new_book_work(cx)),
                    cx,
                ))
                .child(widgets::button(
                    "book-open",
                    s.book_open,
                    widgets::handler(|this, cx| this.open_book_work(cx)),
                    cx,
                ))
            });
        let move_no = super::record::move_number(position);
        let choose = |choice: RootChoice| -> Handler {
            widgets::handler(move |this, cx| {
                if this.book_work.running.is_some() {
                    return;
                }
                this.book_work.root = choice;
                // 「棋譜から」を初めて選んだときは、そのまま棋譜入力ダイアログを開く。
                if choice == RootChoice::Record && this.book_work.record.is_none() {
                    this.enter_book_record(cx);
                }
            })
        };
        let choices = widgets::segmented(
            "book-root",
            vec![
                (
                    format!("{}（{}）", s.book_from_current, s.move_number(move_no)).into(),
                    work.root == RootChoice::Current,
                    choose(RootChoice::Current),
                ),
                (
                    s.book_from_initial.into(),
                    work.root == RootChoice::Initial,
                    choose(RootChoice::Initial),
                ),
                (
                    s.book_from_record.into(),
                    work.root == RootChoice::Record,
                    choose(RootChoice::Record),
                ),
            ],
            cx,
        );
        // 選択中の棋譜（手順の抜粋と手数）および再入力リンクを表示する。
        let record = (work.root == RootChoice::Record).then(|| {
            let text = work
                .record
                .as_ref()
                .map(|r| crate::domain::transcript::to_text(r.actions()));
            let moves = work.record.as_ref().map_or(0, |r| r.actions().len());
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .text_xs()
                .child(
                    div()
                        .max_w(px(300.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(theme::text_secondary())
                        .child(text.unwrap_or_default()),
                )
                .child(div().text_color(theme::text_muted()).child(s.moves(moves)))
                .when(!running, |el| {
                    el.child(widgets::link(
                        "book-record",
                        s.enter_record,
                        widgets::handler(|this, cx| this.enter_book_record(cx)),
                        cx,
                    ))
                })
        });
        let from = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .py(px(4.))
            .child(div().flex().child(choices))
            .children(record);

        let level = widgets::stepper(
            "book-level",
            format!("Lv {}", work.level),
            edit(|w| w.level = w.level.saturating_sub(1).max(1)),
            edit(|w| w.level = (w.level + 1).min(60)),
            cx,
        );
        let plies = widgets::stepper(
            "book-plies",
            s.book_plies(work.max_plies),
            edit(|w| w.max_plies = w.max_plies.saturating_sub(1).max(1)),
            edit(|w| w.max_plies = (w.max_plies + 1).min(60)),
            cx,
        );
        let mut loss =
            |id: &'static str, value: u8, down: fn(&mut BookWork), up: fn(&mut BookWork)| {
                widgets::stepper(id, value.to_string(), edit(down), edit(up), cx)
            };
        let losses = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(super::analysis_panel::disc(16., Color::Black))
            .child(loss(
                "book-black-loss",
                work.black_loss,
                |w| w.black_loss = w.black_loss.saturating_sub(1),
                |w| w.black_loss = (w.black_loss + 1).min(32),
            ))
            .child(div().w(px(6.)))
            .child(super::analysis_panel::disc(16., Color::White))
            .child(loss(
                "book-white-loss",
                work.white_loss,
                |w| w.white_loss = w.white_loss.saturating_sub(1),
                |w| w.white_loss = (w.white_loss + 1).min(32),
            ));
        let cumulative = widgets::stepper(
            "book-cumulative",
            if work.cumulative_loss == 0 {
                s.book_no_limit.to_string()
            } else {
                work.cumulative_loss.to_string()
            },
            edit(|w| w.cumulative_loss = w.cumulative_loss.saturating_sub(1)),
            edit(|w| w.cumulative_loss = (w.cumulative_loss + 1).min(64)),
            cx,
        );
        // 1 手あたりの許容損失より小さい値にしたときだけ意味があるため、上限に達したら「無制限」として扱う。
        let leaf = widgets::stepper(
            "book-leaf",
            work.leaf_loss
                .map_or_else(|| s.book_no_limit.to_string(), |n| n.to_string()),
            edit(|w| {
                let most = w.black_loss.max(w.white_loss);
                w.leaf_loss = Some(match w.leaf_loss {
                    None => most.saturating_sub(1),
                    Some(n) => n.saturating_sub(1),
                });
            }),
            edit(|w| {
                let most = w.black_loss.max(w.white_loss);
                w.leaf_loss = w.leaf_loss.map(|n| n + 1).filter(|&n| n < most);
            }),
            cx,
        );

        let engine_ready = *self.controller.engine() == EngineStatus::Ready;
        let ready = work.path.is_some()
            && engine_ready
            && self
                .book_root()
                .is_some_and(|(root, _)| !root.is_terminal());
        let can_fix = engine_ready && editor.is_some_and(|e| !e.is_empty());
        let action = if running {
            widgets::button(
                "book-stop",
                s.book_stop,
                widgets::handler(|this, cx| this.stop_book(cx)),
                cx,
            )
        } else {
            widgets::primary(
                "book-start",
                s.book_start,
                widgets::handler(|this, cx| this.start_book(false, cx)),
                cx,
            )
            .when(!ready, |el| el.opacity(0.4))
        };
        let fix = (!running).then(|| {
            widgets::button(
                "book-fix",
                s.book_fix,
                widgets::handler(|this, cx| this.start_book(true, cx)),
                cx,
            )
            .when(!can_fix, |el| el.opacity(0.4))
        });
        // 局面数は 10 秒ごとの保存時に更新されるため、経過時間だけは独立したタイマーで毎秒進める。
        let elapsed = work
            .running
            .as_ref()
            .map(|(_, started)| started.elapsed())
            .or(work.progress.map(|p| p.elapsed));
        let progress = elapsed.map(|elapsed| {
            let p = work.progress.unwrap_or(BookProgress {
                positions: positions.unwrap_or(0),
                new_positions: 0,
                nodes: 0,
                elapsed,
            });
            s.book_progress(p.positions, p.new_positions, elapsed.as_secs())
        });
        let status = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .child(action)
            .children(fix)
            .children(running.then(|| {
                div().text_color(theme::accent()).child(if work.fixing {
                    s.book_fixing
                } else {
                    s.book_generating
                })
            }))
            .children(progress.map(|p| div().text_color(theme::text_secondary()).child(p)));

        // 現在表示中の局面について、作業ブックに登録済みの候補手とリンク状態を表示する。
        let moves: Div = match editor {
            _ if running => caption(s.book_generating),
            _ if work.saving => caption(s.book_loading),
            None => caption(s.book_not_in),
            Some(editor) => {
                let board = position.board();
                let mut rows: Vec<(Action, Option<String>, bool)> = position
                    .legal_moves()
                    .map(Action::Place)
                    .filter_map(|action| {
                        let values = editor.move_values(board, action)?;
                        let value = values.selected.map(|v| {
                            let score = signed(i32::from(v.result().value()));
                            match v.result().bound() {
                                Bound::Point => score,
                                Bound::Lower => format!("≥{score}"),
                                Bound::Upper => format!("≤{score}"),
                            }
                        });
                        Some((action, value, values.registered))
                    })
                    .collect();
                rows.sort_by_key(|(action, _, linked)| (!linked, action_text(*action)));
                if rows.is_empty() {
                    caption(s.book_not_in)
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .children(rows.into_iter().enumerate().map(
                            |(i, (action, value, linked))| {
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .h(px(24.))
                                    .child(div().w(px(40.)).child(action_text(action)))
                                    .child(
                                        div()
                                            .w(px(60.))
                                            .text_color(if linked {
                                                theme::text()
                                            } else {
                                                theme::text_muted()
                                            })
                                            .child(value.unwrap_or_else(|| "–".into())),
                                    )
                                    .child(widgets::link(
                                        ("book-link", i),
                                        if linked { s.book_unlink } else { s.book_link },
                                        widgets::handler(move |this, cx| {
                                            this.set_link(action, !linked, cx)
                                        }),
                                        cx,
                                    ))
                            },
                        ))
                }
            }
        };

        let body = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(row(s.book_work_file, file, None))
            .child(row(s.book_from, from, None))
            .child(row(s.book_level, level, None))
            .child(
                div()
                    .pl(px(130. + 12.))
                    .pb(px(8.))
                    .child(super::new_game::level_chart(s, work.level, 440.)),
            )
            .child(row(s.book_max_plies, plies, Some(s.book_plies_note.into())))
            .child(row(s.book_loss, losses, Some(s.book_loss_note.into())))
            .child(row(
                s.book_cumulative,
                cumulative,
                Some(s.book_cumulative_note.into()),
            ))
            .child(row(s.book_leaf, leaf, Some(s.book_leaf_note.into())))
            .child(div().mt(px(6.)).child(status))
            .children(
                work.message
                    .clone()
                    .map(|m| div().text_sm().text_color(theme::text_secondary()).child(m)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(s.book_work_note),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(s.book_fix_note),
            )
            .child(
                div()
                    .mt(px(6.))
                    .pt(px(10.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(heading(s.book_moves))
                    .child(moves),
            );

        widgets::dialog_box(680.)
            .max_h(px(self.viewport.map_or(900., |(_, h)| h as f32 - 60.)))
            .child(widgets::dialog_header(
                s.book_work,
                (!running).then_some(("close-book-work", s.close)),
                cx,
            ))
            .child(widgets::scroll_area(
                "book-work-body",
                &self.dialog_scroll,
                body,
                cx,
            ))
    }

    /// Book 作成ダイアログを開く。
    pub(super) fn open_book_work_dialog(&mut self, cx: &mut Context<Self>) {
        self.open_dialog(Dialog::BookWork, cx);
    }
}
