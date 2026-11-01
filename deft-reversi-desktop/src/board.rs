//! P0 board: 64 GPUI elements, clicks played through `Position`, and the
//! engine's load status. The game state moves to `domain` in P1.
use crate::engine_loader::Loaded;
use deft_reversi_engine::{Action, Color, EngineWorker, Goal, Position, SearchRequest, Square};
use gpui::{
    ClickEvent, Context, Hsla, IntoElement, Pixels, SharedString, Window, div, prelude::*, px, rgb,
};
use std::time::{Duration, Instant};

enum EngineState {
    Missing,
    Loading,
    Ready(EngineWorker),
    Failed(String),
}

pub struct BoardView {
    position: Position,
    last_move: Option<Square>,
    engine: EngineState,
    launched: Instant,
    first_frame: Option<Duration>,
    timings: Vec<SharedString>,
    hint: Option<SharedString>,
    searching: bool,
}

impl BoardView {
    pub fn new(
        launched: Instant,
        loading: Option<async_channel::Receiver<Result<Loaded, String>>>,
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
        Self {
            position: Position::initial(),
            last_move: None,
            engine,
            launched,
            first_frame: None,
            timings: Vec::new(),
            hint: None,
            searching: false,
        }
    }

    fn on_loaded(&mut self, loaded: Result<Loaded, String>, cx: &mut Context<Self>) {
        let ready_at = self.launched.elapsed();
        self.engine = match loaded {
            Ok(loaded) => {
                let line = format!(
                    "model {} ms, engine {} ms ({} threads, TT {} MiB), ready at {} ms",
                    loaded.model_time.as_millis(),
                    loaded.engine_time.as_millis(),
                    loaded.threads,
                    loaded.tt_mib,
                    ready_at.as_millis(),
                );
                eprintln!("[startup] {line} from {}", loaded.model_path.display());
                self.timings.push(line.into());
                EngineState::Ready(loaded.worker)
            }
            Err(error) => {
                eprintln!("[startup] engine failed: {error}");
                EngineState::Failed(error)
            }
        };
        cx.notify();
    }

    fn play(&mut self, square: Square, cx: &mut Context<Self>) {
        if self.searching {
            return;
        }
        let Ok(mut next) = self.position.play(square) else {
            return;
        };
        if next.must_pass() {
            next = next.play(Action::Pass).expect("a pass is legal here");
        }
        self.position = next;
        self.last_move = Some(square);
        self.hint = None;
        cx.notify();
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        if self.searching {
            return;
        }
        self.position = Position::initial();
        self.last_move = None;
        self.hint = None;
        cx.notify();
    }

