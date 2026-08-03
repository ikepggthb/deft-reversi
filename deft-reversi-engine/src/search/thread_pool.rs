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
///
/// 早期解除の仕組みは持たない。master は spawn した slave が全員 `slave_finished`
/// を呼ぶまで必ず待つ。abort や beta cut がかかった場合も、slave は次の
/// `next_work` か 1 手の探索の終わりで停止するので待ち時間は短い。
/// 途中で待機を打ち切ると slave が書き込む統計とスコアを取りこぼす。
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

    /// master が待機中か。ロック無しの概算。
    #[inline(always)]
    pub fn is_waiting(&self) -> bool {
        self.waiting.load(Ordering::Relaxed)
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
        self.state.lock().unwrap().completions >= spawned
    }

    /// 待機中の master にこの仕事を直接渡す。渡せなければ仕事をそのまま返す。
    ///
    /// 呼び出し側は自分の祖先チェーンを近い順に辿ってこれを試す。
    pub fn try_offer(&self, job: DetachedJob) -> Result<(), DetachedJob> {
        if !self.waiting.load(Ordering::Relaxed) {
            return Err(job);
        }
        let mut state = self.state.lock().unwrap();
        if !state.waiting || state.job.is_some() {
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
        self.state.lock().unwrap().job.take()
    }

    /// 完了か、仕事を渡されるまで待つ。`timeout` で必ず戻る。
    ///
    /// 渡された仕事があればそれを返す。`state.job` を完了判定より先に取るので、
    /// 渡された仕事が実行されないまま捨てられることはない。
    pub fn wait_or_take_job(
        &self,
        spawned: u64,
        timeout: std::time::Duration,
    ) -> Option<DetachedJob> {
        let mut state = self.state.lock().unwrap();
        if let Some(job) = state.job.take() {
            return Some(job);
        }
        if state.completions >= spawned {
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
}

pub struct ThreadPool {
    shared: Arc<Shared>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl ThreadPool {
    /// `n_workers` 個のワーカーを起動する。
    ///
    /// 投入された仕事は、その時点で待機中のワーカー数までしか
    /// キューに予約しない。後で空くワーカー向けの作り置きはしない。
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
        });
        let mut workers = Vec::with_capacity(n_workers);
        for _ in 0..n_workers {
            let shared = shared.clone();
            workers.push(thread::spawn(move || worker_loop(shared)));
        }
        Self { shared, workers }
    }

    pub fn try_push(&self, job: Job) -> Result<TaskHandle, Job> {
        // ロックを取る前に、現在待機中のワーカーに渡せない
        // 場合は棄却する。atomic は概算なので、最終判定は lock 下で行う。
        if self.shared.queue_len.load(Ordering::Relaxed)
            >= self.shared.idle_workers.load(Ordering::Relaxed)
        {
            return Err(job);
        }
        let mut state = self.shared.state.lock().unwrap();
        if !state.running || state.queue.len() >= state.idle_workers {
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
        self.shared
            .queue_len
            .store(state.queue.len(), Ordering::Relaxed);
        self.shared.ready.notify_one();
        Ok(TaskHandle { receiver })
    }

    /// 結果を返さない仕事を投入する。`mpsc::channel` を確保しない分だけ軽い。
    ///
    /// 完了は分割点の [`HelperSlot`] で数える。
    pub fn try_push_detached(&self, job: DetachedJob) -> Result<(), DetachedJob> {
        // ロックを取る前に、現在待機中のワーカーに渡せない
        // 場合は棄却する。atomic は概算なので、最終判定は lock 下で行う。
        if self.shared.queue_len.load(Ordering::Relaxed)
            >= self.shared.idle_workers.load(Ordering::Relaxed)
        {
            return Err(job);
        }
        let mut state = self.shared.state.lock().unwrap();
        if !state.running || state.queue.len() >= state.idle_workers {
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

    /// 現在待機中のワーカーに渡せるか。ロック無しの概算。
    #[inline(always)]
    pub fn has_queue_room(&self) -> bool {
        self.shared.queue_len.load(Ordering::Relaxed)
            < self.shared.idle_workers.load(Ordering::Relaxed)
    }

    /// キューから仕事を 1 件取り出す(join 待ちの親スレッドが「手伝う」ために使う)。
    pub fn try_pop_job(&self) -> Option<Job> {
        if self.shared.queue_len.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let mut state = self.shared.state.lock().unwrap();
        let job = state.queue.pop_front();
        self.shared
            .queue_len
            .store(state.queue.len(), Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::time::Duration;

    const SHORT: Duration = Duration::from_millis(5);
    const LONG: Duration = Duration::from_secs(5);
    /// テストが待ってよい上限。超えたら panic して「失敗」にする。
    ///
    /// `cargo test` にはテスト単位のタイムアウトが無いため、無期限のループを
    /// 書くと退行時にテストが落ちずに CI がハングする。待つ側は必ずこれを使う。
    const DEADLINE: Duration = Duration::from_secs(20);

    fn counting_job(counter: Arc<AtomicUsize>) -> DetachedJob {
        Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })
    }

    /// `cond` が真になるまで待つ。`DEADLINE` を超えたら panic する。
    fn wait_until(what: &str, cond: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + DEADLINE;
        while !cond() {
            assert!(
                std::time::Instant::now() < deadline,
                "{what} が {DEADLINE:?} 以内に成立しなかった"
            );
            thread::yield_now();
        }
    }

    /// 誰も待機していないスロットへの `try_offer` は、仕事をそのまま返す。
    #[test]
    fn offer_to_idle_slot_is_rejected_and_returns_the_job() {
        let slot = HelperSlot::new();
        let counter = Arc::new(AtomicUsize::new(0));

        assert!(!slot.is_waiting());
        let returned = slot.try_offer(counting_job(counter.clone()));
        assert!(returned.is_err(), "待機していないので受け取ってはいけない");

        // 返ってきた仕事は失われておらず、呼び出し側が自分で実行できる。
        returned.unwrap_err()();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(slot.take_offered_job().is_none());
    }

    /// 待機中に渡した仕事は必ず `wait_or_take_job` から返る (取りこぼさない)。
    #[test]
    fn job_offered_while_waiting_is_always_returned_to_the_master() {
        let slot = Arc::new(HelperSlot::new());
        let counter = Arc::new(AtomicUsize::new(0));

        let offerer = {
            let slot = slot.clone();
            let counter = counter.clone();
            thread::spawn(move || {
                // master が待機に入るまで粘る。
                wait_until("try_offer の成立", || {
                    slot.is_waiting() && slot.try_offer(counting_job(counter.clone())).is_ok()
                });
            })
        };

        // spawned=1 だが誰も完了しないので、仕事を受け取るまで待ち続ける。
        let deadline = std::time::Instant::now() + DEADLINE;
        let job = loop {
            if let Some(job) = slot.wait_or_take_job(1, SHORT) {
                break job;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "渡されたはずの仕事を master が受け取れなかった"
            );
        };
        offerer.join().unwrap();

        job();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        // 受け取ったので待機は解除されている。
        assert!(!slot.is_waiting());
    }

    /// 同時に保持できる仕事は 1 件だけ。2 件目は取り出されるまで拒否される。
    #[test]
    fn second_offer_is_rejected_until_the_first_is_taken() {
        let slot = Arc::new(HelperSlot::new());
        let counter = Arc::new(AtomicUsize::new(0));

        // master 役を待機させ、その隙に 2 件申し込む。
        let waiter = {
            let slot = slot.clone();
            thread::spawn(move || slot.wait_or_take_job(1, LONG))
        };
        wait_until("master の待機開始", || slot.is_waiting());

        assert!(slot.try_offer(counting_job(counter.clone())).is_ok());
        // 1 件目を受け取った時点で waiting が下りるので 2 件目は入らない。
        assert!(slot.try_offer(counting_job(counter.clone())).is_err());

        let job = waiter.join().unwrap().expect("1 件目が返るはず");
        job();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    /// `spawned` 件の完了通知で待機が解け、以降はブロックしない。
    #[test]
    fn completions_release_the_wait() {
        let slot = Arc::new(HelperSlot::new());
        assert!(!slot.is_complete(2));

        let waiter = {
            let slot = slot.clone();
            thread::spawn(move || {
                let deadline = std::time::Instant::now() + DEADLINE;
                while !slot.is_complete(2) {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "完了通知を出しても待機が解けなかった"
                    );
                    // 完了済みなら None が返り、ループが回って is_complete で抜ける。
                    assert!(slot.wait_or_take_job(2, LONG).is_none());
                }
            })
        };

        slot.notify_completion();
        slot.notify_completion();
        waiter.join().unwrap();

        assert!(slot.is_complete(2));
        // 完了後は待機に入らずすぐ返る。
        assert!(slot.wait_or_take_job(2, LONG).is_none());
    }

    /// 完了済みでも、先に渡された仕事は捨てずに返す。
    #[test]
    fn pending_job_wins_over_completion() {
        let slot = Arc::new(HelperSlot::new());
        let counter = Arc::new(AtomicUsize::new(0));

        let waiter = {
            let slot = slot.clone();
            thread::spawn(move || slot.wait_or_take_job(1, LONG))
        };
        wait_until("master の待機開始", || slot.is_waiting());
        // 仕事を渡した直後に完了通知が来る、という順序を再現する。
        assert!(slot.try_offer(counting_job(counter.clone())).is_ok());
        slot.notify_completion();

        let job = waiter
            .join()
            .unwrap()
            .expect("完了通知より先に渡した仕事は返らなければならない");
        job();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    /// `try_push_detached` で積んだ仕事はワーカーが実行し、`queue_len` が戻る。
    #[test]
    fn detached_jobs_run_on_workers() {
        let pool = ThreadPool::new(2);
        let counter = Arc::new(AtomicUsize::new(0));

        for _ in 0..4 {
            let counter = counter.clone();
            // キューが埋まっていれば自分で実行する、という呼び出し側の作法に倣う。
            if let Err(job) = pool.try_push_detached(counting_job(counter)) {
                job();
            }
        }

        wait_until("detached job が全件実行される", || {
            counter.load(Ordering::SeqCst) >= 4
        });
        assert_eq!(counter.load(Ordering::SeqCst), 4);
        wait_until("ワーカーが待機に戻る", || pool.has_queue_room());
        assert!(pool.has_queue_room());
        assert!(pool.try_pop_job().is_none());
    }

    /// 全ワーカーが実行中なら新しい仕事は作り置きせず、
    /// `try_push` / `try_push_detached` のどちらも元の仕事を返す。
    #[test]
    fn jobs_are_rejected_while_all_workers_are_busy() {
        struct ReleaseWorkersOnDrop(Arc<AtomicBool>);

        impl Drop for ReleaseWorkersOnDrop {
            fn drop(&mut self) {
                // 途中の assert が失敗しても pool の Drop をハングさせない。
                self.0.store(true, Ordering::Release);
            }
        }

        let pool = ThreadPool::new(2);
        wait_until("全ワーカーの待機開始", || {
            pool.idle_worker_count() == 2
        });

        let started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(AtomicBool::new(false));
        let _release_guard = ReleaseWorkersOnDrop(release.clone());
        for _ in 0..2 {
            let started = started.clone();
            let release = release.clone();
            let job: DetachedJob = Box::new(move || {
                started.fetch_add(1, Ordering::SeqCst);
                while !release.load(Ordering::Acquire) {
                    thread::yield_now();
                }
            });
            assert!(pool.try_push_detached(job).is_ok());
        }
        wait_until("全ワーカーのジョブ開始", || {
            started.load(Ordering::SeqCst) == 2
        });
        assert_eq!(pool.idle_worker_count(), 0);
        assert!(!pool.has_queue_room());

        let returned_job_ran = Arc::new(AtomicUsize::new(0));
        let counter = returned_job_ran.clone();
        let job: Job = Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            TaskResult {
                score: 0,
                move_index: 0,
                stats: SearchStats::default(),
                aborted: false,
            }
        });
        let returned = match pool.try_push(job) {
            Err(job) => job,
            Ok(_) => panic!("アイドルワーカーがいないので拒否するはず"),
        };
        assert_eq!(returned_job_ran.load(Ordering::SeqCst), 0);
        returned();

        let counter = returned_job_ran.clone();
        let job: DetachedJob = Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let returned = pool
            .try_push_detached(job)
            .expect_err("アイドルワーカーがいないので拒否するはず");
        assert_eq!(returned_job_ran.load(Ordering::SeqCst), 1);
        returned();
        assert_eq!(returned_job_ran.load(Ordering::SeqCst), 2);

        release.store(true, Ordering::Release);
    }
}
