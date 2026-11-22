//! Growing the opening book from a position (§24, B3). It runs on a thread
//! of its own with the engine thread's engine, which searches nothing else
//! meanwhile, and saves the work file as it goes.
use super::engine::EngineEvent;
use deft_reversi_book::{
    AnalysisSchedule, BookEditor, BookSide, GenerationConfig, GenerationLimits, GenerationRoot,
    GenerationStop, SchedulePolicy, analysis_identity,
};
use deft_reversi_engine::{Action, Color, EngineWorker, Position, StopHandle};
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
    /// The moves from the start to `root`, linked first so the book reaches
    /// it; empty when it grows from the position as it is.
    pub line: Vec<(Position, Action)>,
    /// Repairs the book instead of growing it: the values are worked out
    /// again from the links and what was read. Nothing is searched.
    pub fix: bool,
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
    /// The most a move not yet linked may lose and still become a link;
    /// None leaves it to the losses above.
    pub leaf_loss: Option<u8>,
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
            leaf_loss: config.leaf_loss,
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
        config.leaf_loss = self.leaf_loss;
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
    if job.fix {
        return repair(editor, &job.work);
    }
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
    // What already meets the level is kept.
    config.reuse_completed = true;
    config.unregistered_only = false;
    let roots = [GenerationRoot {
        board: job.root.board(),
        side,
    }];
    for &(position, action) in &job.line {
        editor
            .register_move(position.board(), action)
            .map_err(text)?;
    }
    editor
        .configure_generation_roots(&roots, config)
        .map_err(text)?;
    // No time limit: it would reach every search and change how it reads.
    // The run goes on until stopped or nothing is left within the losses.
    let limits = GenerationLimits::default();
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

/// `book repair`: the values from the links, without a search; then the
/// work file and the book for play are written again.
fn repair(editor: &mut BookEditor, work: &std::path::Path) -> Result<BookDone, String> {
    let text = |error: deft_reversi_book::BookError| error.to_string();
    let started = std::time::Instant::now();
    editor.repair().map_err(text)?;
    editor.save(work).map_err(text)?;
    let book = play_path(work);
    editor.export().map_err(text)?.save(&book).map_err(text)?;
    Ok(BookDone {
        progress: BookProgress {
            positions: editor.len(),
            new_positions: 0,
            nodes: 0,
            elapsed: started.elapsed(),
        },
        reason: GenerationStop::Exhausted,
        book,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_book::{Book, ReadLimits};
    use deft_reversi_engine::{Engine, EngineOptions, Model, PatternModel};

    /// A book grown here opens again, in the builder and for play.
    #[test]
    fn a_grown_book_loads_again() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let dir = std::env::temp_dir().join(format!("deft-book-gen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let work = dir.join("grown.dbook-work");
        let (events, _rx) = async_channel::unbounded();
        let job = BookJob {
            editor: SharedEditor::default(),
            work: work.clone(),
            // Every move ends the game, so no search needs a calibrated model.
            root: {
                let occupied = u64::MAX << 24;
                let white = 1u64 << 27;
                Position::from_colors(occupied ^ white, white, Color::Black).unwrap()
            },
            settings: BookSettings {
                level: 12,
                max_plies: 0,
                black_loss: 0,
                white_loss: 0,
                cumulative_loss: None,
                leaf_loss: None,
            },
            line: Vec::new(),
            fix: false,
            stop: StopHandle::new(),
        };
        let editor = job.editor.clone();
        let fix_job = BookJob {
            editor: editor.clone(),
            work: work.clone(),
            root: job.root,
            line: Vec::new(),
            fix: true,
            settings: job.settings.clone(),
            stop: StopHandle::new(),
        };
        let done = generate(&mut engine, job, &events).unwrap();
        assert!(done.progress.positions > 0);
        let loaded = BookEditor::load(&work, ReadLimits::default()).unwrap();
        assert_eq!(BookSettings::of(Some(&loaded)).level, 12);
        Book::load(&done.book, ReadLimits::default()).unwrap();
        // A repair searches nothing and adds no position.
        let fixed = generate(&mut engine, fix_job, &events).unwrap();
        assert_eq!(fixed.progress.positions, done.progress.positions);
        Book::load(&fixed.book, ReadLimits::default()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
