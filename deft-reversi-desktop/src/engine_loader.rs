//! Loads the model and builds the engine off the UI thread.
use deft_reversi_engine::{Engine, EngineOptions, EngineWorker, Model};
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub struct Loaded {
    pub worker: EngineWorker,
    pub model_path: PathBuf,
    pub model_time: Duration,
    pub engine_time: Duration,
    pub threads: usize,
    pub tt_mib: usize,
}

/// Leaves one logical CPU for the UI, and caps the default at 8 threads.
pub fn default_threads() -> usize {
    let available = thread::available_parallelism().map_or(1, |n| n.get());
    available.saturating_sub(1).clamp(1, 8)
}

pub const DEFAULT_TT_MIB: usize = 128;

/// The release asset of the GitHub release `model-v2-pattern-lr0.3`.
pub const MODEL_FILE: &str = "deft-eval-v2-pattern-lr0.3.dpr2";

/// `DEFT_REVERSI_MODEL`, then `resources/` next to the executable, then
/// `data/eval/` in the workspace for development builds.
pub fn find_model() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DEFT_REVERSI_MODEL") {
        return Some(path.into());
    }
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("resources").join(MODEL_FILE)));
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../data/eval")
        .join(MODEL_FILE);
    bundled
        .into_iter()
        .chain([workspace])
        .find(|path| path.is_file())
}

/// Starts loading at once; the result arrives on the returned channel.
pub fn spawn(model_path: PathBuf) -> async_channel::Receiver<Result<Loaded, String>> {
    let (sender, receiver) = async_channel::bounded(1);
    let spawned = thread::Builder::new()
        .name("engine-load".into())
        .spawn(move || {
            let _ = sender.send_blocking(load(model_path));
        });
    if let Err(error) = spawned {
        eprintln!("engine-load thread: {error}");
    }
    receiver
}

fn load(model_path: PathBuf) -> Result<Loaded, String> {
    let start = Instant::now();
    let model = Model::from_file(&model_path)
        .map_err(|error| format!("{}: {error}", model_path.display()))?;
    let model_time = start.elapsed();

    let threads = default_threads();
    let start = Instant::now();
    let options = EngineOptions::default()
        .threads(threads)
        .tt_mib(DEFAULT_TT_MIB);
    let engine = Engine::new(model, options).map_err(|error| error.to_string())?;
    let engine_time = start.elapsed();

    Ok(Loaded {
        worker: EngineWorker::new(engine),
        model_path,
        model_time,
        engine_time,
        threads,
        tt_mib: DEFAULT_TT_MIB,
    })
}
