use crate::search::search::SearchStats;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;

pub type Job = Box<dyn FnOnce() -> TaskResult + Send + 'static>;

#[derive(Debug)]
pub struct TaskResult {
    pub score: i32,
    pub move_index: usize,
    pub stats: SearchStats,
    pub aborted: bool,
}

pub struct TaskHandle {
    receiver: mpsc::Receiver<TaskResult>,
}

impl TaskHandle {
    pub fn join(self) -> TaskResult {
        self.receiver
            .recv()
            .expect("YBWC worker terminated before sending result")
    }
}

struct State {
    running: bool,
    queue: VecDeque<Job>,
    idle_workers: usize,
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    idle_workers: AtomicUsize,
    /// `state.queue.len()` のロック無し複製。try_push の早期棄却に使う。
    queue_len: AtomicUsize,
    /// キューに積める仕事数の上限。ワーカーが後から空いたときに
    /// すぐ取れる「作り置き」を許しつつ、投機的タスクの溢れを防ぐ。
    queue_cap: usize,
}

pub struct ThreadPool {
    shared: Arc<Shared>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl ThreadPool {
    pub fn new(n_workers: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                running: true,
                queue: VecDeque::new(),
                idle_workers: 0,
            }),
            ready: Condvar::new(),
            idle_workers: AtomicUsize::new(0),
            queue_len: AtomicUsize::new(0),
            queue_cap: n_workers.max(2),
        });
        let mut workers = Vec::with_capacity(n_workers);
        for _ in 0..n_workers {
            let shared = shared.clone();
            workers.push(thread::spawn(move || worker_loop(shared)));
        }
        Self { shared, workers }
    }

    pub fn try_push(&self, job: Job) -> Result<TaskHandle, Job> {
        // ロックを取る前に、キューが埋まっている場合は棄却する。
        if self.shared.queue_len.load(Ordering::Relaxed) >= self.shared.queue_cap {
            return Err(job);
        }
        let mut state = self.shared.state.lock().unwrap();
        if !state.running || state.queue.len() >= self.shared.queue_cap {
            return Err(job);
        }

        let (sender, receiver) = mpsc::channel();
        let wrapped: Job = Box::new(move || {
            let result = job();
            let _ = sender.send(result);
            TaskResult {
                score: 0,
                move_index: 0,
                stats: SearchStats::default(),
                aborted: true,
            }
        });
        state.queue.push_back(wrapped);
        self.shared.queue_len.store(state.queue.len(), Ordering::Relaxed);
        self.shared.ready.notify_one();
        Ok(TaskHandle { receiver })
    }

    /// キューから仕事を 1 件取り出す(join 待ちの親スレッドが「手伝う」ために使う)。
    pub fn try_pop_job(&self) -> Option<Job> {
        if self.shared.queue_len.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let mut state = self.shared.state.lock().unwrap();
        let job = state.queue.pop_front();
        self.shared.queue_len.store(state.queue.len(), Ordering::Relaxed);
        job
    }

    #[inline(always)]
    pub fn idle_worker_count(&self) -> usize {
        self.shared.idle_workers.load(Ordering::Acquire)
    }

    /// 子タスクの完了を待ちながら、待ち時間でキューの仕事を実行する。
    ///
    /// 親が join でブロックして遊ぶのを防ぎ、分割チェーンの各層の親を
    /// ワーカーとして働かせる。
    pub fn join_helping(&self, handle: TaskHandle) -> TaskResult {
        loop {
            match handle.receiver.try_recv() {
                Ok(result) => return result,
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    panic!("YBWC worker terminated before sending result")
                }
            }
            if let Some(job) = self.try_pop_job() {
                let _ = job();
                continue;
            }
            match handle
                .receiver
                .recv_timeout(std::time::Duration::from_micros(50))
            {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("YBWC worker terminated before sending result")
                }
            }
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.running = false;
        }
        self.shared.ready.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(shared: Arc<Shared>) {
    loop {
        let job = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if !state.running && state.queue.is_empty() {
                    return;
                }
                if let Some(job) = state.queue.pop_front() {
                    shared.queue_len.store(state.queue.len(), Ordering::Relaxed);
                    break job;
                }
                state.idle_workers += 1;
                shared.idle_workers.fetch_add(1, Ordering::Release);
                state = shared.ready.wait(state).unwrap();
                state.idle_workers -= 1;
                shared.idle_workers.fetch_sub(1, Ordering::Release);
            }
        };
        let _ = job();
    }
}
