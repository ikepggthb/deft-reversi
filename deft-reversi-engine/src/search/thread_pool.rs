use crate::search::search::SearchStats;
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
    queue: Vec<Job>,
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
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
                queue: Vec::new(),
            }),
            ready: Condvar::new(),
            queue_cap: (n_workers / 2).max(2),
        });
        let mut workers = Vec::with_capacity(n_workers);
        for _ in 0..n_workers {
            let shared = shared.clone();
            workers.push(thread::spawn(move || worker_loop(shared)));
        }
        Self { shared, workers }
    }

    pub fn try_push(&self, job: Job) -> Result<TaskHandle, Job> {
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
        state.queue.push(wrapped);
        self.shared.ready.notify_one();
        Ok(TaskHandle { receiver })
    }

    /// キューから仕事を 1 件取り出す(join 待ちの親スレッドが「手伝う」ために使う)。
    pub fn try_pop_job(&self) -> Option<Job> {
        let mut state = self.shared.state.lock().unwrap();
        state.queue.pop()
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
                .recv_timeout(std::time::Duration::from_micros(200))
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
                if let Some(job) = state.queue.pop() {
                    break job;
                }
                state = shared.ready.wait(state).unwrap();
            }
        };
        let _ = job();
    }
}
