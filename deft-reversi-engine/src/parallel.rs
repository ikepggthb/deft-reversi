//! Bounded mailboxes, no global task queue. Waiting ancestors accept descendant work.
use std::{
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
#[derive(Clone, Copy)]
pub(crate) enum Dispatch {
    Ancestor,
    Worker,
}
pub(crate) type Job = Box<dyn FnOnce() + Send + 'static>;
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}
struct MailState {
    job: Option<Job>,
    shutdown: bool,
}
struct Mailbox {
    state: Mutex<MailState>,
    ready: Condvar,
    busy: AtomicBool,
}
pub(crate) struct Pool {
    mailboxes: Vec<Arc<Mailbox>>,
}
pub(crate) struct Workers {
    pub pool: Arc<Pool>,
    threads: Vec<JoinHandle<()>>,
}
impl Workers {
    pub(crate) fn new(count: usize) -> std::io::Result<Self> {
        let mut workers = Self {
            pool: Arc::new(Pool {
                mailboxes: Vec::new(),
            }),
            threads: Vec::new(),
        };
        for index in 0..count {
            let mailbox = Arc::new(Mailbox {
                state: Mutex::new(MailState {
                    job: None,
                    shutdown: false,
                }),
                ready: Condvar::new(),
                busy: AtomicBool::new(false),
            });
            let worker = mailbox.clone();
            let handle = thread::Builder::new()
                .name(format!("deft-search-{index}"))
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    loop {
                        let mut state = lock(&worker.state);
                        while state.job.is_none() && !state.shutdown {
                            state = worker.ready.wait(state).unwrap_or_else(|p| p.into_inner());
                        }
                        if state.shutdown && state.job.is_none() {
                            break;
                        }
                        let job = state.job.take().unwrap();
                        drop(state);
                        // Search jobs catch failures and signal the owning session.
                        job();
                        worker.busy.store(false, Ordering::Release);
                    }
                })?;
            Arc::get_mut(&mut workers.pool)
                .unwrap()
                .mailboxes
                .push(mailbox);
            workers.threads.push(handle);
        }
        Ok(workers)
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        for mailbox in &self.pool.mailboxes {
            lock(&mailbox.state).shutdown = true;
            mailbox.ready.notify_all();
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
impl Pool {
    pub(crate) fn can_offer(&self, ancestors: &[Arc<Helper>]) -> bool {
        self.mailboxes
            .iter()
            .any(|m| !m.busy.load(Ordering::Relaxed))
            || ancestors
                .iter()
                .any(|h| h.available.load(Ordering::Relaxed))
    }
    pub(crate) fn offer(&self, mut job: Job, ancestors: &[Arc<Helper>]) -> Result<Dispatch, Job> {
        for ancestor in ancestors.iter().rev() {
            match ancestor.offer(job) {
                Ok(()) => return Ok(Dispatch::Ancestor),
                Err(back) => job = back,
            }
        }
        for mailbox in &self.mailboxes {
            if mailbox
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                let mut state = lock(&mailbox.state);
                if state.shutdown {
                    mailbox.busy.store(false, Ordering::Release);
                    continue;
                }
                state.job = Some(job);
                mailbox.ready.notify_one();
                return Ok(Dispatch::Worker);
            }
        }
        Err(job)
    }
}
struct HelpState {
    waiting: bool,
    job: Option<Job>,
    closed: bool,
}
pub(crate) struct Helper {
    available: AtomicBool,
    state: Mutex<HelpState>,
    ready: Condvar,
}
impl Helper {
    pub(crate) fn new() -> Self {
        Self {
            available: AtomicBool::new(false),
            state: Mutex::new(HelpState {
                waiting: false,
                job: None,
                closed: false,
            }),
            ready: Condvar::new(),
        }
    }
    fn offer(&self, job: Job) -> Result<(), Job> {
        if !self.available.load(Ordering::Acquire) {
            return Err(job);
        }
        let mut state = lock(&self.state);
        if !state.closed && state.waiting && state.job.is_none() {
            state.job = Some(job);
            state.waiting = false;
            self.available.store(false, Ordering::Release);
            self.ready.notify_one();
            Ok(())
        } else {
            Err(job)
        }
    }
    pub(crate) fn wake(&self) {
        self.ready.notify_all();
    }
    pub(crate) fn wait(&self, complete: impl Fn() -> bool) {
        loop {
            let mut state = lock(&self.state);
            if let Some(job) = state.job.take() {
                state.waiting = false;
                self.available.store(false, Ordering::Release);
                drop(state);
                job();
                continue;
            }
            if complete() {
                state.waiting = false;
                self.available.store(false, Ordering::Release);
                state.closed = true;
                return;
            }
            state.waiting = true;
            self.available.store(true, Ordering::Release);
            let (mut state, _) = self
                .ready
                .wait_timeout(state, Duration::from_millis(5))
                .unwrap_or_else(|p| p.into_inner());
            state.waiting = false;
            self.available.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waiting_ancestor_executes_descendant_job() {
        let helper = Arc::new(Helper::new());
        let done = Arc::new(AtomicBool::new(false));
        let waiter = helper.clone();
        let finished = done.clone();
        let thread = thread::spawn(move || waiter.wait(|| finished.load(Ordering::Acquire)));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut job: Job = Box::new(move || {
            done.store(true, Ordering::Release);
        });
        loop {
            match helper.offer(job) {
                Ok(()) => break,
                Err(back) => job = back,
            }
            if std::time::Instant::now() > deadline {
                // Always release the waiter before reporting a scheduling failure.
                let mut state = lock(&helper.state);
                state.job = Some(job);
                helper.ready.notify_all();
                drop(state);
                thread.join().unwrap();
                panic!("ancestor never became available");
            }
            thread::yield_now();
        }
        thread.join().unwrap();
    }
}