    /// Checks that a search runs without blocking the window.
    fn ask_engine(&mut self, cx: &mut Context<Self>) {
        let EngineState::Ready(worker) = &self.engine else {
            return;
        };
        if self.searching || self.position.is_terminal() {
            return;
        }
        let request = SearchRequest::new(Goal::Level(18)).time_limit(Duration::from_secs(1));
        let handle = match worker.start(self.position, request) {
            Ok(handle) => handle,
            Err(error) => {
                self.hint = Some(format!("search failed: {error}").into());
                cx.notify();
                return;
            }
        };
        // `wait` blocks, so a short-lived thread hands the result back.
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = sender.send_blocking(handle.wait());
        });
        self.searching = true;
        let side = self.position.side_to_move();
        cx.spawn(async move |this, cx| {
            let result = receiver.recv().await;
            this.update(cx, |this, cx| {
                this.searching = false;
                this.hint = Some(match result {
                    Ok(Ok(result)) => {
                        // Scores are from the side to move; show Black's view.
                        let score = result
                            .score
                            .map(|s| if side == Color::Black { s } else { -s });
                        let best = result.best_move.map_or("-".into(), |m| m.to_string());
                        let score = score.map_or("?".into(), |s| format!("{s:+}"));
                        format!(
                            "best {best}, Black {score}, {} nodes, {} ms",
                            result.stats.nodes,
                            result.elapsed.as_millis()
                        )
                        .into()
                    }
                    Ok(Err(error)) => format!("search failed: {error}").into(),
                    Err(_) => "search thread ended".into(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn render_board(&self, side: Pixels, cx: &mut Context<Self>) -> impl IntoElement {
        let cell = side / 8.0;
        let legal = self.position.legal_moves();
        let rows = (0..8).rev().map(|y| {
            div().flex().flex_row().children((0..8).map(|x| {
                let square = Square::from_xy(x, y).expect("x and y are 0..8");
                let bit = square.bit();
                let disc = if self.position.black() & bit != 0 {
                    Some(rgb(0x111111))
                } else if self.position.white() & bit != 0 {
                    Some(rgb(0xf4f4f4))
                } else {
                    None
                };
                let mut cell_el = div()
                    .id(("square", usize::from(square.index())))
                    .size(cell)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(0x2e7d4f))
                    .border_1()
                    .border_color(rgb(0x1b4d30));
                if self.last_move == Some(square) {
                    cell_el = cell_el.bg(rgb(0x3b9562));
                }
                if let Some(color) = disc {
                    cell_el = cell_el.child(div().size(cell * 0.8).rounded_full().bg(color));
                } else if legal.contains(square) {
                    cell_el = cell_el
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0x358a5a)))
                        .child(div().size(cell * 0.2).rounded_full().bg(legal_dot()))
                        .on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.play(square, cx)),
                        );
                }
                cell_el
            }))
        });
        div()
            .flex()
            .flex_col()
            .border_2()
            .border_color(rgb(0x1b4d30))
            .children(rows)
    }

    fn render_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pos = &self.position;
        let turn: SharedString = if pos.is_terminal() {
            let diff = pos.final_score(Color::Black);
            format!("Game over: Black {diff:+}").into()
        } else if pos.side_to_move() == Color::Black {
            "Black to move".into()
        } else {
            "White to move".into()
        };
        let engine: SharedString = match &self.engine {
            EngineState::Missing => "AI: model not found (see data/eval/)".into(),
            EngineState::Loading => "AI: loading…".into(),
            EngineState::Ready(_) if self.searching => "AI: thinking…".into(),
            EngineState::Ready(_) => "AI: ready".into(),
            EngineState::Failed(error) => format!("AI: failed ({error})").into(),
        };
        let first_frame: SharedString = match self.first_frame {
            Some(t) => format!("first frame {} ms", t.as_millis()).into(),
            None => "".into(),
        };
        let ready = matches!(self.engine, EngineState::Ready(_)) && !self.searching;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(300.))
            .p_4()
            .text_sm()
            .text_color(rgb(0xe8e8e8))
            .child(div().text_lg().child(turn))
            .child(format!(
                "Black {}  White {}",
                pos.disc_count(Color::Black),
                pos.disc_count(Color::White)
            ))
            .child(engine)
            .child(
                button("new-game", "New game", true)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reset(cx))),
            )
            .child(
                button("ask-engine", "Ask engine (L18, 1 s)", ready)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.ask_engine(cx))),
            )
            .children(self.hint.clone())
            .child(div().text_xs().text_color(rgb(0x9a9a9a)).child(first_frame))
            .children(self.timings.iter().map(|line| {
                div()
                    .text_xs()
                    .text_color(rgb(0x9a9a9a))
                    .child(line.clone())
            }))
    }
}

fn legal_dot() -> Hsla {
    Hsla::from(rgb(0x000000)).opacity(0.35)
}

fn button(id: &'static str, label: &'static str, enabled: bool) -> gpui::Stateful<gpui::Div> {
    let el = div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0x3a3a3a))
        .child(label);
    if enabled {
        el.cursor_pointer().hover(|style| style.bg(rgb(0x4a4a4a)))
    } else {
        el.text_color(rgb(0x777777))
    }
}

impl Render for BoardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.first_frame.is_none() {
            let t = self.launched.elapsed();
            eprintln!("[startup] first frame at {} ms", t.as_millis());
            self.first_frame = Some(t);
        }
        let viewport = window.viewport_size();
        let side = (viewport.width - px(300.) - px(32.))
            .min(viewport.height - px(32.))
            .max(px(160.));
        div()
            .size_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgb(0x202124))
            .child(self.render_board(side, cx))
            .child(self.render_panel(cx))
    }
}
