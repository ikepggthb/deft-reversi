//! Runs searches off the caller's thread, for GUI event loops.
use crate::{
    Board, Engine, EngineError, Progress, SearchRequest, SearchResult, StopHandle, parallel::lock,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
};

/// Owns an [`Engine`] and runs one search at a time on a thread of its own.
pub struct EngineWorker {
    engine: Arc<Mutex<Engine>>,
    busy: Arc<AtomicBool>,
}
impl EngineWorker {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine: Arc::new(Mutex::new(engine)),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }
    /// Starts a search and returns at once. Fails with `EngineError::Busy`
    /// while another search is running.
    pub fn start(
        &self,
        position: impl Into<Board>,
        request: SearchRequest,
    ) -> Result<SearchHandle, EngineError> {
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(EngineError::Busy);
        }
        let board = position.into();
        let stop = request.stop_handle();
        let (sender, events) = mpsc::channel();
        let engine = self.engine.clone();
        let idle = Idle(self.busy.clone());
        let spawned = thread::Builder::new()
            .name("deft-search".into())
            // The main search recurses on this thread, like on the workers.
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let mut engine = lock(&engine);
                // Dropped before the lock: once `with` gets the engine, the
                // worker no longer reports itself busy.
                let _idle = idle;
                engine.search_with(board, &request, |progress| {
                    // The handle may have stopped listening; the search goes on.
                    let _ = sender.send(progress.clone());
                })
            });
        match spawned {
            Ok(thread) => Ok(SearchHandle {
                stop,
                events,
                thread: Some(thread),
            }),
            // The closure, and the busy flag guard in it, were dropped.
            Err(error) => Err(EngineError::Thread(error)),
        }
    }
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    /// Runs `f` on the engine, for example to clear its tables. Waits for a
    /// running search to finish first.
    pub fn with<R>(&self, f: impl FnOnce(&mut Engine) -> R) -> R {
        f(&mut lock(&self.engine))
    }
}

/// Clears the busy flag when the search thread ends, even by a panic.
struct Idle(Arc<AtomicBool>);
impl Drop for Idle {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// A running search. Dropping it without [`wait`](Self::wait) stops it.
pub struct SearchHandle {
    stop: StopHandle,
    events: Receiver<Progress>,
    thread: Option<JoinHandle<Result<SearchResult, EngineError>>>,
}
impl SearchHandle {
    /// Progress after every completed iteration; the channel closes when the
    /// search ends.
    pub fn events(&self) -> &Receiver<Progress> {
        &self.events
    }
    /// Asks the search to stop; it still returns its last completed iteration.
    pub fn stop(&self) {
        self.stop.stop();
    }
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
    /// Waits for the search to end.
    pub fn wait(mut self) -> Result<SearchResult, EngineError> {
        let thread = self.thread.take().unwrap();
        thread.join().unwrap_or_else(|_| {
            Err(EngineError::Thread(std::io::Error::other(
                "search thread panicked",
            )))
        })
    }
}
impl Drop for SearchHandle {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.stop.stop();
        }
    }
}
