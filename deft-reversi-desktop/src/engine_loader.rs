//! Finds the model and builds engines from it on the `engine-ctl` thread.
use crate::services::engine::{EngineBackend, Resources};
use deft_reversi_engine::{Engine, EngineOptions, Model};
use std::{
    path::{Path, PathBuf},
    thread,
    time::Instant,
};

/// Leaves one logical CPU for the UI, and caps the default at 8 threads.
pub fn default_threads() -> usize {
    let available = thread::available_parallelism().map_or(1, |n| n.get());
    available.saturating_sub(1).clamp(1, 8)
}

pub const DEFAULT_TT_MIB: usize = 128;

pub fn default_resources() -> Resources {
    Resources {
        threads: default_threads(),
        tt_mib: DEFAULT_TT_MIB,
    }
}

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

/// Reads the model again for each engine, so a resource change does not keep
/// a second copy of it in memory meanwhile.
pub fn backend(model_path: PathBuf, resources: Resources) -> Result<EngineBackend, String> {
    EngineBackend::new(resources, move |resources| {
        let start = Instant::now();
        let model = Model::from_file(&model_path)
            .map_err(|error| format!("{}: {error}", model_path.display()))?;
        let model_time = start.elapsed();
        let start = Instant::now();
        let options = EngineOptions::default()
            .threads(resources.threads)
            .tt_mib(resources.tt_mib);
        let engine = Engine::new(model, options).map_err(|error| error.to_string())?;
        eprintln!(
            "[engine] model {} ms, engine {} ms ({} threads, TT {} MiB) from {}",
            model_time.as_millis(),
            start.elapsed().as_millis(),
            resources.threads,
            resources.tt_mib,
            model_path.display(),
        );
        Ok(engine)
    })
}
