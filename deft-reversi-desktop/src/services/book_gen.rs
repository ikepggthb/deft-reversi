//! Growing the opening book from a position (§24, B3). It runs on a thread
//! of its own with the engine thread's engine, which searches nothing else
//! meanwhile, and saves the work file as it goes.
use super::engine::EngineEvent;
use deft_reversi_book::{
    AnalysisSchedule, BookEditor, BookSide, GenerationConfig, GenerationLimits, GenerationRoot,
    GenerationStop, SchedulePolicy, analysis_identity,
};
use deft_reversi_engine::{Color, EngineWorker, Position, StopHandle};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::Duration,
};

/// The work file being edited; None until the first generation makes it.
pub type SharedEditor = Arc<Mutex<Option<BookEditor>>>;

pub struct BookJob {
    pub editor: SharedEditor,
    /// The `.dbook-work` it is saved to; the `.dbook` for play goes next
    /// to it.
    pub work: PathBuf,
    /// Where the book grows from.
    pub root: Position,
    pub settings: BookSettings,
    /// It runs until stopped, or until nothing is left within the losses.
    pub stop: StopHandle,
}

/// What the book builder sets; the work file keeps it between runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookSettings {
    /// Every position is read at this one level.
    pub level: u8,
    /// Moves played from the start position, passes not counted.
    pub max_plies: u8,
    /// How many discs worse than the best a move may be and still be
    /// followed, for each colour.
    pub black_loss: u8,
    pub white_loss: u8,
    /// The most the losses along one line may add up to.
    pub cumulative_loss: Option<u16>,
}

impl BookSettings {
    /// What a work file was last grown with, or the defaults.
    pub fn of(editor: Option<&BookEditor>) -> Self {
        let config = editor
            .and_then(BookEditor::generation_config)
            .cloned()
            .unwrap_or_default();
        // A file grown elsewhere in stages opens at its finest level.
        let level = match &config.analysis {
            AnalysisSchedule::Levels(levels) => levels.last().copied(),
            _ => None,
        };
        Self {
            level: level.unwrap_or(16),
            max_plies: config.max_plies,
            black_loss: config.black_loss.unwrap_or(config.expansion_loss),
            white_loss: config.white_loss.unwrap_or(config.expansion_loss),
            cumulative_loss: config.cumulative_loss,
        }
    }

    /// The most promising lines are always followed first.
    fn apply(&self, config: &mut GenerationConfig) {
        config.analysis = AnalysisSchedule::Levels(vec![self.level]);
        config.max_plies = self.max_plies;
        config.expansion_loss = self.black_loss.max(self.white_loss);
        config.black_loss = Some(self.black_loss);
        config.white_loss = Some(self.white_loss);
        config.cumulative_loss = self.cumulative_loss;
        config.schedule = SchedulePolicy::Promising;
    }
}

impl std::fmt::Debug for BookJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BookJob")
            .field("work", &self.work)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// Saved so far, at a checkpoint or at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookProgress {
    pub positions: usize,
    pub new_positions: u64,
    pub nodes: u64,
    pub elapsed: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookDone {
    pub progress: BookProgress,
    pub reason: GenerationStop,
    /// The book for play, written next to the work file.
    pub book: PathBuf,
}

/// The book for play that goes with a work file.
pub fn play_path(work: &std::path::Path) -> PathBuf {
    work.with_extension("dbook")
}

/// Starts the generation; the thread reports through `events`, last with
/// `EngineEvent::BookDone`.
pub fn spawn(
    worker: Arc<EngineWorker>,
    job: BookJob,
    events: async_channel::Sender<EngineEvent>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("book-gen".into())
        // The searches recurse on this thread, as on the search thread.
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let outcome = worker.with(|engine| generate(engine, job, &events));
            let _ = events.send_blocking(EngineEvent::BookDone(outcome));
        })
}

fn generate(
    engine: &mut deft_reversi_engine::Engine,
    job: BookJob,
    events: &async_channel::Sender<EngineEvent>,
) -> Result<BookDone, String> {
    let text = |error: deft_reversi_book::BookError| error.to_string();
    let mut slot = job.editor.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(BookEditor::for_engine(engine).map_err(text)?);
    }
    let editor = slot.as_mut().expect("made above");
    let identity = analysis_identity(engine).map_err(|e| e.to_string())?;
    if editor.identity() != identity {
        return Err("model".into());
    }
    let side = match job.root.side_to_move() {
        Color::Black => BookSide::Black,
        Color::White => BookSide::White,
    };
    let mut config = editor
        .generation_config()
        .cloned()
        .unwrap_or_else(GenerationConfig::default);
    job.settings.apply(&mut config);
    config.checkpoint_interval = Duration::from_secs(10);
    config.reuse_completed = true;
    editor
        .configure_generation_roots(
            &[GenerationRoot {
                board: job.root.board(),
                side,
            }],
            config,
        )
        .map_err(text)?;
    // A budget is required. A time limit would also reach every search and
    // change how it reads, so the run is bounded only by the stop button.
    let limits = GenerationLimits {
        time: None,
        nodes: None,
        new_positions: Some(u64::MAX),
    };
    let work = job.work.clone();
    let result = editor.generate_with_checkpoint(engine, limits, job.stop, |editor, report| {
        editor.save(&work)?;
        let _ = events.send_blocking(EngineEvent::BookProgress(BookProgress {
            positions: editor.len(),
            new_positions: report.new_positions,
            nodes: report.nodes,
            elapsed: report.elapsed,
        }));
        Ok(())
    });
    // What was found before an error is kept too.
    editor.save(&job.work).map_err(text)?;
    let report = result.map_err(text)?;
    let book = play_path(&job.work);
    editor.export().map_err(text)?.save(&book).map_err(text)?;
    Ok(BookDone {
        progress: BookProgress {
            positions: editor.len(),
            new_positions: report.new_positions,
            nodes: report.nodes,
            elapsed: report.elapsed,
        },
        reason: report.reason,
        book,
    })
}
