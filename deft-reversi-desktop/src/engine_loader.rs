//! Finds the model and builds engines from it on the `engine-ctl` thread.
use crate::services::engine::{EngineBackend, Resources};
use deft_reversi_engine::{Engine, EngineError, EngineOptions, Model};
use std::{
    cell::Cell,
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
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

/// The release asset of the GitHub release `model-v4`.
pub const MODEL_FILE: &str = "deft-eval-v4.dpr2";

/// `DEFT_REVERSI_MODEL`, then `resources/` next to the executable, then
/// `data/eval/` in the workspace for development builds.
pub fn find_model() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DEFT_REVERSI_MODEL").filter(|v| !v.is_empty()) {
        return Some(path.into());
    }
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("resources").join(MODEL_FILE)));
    // The parent rather than `..`, so the settings screen shows a plain path.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("data").join("eval").join(MODEL_FILE));
    bundled
        .into_iter()
        .chain(workspace)
        .find(|path| path.is_file())
}

/// The DPR2 version in the file header, and the file size.
fn model_header(path: &Path) -> std::io::Result<(u32, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    let version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    Ok((version, file.metadata()?.len()))
}

/// Reads the model again for each engine, so a resource change does not keep
/// a second copy of it in memory meanwhile.
pub fn backend(model_path: PathBuf, resources: Resources) -> Result<EngineBackend, String> {
    let header = model_header(&model_path);
    let path = model_path.clone();
    // The engine does not tell whether its model came with a calibration.
    let calibrated = Rc::new(Cell::new(false));
    let seen = calibrated.clone();
    let mut backend = EngineBackend::new(resources, move |resources| {
        let mut tt_mib = resources.tt_mib;
        loop {
            let model_start = Instant::now();
            let model = Model::from_file(&model_path)
                .map_err(|error| format!("{}: {error}", model_path.display()))?;
            seen.set(model.has_calibration());
            let model_time = model_start.elapsed();
            let engine_start = Instant::now();
            let options = EngineOptions::default()
                .threads(resources.threads)
                .tt_mib(tt_mib);
            match Engine::new(model, options) {
                Ok(engine) => {
                    eprintln!(
                        "[engine] model {} ms, engine {} ms ({} threads, TT {tt_mib} MiB) from {}",
                        model_time.as_millis(),
                        engine_start.elapsed().as_millis(),
                        resources.threads,
                        model_path.display(),
                    );
                    return Ok(engine);
                }
                // A table too large for this PC: halve it, down to 16 MiB.
                Err(EngineError::Memory(error)) if tt_mib > 16 => {
                    eprintln!(
                        "[engine] TT {tt_mib} MiB: {error}; trying {} MiB",
                        tt_mib / 2
                    );
                    tt_mib /= 2;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    })?;
    match header {
        Ok((format, bytes)) => backend.set_model(path, format, calibrated.get(), bytes),
        Err(error) => eprintln!("[engine] model header: {error}"),
    }
    Ok(backend)
}
