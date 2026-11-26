//! GUI イベントループなど呼び出し元スレッドを止めずに探索を実行するためのワーカースレッドラッパーです。
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

/// [`Engine`] を所有し、専用スレッド上で一度に 1 つの探索を非同期実行するワーカーです。
pub struct EngineWorker {
    engine: Arc<Mutex<Engine>>,
    busy: Arc<AtomicBool>,
}
impl EngineWorker {
    /// 指定した [`Engine`] を管理する [`EngineWorker`] を生成します。
    pub fn new(engine: Engine) -> Self {
        Self {
            engine: Arc::new(Mutex::new(engine)),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }
    /// バックグラウンドスレッドで探索を開始し、制御用の [`SearchHandle`] を即座に返します。
    /// 別の探索が実行中の場合は [`EngineError::Busy`] を返します。
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
            // ワーカープール内のスレッドと同様に、メイン探索スレッドも再帰が深くなっても溢れないよう 8 MiB のスタックを確保します。
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let mut engine = lock(&engine);
                // ロックより先にドロップされるよう束縛順を調整しています。
                // これにより `with()` がエンジンロックを取得した時点では `is_busy()` が必ず false になります。
                let _idle = idle;
                engine.search_with(board, &request, |progress| {
                    // 受信側ハンドルがすでに閉じられていても探索自体は継続します。
                    let _ = sender.send(progress.clone());
                })
            });
        match spawned {
            Ok(thread) => Ok(SearchHandle {
                stop,
                events,
                thread: Some(thread),
            }),
            // スレッド起動に失敗した場合はクロージャごと `idle` ガードがドロップされ、`busy` が false に戻ります。
            Err(error) => Err(EngineError::Thread(error)),
        }
    }
    /// 現在探索が実行中かを返します。
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    /// 実行中の探索が終わるのを待ってから、エンジンに対して任意の処理 `f`（置換表のクリアなど）を実行します。
    pub fn with<R>(&self, f: impl FnOnce(&mut Engine) -> R) -> R {
        f(&mut lock(&self.engine))
    }
}

/// 探索スレッド終了時（パニック時を含みます）に確実に `busy` フラグを解除する RAII ガードです。
struct Idle(Arc<AtomicBool>);
impl Drop for Idle {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 実行中の探索を制御・監視するためのハンドルです。
/// [`wait`](Self::wait) を呼ばずにドロップすると探索に停止要求を送ります。
pub struct SearchHandle {
    stop: StopHandle,
    events: Receiver<Progress>,
    thread: Option<JoinHandle<Result<SearchResult, EngineError>>>,
}
impl SearchHandle {
    /// 反復深化の各深さが完了するたびに送信される進捗イベントの受信チャネルを返します。
    /// 探索が終了するとチャネルは閉じられます。
    pub fn events(&self) -> &Receiver<Progress> {
        &self.events
    }
    /// 探索へ停止要求を送ります。最後に完了した反復までの結果は引き続き取得できます。
    pub fn stop(&self) {
        self.stop.stop();
    }
    /// 探索スレッドがすでに終了しているかを返します。
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
    /// 探索スレッドの終了を待ち、最終的な [`SearchResult`] を返します。
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
