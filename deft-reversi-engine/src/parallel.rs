//! グローバルなタスクキューを持たない、有界メールボックス型のスレッドプールです。
//!
//! 並列探索では YBWC（Young Brothers Wait Concept：最初の有力な手だけを先に調べ終えて探索窓を狭めてから、
//! 残りの手を複数スレッドで並列探索する手法）を使います。
//!
//! もし全スレッドが共有のグローバルキューから任意の仕事を取れるようにすると、
//! 子の完了を待っている親スレッドが無関係な別の仕事を引き受けてしまい、
//! 元の子の完了を待てなくなるデッドロックやスレッド枯渇が起きる恐れがあります。
//! そのため、このプールでは子の完了を待つ親スレッド（[`Helper`]）が「自分の配下にある子孫のジョブ」だけを
//! 直接引き受けて手伝う仕組みにして、デッドロックを防いでいます。
use std::{
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
/// ジョブの投入先種別です。
#[derive(Clone, Copy)]
pub(crate) enum Dispatch {
    /// 子タスクの完了を待っている祖先スレッド（[`Helper`]）へ直接渡されました。
    Ancestor,
    /// プール内の待機中ワーカースレッドのメールボックスへ渡されました。
    Worker,
}
/// ワーカースレッドまたはヘルパー上で実行される 1 回限りのクロージャです。
pub(crate) type Job = Box<dyn FnOnce() + Send + 'static>;
/// 値を 128 バイトのキャッシュライン境界に配置するラッパーです（隣接ラインプリフェッチャ対策を含みます）。
///
/// 異なるスレッドが同じキャッシュライン上の別々の変数を書き換えると、キャッシュ同期が頻発して性能が落ちます（偽共有）。
/// また、他スレッドが毎ノード参照するデータや `Arc` の参照カウントが同じキャッシュラインに乗ると追い出されてしまうため、
/// 境界を分けてそれを防いでいます。
#[repr(align(128))]
pub(crate) struct Padded<T>(pub T);
impl<T> std::ops::Deref for Padded<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
/// ミューテックスのポイズン化（他スレッドのパニックによるロック汚染）を無視してロックを取得します。
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}
/// ワーカースレッド 1 本分のメールボックスの内部状態です。
struct MailState {
    job: Option<Job>,
    shutdown: bool,
}
/// 1 本のワーカースレッドに 1 対 1 で対応する有界メールボックス（容量 1）です。
struct Mailbox {
    state: Mutex<MailState>,
    ready: Condvar,
    busy: AtomicBool,
}
/// 固定数のワーカースレッドに対応するメールボックス群を保持するプールです。
pub(crate) struct Pool {
    mailboxes: Vec<Arc<Mailbox>>,
}
/// 探索用ワーカースレッド群のライフサイクルを管理し、Drop 時に全スレッドを終了して join します。
pub(crate) struct Workers {
    pub pool: Arc<Pool>,
    threads: Vec<JoinHandle<()>>,
}
impl Workers {
    /// 指定数 `count` のワーカースレッド（各 8 MiB スタック）を起動し、プールを生成します。
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
                        // ジョブ内のパニックなどはジョブ側で捕捉して探索セッションへ通知します。
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
    /// 空いているワーカーまたは待機中の祖先スレッドが存在するかを、ロックなしで概算判定します。
    pub(crate) fn can_offer(&self, ancestors: &[Arc<Helper>]) -> bool {
        self.mailboxes
            .iter()
            .any(|m| !m.busy.load(Ordering::Relaxed))
            || ancestors
                .iter()
                .any(|h| h.available.load(Ordering::Relaxed))
    }
    /// ジョブの実行を試みます。まず最も近い待機中の祖先（`ancestors` の末尾側）へ渡し、
    /// 受け手がいなければ空いているワーカーメールボックスへ渡します。すべて埋まっていれば `Err(job)` を返します。
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
/// 待機中の祖先ヘルパーの内部状態です。
struct HelpState {
    waiting: bool,
    job: Option<Job>,
    closed: bool,
}
// 空きワーカーを探す全スレッドが `available` をポーリングするため、128 バイト境界に揃えて
// `Arc` の参照カウントと同じキャッシュラインに乗らないようにしています。
#[repr(align(128))]
/// 子タスクの完了を待つ間、自身をヘルパースレッドとして子孫ジョブを受け入れるための同期プリミティブです。
pub(crate) struct Helper {
    available: AtomicBool,
    state: Mutex<HelpState>,
    ready: Condvar,
}
impl Helper {
    /// 初期状態（未待機・ジョブなし）の [`Helper`] を生成します。
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
    /// 待機中の祖先へジョブを引き渡します。受け入れできない場合は `Err(job)` を返します。
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
    /// 待機中のスレッドに完了または状態変化を通知して起こします。
    pub(crate) fn wake(&self) {
        // ロック下で通知します。`wait` はロックを保持したまま `complete()` を確認して待機に入るため、
        // 確認と待機開始のあいだに通知が取りこぼされる（lost wakeup）のを防ぐためです。
        let _state = lock(&self.state);
        self.ready.notify_all();
    }
    /// `complete()` が真を返すまで待機し、その間に渡された子孫ジョブがあれば実行します。
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
                // スケジューリング失敗を報告する前に、待機中のスレッドを必ず解放します。
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
