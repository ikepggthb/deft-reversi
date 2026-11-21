//! The book builder (§24, B3): a work file, growing it from the position
//! shown until stopped, and linking or unlinking the moves of that position.
use super::{
    analysis_panel::{action_text, signed},
    root::{Dialog, RootView},
    theme,
    widgets::{self, Handler, caption, heading},
};
use crate::{
    app::{Command, EngineStatus},
    services::{
        book_gen::{self, BookDone, BookJob, BookProgress, BookSettings, SharedEditor},
        engine::EngineCommand,
    },
};
use deft_reversi_book::{BookEditor, Bound, GenerationStop, ReadLimits};
use deft_reversi_engine::{Action, Color, StopHandle};
use gpui::{Context, Div, FontWeight, PathPromptOptions, SharedString, div, prelude::*, px};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) struct BookWork {
    /// The `.dbook-work`; None until one is made or opened.
    pub path: Option<PathBuf>,
    /// Shared with a running generation, which holds it until done.
    pub editor: SharedEditor,
    pub level: u8,
    pub max_plies: u8,
    pub black_loss: u8,
    pub white_loss: u8,
    /// 0 is no limit.
    pub cumulative_loss: u16,
    /// Stops the running generation, and when it began.
    pub running: Option<(StopHandle, Instant)>,
    pub progress: Option<BookProgress>,
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
        }
    }

    /// A work file opens with the settings it was last grown with.
    fn take_settings(&mut self, settings: &BookSettings) {
        self.level = settings.level;
        self.max_plies = settings.max_plies;
        self.black_loss = settings.black_loss;
        self.white_loss = settings.white_loss;
        self.cumulative_loss = settings.cumulative_loss.unwrap_or(0);
    }
}

impl RootView {
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

    pub(super) fn start_book(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.book_work.path.clone() else {
            return;
        };
        if self.book_work.running.is_some() || *self.controller.engine() != EngineStatus::Ready {
            return;
        }
        let stop = StopHandle::new();
        let job = BookJob {
            editor: self.book_work.editor.clone(),
            work: path,
            root: self.controller.history().current(),
            settings: self.book_work.settings(),
            stop: stop.clone(),
        };
        self.book_work.running = Some((stop, Instant::now()));
        self.book_work.progress = None;
        self.book_work.message = None;
        self.send_engine(EngineCommand::Generate(job));
        self.run(Command::GenerateBook, cx);
        // The clock goes on between the checkpoints.
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

    pub(super) fn stop_book(&mut self, cx: &mut Context<Self>) {
        if let Some((stop, _)) = &self.book_work.running {
            stop.stop();
        }
        cx.notify();
    }

    pub(super) fn on_book_progress(&mut self, progress: BookProgress) {
        self.book_work.progress = Some(progress);
    }

    /// The play book is read again from what the generation wrote.
    pub(super) fn on_book_done(&mut self, done: Result<BookDone, String>, cx: &mut Context<Self>) {
        let s = self.settings.lang.strings();
        self.book_work.running = None;
        match done {
            Ok(done) => {
                self.book_work.progress = Some(done.progress);
                let why = match done.reason {
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

    /// Links or unlinks a move of the position shown, then saves the work
    /// and the play book.
    fn set_link(&mut self, action: Action, link: bool, cx: &mut Context<Self>) {
        let Some(path) = self.book_work.path.clone() else {
            return;
        };
        let board = self.controller.history().current().board();
        let outcome = {
            let Ok(mut slot) = self.book_work.editor.try_lock() else {
                return;
            };
            let Some(editor) = slot.as_mut() else {
                return;
            };
            let changed = if link {
                editor.register_move(board, action)
            } else {
                editor.unregister_move(board, action)
            };
            changed
                .and_then(|()| editor.save(&path))
                .and_then(|()| editor.export()?.save(book_gen::play_path(&path)))
        };
        let s = self.settings.lang.strings();
        match outcome {
            Ok(()) => self.set_book_path(Some(book_gen::play_path(&path)), cx),
            Err(error) => self.book_work.message = Some(s.book_error(&error.to_string()).into()),
        }
        cx.notify();
    }

    pub(super) fn render_book_work(&self, cx: &mut Context<Self>) -> Div {
        let s = self.settings.lang.strings();
        let work = &self.book_work;
        let running = work.running.is_some();
        let position = self.controller.history().current();
        // A running generation holds the editor.
        let slot = work.editor.try_lock().ok();
        let editor = slot.as_ref().and_then(|slot| slot.as_ref());

        let row = |label: &'static str, control: Div, note: Option<SharedString>| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .min_h(px(32.))
                .child(div().w(px(130.)).flex_none().child(label))
                .child(control)
                .children(note.map(|n| {
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(n)
                }))
        };
        // Settings change only between runs.
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
        let discs = position.disc_count(Color::Black) + position.disc_count(Color::White);
        let from = div().text_color(theme::text_secondary()).child(format!(
            "{}（{}）",
            s.book_from_current,
            s.move_number(discs as usize - 3)
        ));

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
        let mut loss = |id: &'static str, value: u8, down: fn(&mut BookWork), up: fn(&mut BookWork)| {
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

        let ready = work.path.is_some() && *self.controller.engine() == EngineStatus::Ready;
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
                widgets::handler(|this, cx| this.start_book(cx)),
                cx,
            )
            .when(!ready, |el| el.opacity(0.4))
        };
        // Positions come at each save; the clock runs on its own.
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
            .children(running.then(|| {
                div()
                    .text_color(theme::accent())
                    .child(s.book_generating)
            }))
            .children(progress.map(|p| div().text_color(theme::text_secondary()).child(p)));

        // The moves of the position shown, with what the work knows of them.
        let moves: Div = match editor {
            _ if running => caption(s.book_generating),
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
                    div().flex().flex_col().children(rows.into_iter().enumerate().map(
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
            .child(row(s.book_max_plies, plies, Some(s.book_plies_note.into())))
            .child(row(s.book_loss, losses, Some(s.book_loss_note.into())))
            .child(row(
                s.book_cumulative,
                cumulative,
                Some(s.book_cumulative_note.into()),
            ))
            .child(div().mt(px(6.)).child(status))
            .children(work.message.clone().map(|m| {
                div().text_sm().text_color(theme::text_secondary()).child(m)
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(s.book_work_note),
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

        div()
            .w(px(680.))
            .max_h(px(self.viewport.map_or(900., |(_, h)| h as f32 - 60.)))
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
                            .child(s.book_work),
                    )
                    .child(widgets::link(
                        "close-book-work",
                        s.close,
                        widgets::handler(|this, cx| this.close_dialog(cx)),
                        cx,
                    )),
            )
            .child(widgets::scroll_area(
                "book-work-body",
                &self.book_scroll,
                body,
                cx,
            ))
    }

    pub(super) fn open_book_work_dialog(&mut self, cx: &mut Context<Self>) {
        self.open_dialog(Dialog::BookWork, cx);
    }
}
