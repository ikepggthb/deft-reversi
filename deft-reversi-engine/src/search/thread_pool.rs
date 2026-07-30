use crate::search::search::SearchStats;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;

pub type Job = Box<dyn FnOnce() -> TaskResult + Send + 'static>;

/// 完了結果を呼び出し側へ返さない仕事。
///
/// 結果は分割点の共有オブジェクトへ書き込むため、`mpsc::channel` を確保せずに済む。
pub type DetachedJob = Box<dyn FnOnce() + Send + 'static>;

/// 分割点で master が slave の完了を待つための受け口。
///
/// `completions` は単調増加のカウンタで、master は自分が観測済みの値と
/// 比較して待機を解除する。更新はすべて mutex 下で行い、更新後に notify する
/// ため通知の取りこぼしは起きない。
pub struct HelperSlot {
    /// `state.waiting` のロック無し複製。`try_offer` の早期棄却に使う。
    waiting: AtomicBool,
    state: Mutex<HelperState>,
    cv: Condvar,
}

#[derive(Default)]
struct HelperState {
    /// この分割点で完了した slave の数。
    completions: u64,
    /// cutoff や外部 stop で待機を解除するためのフラグ。
    closed: bool,
    /// master がこの受け口で待機中か。
    waiting: bool,
    /// 子孫から直接渡された仕事。同時に保持するのは 1 件だけ。
    job: Option<DetachedJob>,
}

impl Default for HelperSlot {
    fn default() -> Self {
        Self {
            waiting: AtomicBool::new(false),
            state: Mutex::new(HelperState::default()),
            cv: Condvar::new(),
        }
    }
}

impl HelperSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// slave が 1 件終わったことを master に伝える。
    pub fn notify_completion(&self) {
        {
            let mut state = self.state.lock().unwrap();
            state.completions = state.completions.wrapping_add(1);
        }
        self.cv.notify_all();
    }


    /// `spawned` 件すべてが完了したか。ブロックしない。
    pub fn is_complete(&self, spawned: u64) -> bool {
        let state = self.state.lock().unwrap();
        state.completions >= spawned || state.closed
    }

    /// 待機中の master にこの仕事を直接渡す。渡せなければ仕事をそのまま返す。
    ///
    /// 呼び出し側は自分の祖先チェーンを近い順に辿ってこれを試す。
    pub fn try_offer(&self, job: DetachedJob) -> Result<(), DetachedJob> {
        if !self.waiting.load(Ordering::Relaxed) {
            return Err(job);
        }
        let mut state = self.state.lock().unwrap();
        if !state.waiting || state.job.is_some() || state.closed {
            return Err(job);
        }
        state.job = Some(job);
        // 同じ master へ二重に渡さないよう、ここで待機を解除しておく。
        state.waiting = false;
        self.waiting.store(false, Ordering::Relaxed);
        drop(state);
        self.cv.notify_all();
        Ok(())
    }

    /// 渡された仕事があれば取り出す。ブロックしない。
    pub fn take_offered_job(&self) -> Option<DetachedJob> {
        if self.state.lock().unwrap().job.is_none() {
            return None;
        }
        self.state.lock().unwrap().job.take()
    }

    /// 完了か、仕事を渡されるまで待つ。`timeout` で必ず戻る。
    ///
    /// 渡された仕事があればそれを返す。
    pub fn wait_or_take_job(
        &self,
        spawned: u64,
        timeout: std::time::Duration,
    ) -> Option<DetachedJob> {
        let mut state = self.state.lock().unwrap();
        if let Some(job) = state.job.take() {
            return Some(job);
        }
        if state.completions >= spawned || state.closed {
            return None;
        }
        state.waiting = true;
        self.waiting.store(true, Ordering::Relaxed);
        let (mut state, _) = self.cv.wait_timeout(state, timeout).unwrap();
        state.waiting = false;
        self.waiting.store(false, Ordering::Relaxed);
        state.job.take()
    }

}

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

    /// 結果を返さない仕事を投入する。`mpsc::channel` を確保しない分だけ軽い。
    ///
    /// 完了は分割点の [`HelperSlot`] で数える。
    pub fn try_push_detached(&self, job: DetachedJob) -> Result<(), DetachedJob> {
        // ロックを取る前に、キューが埋まっている場合は棄却する。
        if self.shared.queue_len.load(Ordering::Relaxed) >= self.shared.queue_cap {
            return Err(job);
        }
        let mut state = self.shared.state.lock().unwrap();
        if !state.running || state.queue.len() >= self.shared.queue_cap {
            return Err(job);
        }

        let wrapped: Job = Box::new(move || {
            job();
            TaskResult {
                score: 0,
                move_index: 0,
                stats: SearchStats::default(),
                aborted: false,
            }
        });
        state.queue.push_back(wrapped);
        self.shared
            .queue_len
            .store(state.queue.len(), Ordering::Relaxed);
        self.shared.ready.notify_one();
        Ok(())
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
