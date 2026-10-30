//! Ctrl-C stops the running search; with none running it exits.
use deft_reversi_engine::StopHandle;
use std::sync::Mutex;

static CURRENT: Mutex<Option<StopHandle>> = Mutex::new(None);

pub fn install() {
    let installed = ctrlc::set_handler(|| match &*CURRENT.lock().unwrap() {
        Some(stop) => stop.stop(),
        None => std::process::exit(130),
    });
    if let Err(e) = installed {
        eprintln!("warning: Ctrl-C will not stop searches: {e}");
    }
}

/// Makes Ctrl-C stop `stop` until the returned guard is dropped.
pub fn arm(stop: StopHandle) -> Armed {
    *CURRENT.lock().unwrap() = Some(stop);
    Armed
}

pub struct Armed;

impl Drop for Armed {
    fn drop(&mut self) {
        *CURRENT.lock().unwrap() = None;
    }
}
