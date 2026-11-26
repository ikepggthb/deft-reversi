//! 探索エンジンと Book を専用スレッド（`engine-ctl`）で管理するサービスモジュール。
//!
//! エンジンを所有し、同時に 1 つの探索（または Book 生成）のみを実行して、
//! 進捗と結果を非同期チャネル経由で UI スレッドへ通知する。
//! また、対局用 Book を保持し、探索前に Book 手の照会を行う。
use super::{
    book::SharedBook,
    book_gen::{self, BookDone, BookHandle, BookJob, BookProgress},
};
use deft_reversi_book::{AnalysisIdentity, Book, BookDecision, BookPlayer, ReadLimits};
use deft_reversi_engine::{
    Action, Engine, EngineWorker, Goal, MoveScore, Position, Progress, ResultKind, SearchHandle,
    SearchRequest, SearchResult, Selectivity,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

/// 実行中の探索に対するコマンド受信および進捗ポーリング間隔。
const POLL: Duration = Duration::from_millis(8);
/// UI スレッドへ送出する探索進捗レポートの最小間隔（約 15 fps 上限）。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(66);

/// 個々の探索リクエストを識別する一意な ID。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchId(pub u64);

/// 探索リクエストの条件仕様。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchSpec {
    /// 探索目標（深さ・レベル・終局読みなど）。
    pub goal: Goal,
    /// 1 手あたりの時間制限（任意）。
    pub time_limit: Option<Duration>,
    /// 解析パネル向けに、最善手だけでなくすべての合法手の評価値も計算するか。
    pub all_moves: bool,
    /// `Goal::Endgame` 時の選択率（枝刈りの慎重さ。レベル指定時はエンジン側が自動決定する）。
    pub selectivity: Option<Selectivity>,
    /// 有効な場合、エンジンで探索する前に Book へ問い合わせて登録手があれば即答する。
    pub book: bool,
    /// 探索対象を限定するルート手のビットマスク（解析時に 1 手ずつ順番に読むために使う）。
    pub root_moves: Option<u64>,
    /// 置換表（キャッシュ）で探索が打ち切られた場合でも、読み筋（PV）を末尾まで辿って復元するか。
    pub recover_pv: bool,
}

impl SearchSpec {
    /// 指定された [`Goal`] を持つ既定の探索仕様を生成する。
    pub fn new(goal: Goal) -> Self {
        Self {
            goal,
            time_limit: None,
            all_moves: false,
            selectivity: None,
            book: false,
            root_moves: None,
            recover_pv: false,
        }
    }
}

/// 設定画面に表示する評価モデルファイルのメタ情報。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    /// モデルファイルのパス。
    pub path: PathBuf,
    /// DPR2 フォーマットバージョン（3 以降でパターン埋め込みネットワーク対応）。
    pub format: u32,
    /// ProbCut 用のキャリブレーションデータを含むか。
    pub calibrated: bool,
    /// ファイルサイズ（バイト）。
    pub bytes: u64,
    /// モデル・キャリブレーション・探索設定から算出される解析同一性 ID。
    /// 異なる ID で作成された Book とは評価値の互換性がない。
    pub identity: Option<AnalysisIdentity>,
}

/// エンジンスレッドが保持している Book の状態。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookStatus {
    /// Book 未設定。
    None,
    /// 読み込み完了。
    Loaded {
        /// 登録局面数。
        positions: usize,
        /// 共有ブックハンドル。
        book: SharedBook,
    },
    /// 異なるモデル／設定で生成されたブックのため使用不可。
    OtherModel,
    /// 読み込みエラー。
    Failed(String),
}

/// エンジンへ投入する探索ジョブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchJob {
    /// 探索 ID。
    pub id: SearchId,
    /// 探索対象の局面。
    pub position: Position,
    /// 探索条件。
    pub spec: SearchSpec,
}

impl From<crate::domain::Strength> for SearchSpec {
    fn from(strength: crate::domain::Strength) -> Self {
        use crate::domain::{
            Strength,
            player::{LEVEL_RANGE, TIMED_LEVEL},
        };
        match strength {
            Strength::Preset(preset) => Self::new(Goal::Level(preset.level())),
            Strength::Level(level) => Self::new(Goal::Level(
                level.clamp(*LEVEL_RANGE.start(), *LEVEL_RANGE.end()),
            )),
            Strength::TimePerMove(limit) => Self {
                time_limit: Some(limit),
                ..Self::new(Goal::Level(TIMED_LEVEL))
            },
        }
    }
}

pub use crate::domain::settings::Resources;

/// `engine-ctl` スレッドへ送信する制御コマンド。
#[derive(Debug)]
pub enum EngineCommand {
    /// 実行中の探索を停止・破棄してから新しい探索を開始する。
    Start(SearchJob),
    /// 実行中の探索を停止し、結果を破棄する。
    Cancel,
    /// 実行中の探索を停止し、そこまでの最善手結果を報告させる。
    MoveNow,
    /// 置換表をクリアする。
    ClearTt,
    /// 実行中の探索を破棄した上で、新しいリソース設定でエンジンを再構築する。
    Reconfigure(Resources),
    /// 指定パスの Book を読み込む（`None` で解除）。
    SetBook(Option<PathBuf>),
    /// エンジンを占有して Book 生成・修復を実行する（完了まで他の探索は受け付けない）。
    Generate(BookJob),
    /// 実行中の処理を停止してスレッドを終了する。
    Shutdown,
}

/// 完了した反復深化（段階的に読みを深くする探索）の深さと精度情報。
///
/// `percent` は ProbCut（確率的な枝刈り）の信頼度を表す。
/// 値が低いほど強く枝刈りをして高速に読み、100 は枝刈りを一切行わない（`Endgame` なら完全読み切り）ことを意味する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// 深さ指定探索。
    Depth {
        /// 読み深さ（手数）。
        depth: u8,
        /// ProbCut 信頼度（%）。
        percent: u8,
    },
    /// 終局読み切り探索。
    Endgame {
        /// ProbCut 信頼度（%）。100 で完全解析。
        percent: u8,
    },
    /// 探索ではなく Book から即答された手。
    Book,
}

impl From<ResultKind> for Reading {
    fn from(kind: ResultKind) -> Self {
        match kind {
            ResultKind::Depth { depth, selectivity } => Self::Depth {
                depth,
                percent: selectivity.percent(),
            },
            ResultKind::Endgame { selectivity } => Self::Endgame {
                percent: selectivity.percent(),
            },
        }
    }
}

/// 探索の途中経過または最終結果（手番側から見た評価値）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// 最善手。
    pub best_move: Option<Action>,
    /// 評価値（手番側が有利なほど正の石差になる）。
    pub score: Option<i32>,
    /// 読みの深さ・精度情報。
    pub reading: Option<Reading>,
    /// 読み筋（PV: お互いに最善を尽くしたときに進むと予想される手順）。
    pub pv: Vec<Action>,
    /// 全合法手の評価結果（最善手順。`all_moves` 指定時のみ格納される）。
    pub moves: Vec<MoveScore>,
    /// 探索した局面（ノード）数。
    pub nodes: u64,
    /// 経過時間。
    pub elapsed: Duration,
}

impl From<Progress> for Report {
    fn from(p: Progress) -> Self {
        Self {
            best_move: p.best_move,
            score: Some(p.score),
            reading: Some(p.kind.into()),
            pv: p.pv,
            moves: p.moves,
            nodes: p.nodes,
            elapsed: p.elapsed,
        }
    }
}

impl From<SearchResult> for Report {
    fn from(r: SearchResult) -> Self {
        Self {
            best_move: r.best_move,
            score: r.score,
            reading: r.kind.map(Reading::from),
            pv: r.pv,
            moves: r.moves,
            nodes: r.stats.nodes,
            elapsed: r.elapsed,
        }
    }
}

/// `engine-ctl` スレッドから UI 側へ通知されるイベント。
#[derive(Debug)]
pub enum EngineEvent {
    /// エンジンの初期化または再構成が完了し、探索可能になった。
    Ready {
        /// 適用されたリソース設定。
        resources: Resources,
        /// 初期化に要した時間。
        load_time: Duration,
    },
    /// 初回の `Ready` 直後に 1 度だけ送信される評価モデル情報。
    Model(ModelInfo),
    /// 初期ロードに失敗した。
    LoadFailed(String),
    /// リソース再構成に失敗した（エンジンは従来のリソース設定のまま維持される）。
    ReconfigureFailed(String),
    /// Book の読み込み結果。
    Book(BookStatus),
    /// Book 生成のチェックポイント進捗。
    BookProgress(BookProgress),
    /// Book 生成・修復が完了し、エンジンが再び通常探索を受け付けられる状態になった。
    BookDone(Result<BookDone, String>),
    /// 探索中の定期進捗レポート。
    Progress {
        /// 探索 ID。
        id: SearchId,
        /// 途中経過。
        report: Report,
    },
    /// 探索完了。
    Finished {
        /// 探索 ID。
        id: SearchId,
        /// 最終結果。
        report: Report,
    },
    /// 探索失敗。
    Failed {
        /// 探索 ID。
        id: SearchId,
        /// エラー内容。
        error: String,
    },
}

/// `engine-ctl` スレッドが操作するエンジンバックエンドの抽象トレイト（テストではフェイク実装を差し替える）。
pub trait Backend {
    /// 実行中探索のハンドル型。
    type Running: Running;
    /// 探索を開始する。
    fn start(&mut self, position: Position, spec: SearchSpec) -> Result<Self::Running, String>;
    /// 置換表をクリアする。
    fn clear_tt(&mut self) -> Result<(), String>;
    /// リソース設定を変更してエンジンを再構成する。
    fn reconfigure(&mut self, resources: Resources) -> Result<(), String>;
    /// Book を設定または解除する。
    fn set_book(&mut self, path: Option<&Path>) -> BookStatus;
    /// モデル情報を返す（テスト用フェイクでは `None`）。
    fn model(&self) -> Option<ModelInfo> {
        None
    }
    /// Book 生成・修復スレッドを起動する。
    fn generate(
        &mut self,
        job: BookJob,
        events: async_channel::Sender<EngineEvent>,
    ) -> Result<BookHandle, String>;
}

/// 実行中の探索または Book 即答を制御するトレイト。
pub trait Running {
    /// 前回呼び出し以降の最新進捗を取得する。
    fn latest(&mut self) -> Option<Report>;
    /// 停止要求を送る。
    fn stop(&self);
    /// 探索がすでに完了しているかを返す。
    fn is_finished(&self) -> bool;
    /// 完了を待機して最終結果を返す。
    fn wait(self) -> Result<Report, String>;
}

/// 探索エンジン制御スレッド（`engine-ctl`）のサービスハンドル。
pub struct EngineService {
    commands: Option<mpsc::Sender<EngineCommand>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl EngineService {
    /// `engine-ctl` スレッドを起動し、そのスレッド上で `load` を実行する。
    ///
    /// ウィンドウの初期表示をブロックしないよう、モデル読み込みはバックグラウンドで行う。
    pub fn spawn<B, F>(load: F) -> (Self, async_channel::Receiver<EngineEvent>)
    where
        B: Backend,
        F: FnOnce() -> Result<(B, Resources), String> + Send + 'static,
    {
        let (commands, receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        let thread = match thread::Builder::new()
            .name("engine-ctl".into())
            .spawn(move || run(load, receiver, events))
        {
            Ok(thread) => Some(thread),
            Err(error) => {
                eprintln!("engine-ctl thread: {error}");
                None
            }
        };
        (
            Self {
                commands: Some(commands),
                thread,
            },
            event_receiver,
        )
    }

    /// コマンドを送信する（スレッドが既に終了している場合は理由を報告済みのため無視する）。
    pub fn send(&self, command: EngineCommand) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }

    /// 実行中の探索・Book 生成を停止し、スレッドの終了を待機する。
    pub fn shutdown(&mut self) {
        self.send(EngineCommand::Shutdown);
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for EngineService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Current<R> {
    id: SearchId,
    running: R,
    unsent: Option<Report>,
    last_sent: Option<Instant>,
}

fn run<B, F>(
    load: F,
    commands: mpsc::Receiver<EngineCommand>,
    events: async_channel::Sender<EngineEvent>,
) where
    B: Backend,
    F: FnOnce() -> Result<(B, Resources), String>,
{
    let send = |event| {
        let _ = events.send_blocking(event);
    };
    let started = Instant::now();
    let mut backend = match load() {
        Ok((backend, resources)) => {
            send(EngineEvent::Ready {
                resources,
                load_time: started.elapsed(),
            });
            if let Some(info) = backend.model() {
                send(EngineEvent::Model(info));
            }
            backend
        }
        Err(error) => {
            send(EngineEvent::LoadFailed(error));
            return;
        }
    };

    let mut current: Option<Current<B::Running>> = None;
    // Book 生成中はエンジンを占有するため、他の探索・再構成コマンドは受け付けない。
    let mut generating: Option<BookHandle> = None;
    let mut queue = VecDeque::new();
    loop {
        if generating.as_ref().is_some_and(BookHandle::is_finished)
            && let Some(handle) = generating.take()
        {
            send(EngineEvent::BookDone(handle.join()));
        }
        // 待機中はブロッキング受信で省電力化し、探索・生成中は POLL 間隔で起床して進捗を回収する。
        if current.is_none() && generating.is_none() {
            match commands.recv() {
                Ok(command) => queue.push_back(command),
                Err(_) => return,
            }
        } else {
            match commands.recv_timeout(POLL) {
                Ok(command) => queue.push_back(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    drop_search(&mut current);
                    drop_book(&mut generating);
                    return;
                }
            }
        }
        queue.extend(commands.try_iter());

        while let Some(command) = queue.pop_front() {
            match command {
                EngineCommand::Start(job) if generating.is_some() => send(EngineEvent::Failed {
                    id: job.id,
                    error: "busy".into(),
                }),
                EngineCommand::ClearTt if generating.is_some() => {}
                EngineCommand::Reconfigure(_) if generating.is_some() => {
                    send(EngineEvent::ReconfigureFailed("busy".into()))
                }
                EngineCommand::Generate(_) if generating.is_some() => {
                    send(EngineEvent::BookDone(Err("busy".into())))
                }
                EngineCommand::Generate(job) => {
                    drop_search(&mut current);
                    match backend.generate(job, events.clone()) {
                        Ok(handle) => generating = Some(handle),
                        Err(error) => send(EngineEvent::BookDone(Err(error))),
                    }
                }
                EngineCommand::Start(job) => {
                    // すでにキューの後ろに新しい Start / Cancel / 再構成などが積まれているなら、
                    // この古い探索要求を始めてもすぐ中断するだけなのでスキップする。
                    let superseded = queue.iter().any(|c| {
                        matches!(
                            c,
                            EngineCommand::Start(_)
                                | EngineCommand::Cancel
                                | EngineCommand::Generate(_)
                                | EngineCommand::Reconfigure(_)
                                | EngineCommand::Shutdown
                        )
                    });
                    if superseded {
                        continue;
                    }
                    drop_search(&mut current);
                    match backend.start(job.position, job.spec) {
                        Ok(running) => {
                            current = Some(Current {
                                id: job.id,
                                running,
                                unsent: None,
                                last_sent: None,
                            })
                        }
                        Err(error) => send(EngineEvent::Failed { id: job.id, error }),
                    }
                }
                EngineCommand::Cancel => drop_search(&mut current),
                EngineCommand::MoveNow => {
                    if let Some(c) = &current {
                        c.running.stop();
                    }
                }
                EngineCommand::ClearTt => {
                    drop_search(&mut current);
                    if let Err(error) = backend.clear_tt() {
                        eprintln!("[engine] clear tt: {error}");
                    }
                }
                EngineCommand::Reconfigure(resources) => {
                    drop_search(&mut current);
                    let started = Instant::now();
                    send(match backend.reconfigure(resources) {
                        Ok(()) => EngineEvent::Ready {
                            resources,
                            load_time: started.elapsed(),
                        },
                        Err(error) => EngineEvent::ReconfigureFailed(error),
                    });
                }
                // 実行中の探索はそのまま継続し、新しい Book は次の探索から適用する。
                EngineCommand::SetBook(path) => {
                    send(EngineEvent::Book(backend.set_book(path.as_deref())))
                }
                EngineCommand::Shutdown => {
                    drop_search(&mut current);
                    drop_book(&mut generating);
                    return;
                }
            }
        }

        let Some(c) = current.as_mut() else {
            continue;
        };
        if c.running.is_finished() {
            let c = current.take().expect("checked above");
            send(match c.running.wait() {
                Ok(report) => EngineEvent::Finished { id: c.id, report },
                Err(error) => EngineEvent::Failed { id: c.id, error },
            });
            continue;
        }
        if let Some(report) = c.running.latest() {
            c.unsent = Some(report);
        }
        let due = c
            .last_sent
            .is_none_or(|sent| sent.elapsed() >= PROGRESS_INTERVAL);
        if due && let Some(report) = c.unsent.take() {
            c.last_sent = Some(Instant::now());
            send(EngineEvent::Progress { id: c.id, report });
        }
    }
}

/// 実行中の探索を停止させ、結果を破棄してスレッド終了を待つ。
fn drop_search<R: Running>(current: &mut Option<Current<R>>) {
    if let Some(c) = current.take() {
        c.running.stop();
        let _ = c.running.wait();
    }
}

/// 実行中の Book 生成を停止させ、保存完了までスレッド終了を待つ。
fn drop_book(generating: &mut Option<BookHandle>) {
    if let Some(handle) = generating.take() {
        handle.stop();
        let _ = handle.join();
    }
}

/// 実エンジン（[`EngineWorker`]）をラップする [`Backend`] 実装。
///
/// `make` クロージャは指定されたリソース設定で [`Engine`] を構築し、設定変更時に再実行される。
pub struct EngineBackend {
    worker: Arc<EngineWorker>,
    make: Box<dyn FnMut(Resources) -> Result<Engine, String>>,
    /// 現在のモデル・キャリブレーションの識別子（別モデル製の Book との互換性判定に使用）。
    identity: Option<AnalysisIdentity>,
    book: Option<Arc<Book>>,
    model: Option<ModelInfo>,
    /// Book から着手を選ぶ [`BookPlayer`]（初回照会時にアイドル状態のエンジンから遅延生成する）。
    player: Option<BookPlayer>,
    player_failed: bool,
}

impl EngineBackend {
    /// 初期リソースでエンジンを構築し、[`EngineBackend`] を生成する。
    pub fn new(
        resources: Resources,
        mut make: impl FnMut(Resources) -> Result<Engine, String> + 'static,
    ) -> Result<Self, String> {
        let engine = make(resources)?;
        let identity = deft_reversi_book::analysis_identity(&engine).ok();
        Ok(Self {
            worker: Arc::new(EngineWorker::new(engine)),
            make: Box::new(make),
            identity,
            book: None,
            model: None,
            player: None,
            player_failed: false,
        })
    }

    /// モデルファイルのヘッダ情報等を記録する（`identity` はエンジンから取得済みの値を引き継ぐ）。
    pub fn set_model(&mut self, path: PathBuf, format: u32, calibrated: bool, bytes: u64) {
        self.model = Some(ModelInfo {
            path,
            format,
            calibrated,
            bytes,
            identity: self.identity,
        });
    }

    /// Book に現在局面の登録手があれば即座に [`Report`] として返す。
    fn book_move(&mut self, position: Position) -> Option<Report> {
        let book = self.book.clone()?;
        if self.player.is_none() {
            if self.player_failed {
                return None;
            }
            let policy = super::book::use_policy();
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64);
            match self
                .worker
                .with(|engine| BookPlayer::new(engine, Some(book), policy, seed))
            {
                Ok(player) => self.player = Some(player),
                Err(error) => {
                    eprintln!("[book] {error}");
                    self.player_failed = true;
                    return None;
                }
            }
        }
        let player = self.player.as_mut()?;
        let started = Instant::now();
        let BookDecision::Chosen(choice) = player.decide(position) else {
            return None;
        };
        Some(Report {
            best_move: Some(choice.action),
            score: choice.value.result().point_value().map(i32::from),
            reading: Some(Reading::Book),
            pv: vec![choice.action],
            moves: Vec::new(),
            nodes: 0,
            elapsed: started.elapsed(),
        })
    }
}

/// 通常のエンジン探索ハンドルか、Book による即答結果のいずれかを表す実行状態。
pub enum Answer {
    /// エンジンで非同期探索中。
    Search(SearchHandle),
    /// Book から即座に得られた回答。
    Book(Report),
}

impl Backend for EngineBackend {
    type Running = Answer;

    fn start(&mut self, position: Position, spec: SearchSpec) -> Result<Answer, String> {
        if spec.book
            && let Some(report) = self.book_move(position)
        {
            return Ok(Answer::Book(report));
        }
        let mut request = SearchRequest::new(spec.goal);
        if let Some(limit) = spec.time_limit {
            request = request.time_limit(limit);
        }
        if spec.all_moves {
            request = request.all_moves();
        }
        if let Some(selectivity) = spec.selectivity {
            request = request.selectivity(selectivity);
        }
        if let Some(mask) = spec.root_moves {
            request = request.root_moves(mask);
        }
        if spec.recover_pv {
            request = request.recover_pv();
        }
        self.worker
            .start(position, request)
            .map(Answer::Search)
            .map_err(|error| error.to_string())
    }

    fn clear_tt(&mut self) -> Result<(), String> {
        self.worker
            .with(|engine| engine.clear_tt())
            .map_err(|error| error.to_string())
    }

    fn reconfigure(&mut self, resources: Resources) -> Result<(), String> {
        // 構築に失敗しても従来のエンジンを維持できるよう、新しいエンジンを先に生成してから差し替える。
        let engine = (self.make)(resources)?;
        self.worker = Arc::new(EngineWorker::new(engine));
        // 新しいエンジンに合わせて BookPlayer も作り直す。
        self.player = None;
        self.player_failed = false;
        Ok(())
    }

    fn model(&self) -> Option<ModelInfo> {
        self.model.clone()
    }

    fn set_book(&mut self, path: Option<&Path>) -> BookStatus {
        self.book = None;
        self.player = None;
        self.player_failed = false;
        let Some(path) = path else {
            return BookStatus::None;
        };
        let book = match Book::load(path, ReadLimits::default()) {
            Ok(book) => book,
            Err(error) => return BookStatus::Failed(format!("{}: {error}", path.display())),
        };
        if self.identity != Some(book.identity()) {
            return BookStatus::OtherModel;
        }
        let positions = book.stats().positions;
        let book = Arc::new(book);
        self.book = Some(book.clone());
        BookStatus::Loaded {
            positions,
            book: SharedBook(book),
        }
    }

    fn generate(
        &mut self,
        job: BookJob,
        events: async_channel::Sender<EngineEvent>,
    ) -> Result<BookHandle, String> {
        // 対局中の Book は作業ファイルから書き出されたものを使っている可能性があるため、
        // 生成完了後に再読み込みされるようプレイヤーをリセットする。
        self.player = None;
        self.player_failed = false;
        book_gen::spawn(self.worker.clone(), job, events).map_err(|error| error.to_string())
    }
}

impl Running for Answer {
    fn latest(&mut self) -> Option<Report> {
        match self {
            Self::Search(handle) => handle.events().try_iter().last().map(Report::from),
            Self::Book(_) => None,
        }
    }

    fn stop(&self) {
        if let Self::Search(handle) = self {
            handle.stop();
        }
    }

    fn is_finished(&self) -> bool {
        match self {
            Self::Search(handle) => handle.is_finished(),
            Self::Book(_) => true,
        }
    }

    fn wait(self) -> Result<Report, String> {
        match self {
            Self::Search(handle) => handle
                .wait()
                .map(Report::from)
                .map_err(|error| error.to_string()),
            Self::Book(report) => Ok(report),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    type Log = Arc<Mutex<Vec<String>>>;

    struct FakeBackend {
        log: Log,
    }

    /// Searches finish at once at `Goal::Depth(0)`, never on their own at
    /// other depths, and fail to start at `Goal::Depth(60)`.
    struct FakeRunning {
        position: Position,
        instant: bool,
        stopped: Arc<AtomicBool>,
        log: Log,
        reported: bool,
    }

    impl Backend for FakeBackend {
        type Running = FakeRunning;

        fn start(&mut self, position: Position, spec: SearchSpec) -> Result<FakeRunning, String> {
            if spec.goal == Goal::Depth(60) {
                return Err("cannot start".into());
            }
            self.log
                .lock()
                .unwrap()
                .push(format!("start {:?}", spec.goal));
            Ok(FakeRunning {
                position,
                instant: spec.goal == Goal::Depth(0),
                stopped: Arc::new(AtomicBool::new(false)),
                log: self.log.clone(),
                reported: false,
            })
        }

        fn clear_tt(&mut self) -> Result<(), String> {
            self.log.lock().unwrap().push("clear".into());
            Ok(())
        }

        fn reconfigure(&mut self, resources: Resources) -> Result<(), String> {
            if resources.threads == 0 {
                return Err("no threads".into());
            }
            self.log.lock().unwrap().push("reconfigure".into());
            Ok(())
        }

        fn set_book(&mut self, _: Option<&Path>) -> BookStatus {
            BookStatus::None
        }

        fn generate(
            &mut self,
            _: BookJob,
            _: async_channel::Sender<EngineEvent>,
        ) -> Result<BookHandle, String> {
            Err("no book generation in tests".into())
        }
    }

    fn report(position: Position) -> Report {
        Report {
            best_move: position.legal_moves().next().map(Action::Place),
            score: Some(0),
            reading: Some(Reading::Depth {
                depth: 1,
                percent: 100,
            }),
            pv: Vec::new(),
            moves: Vec::new(),
            nodes: 1,
            elapsed: Duration::ZERO,
        }
    }

    impl Running for FakeRunning {
        fn latest(&mut self) -> Option<Report> {
            (!std::mem::replace(&mut self.reported, true)).then(|| report(self.position))
        }
        fn stop(&self) {
            self.log.lock().unwrap().push("stop".into());
            self.stopped.store(true, Ordering::Release);
        }
        fn is_finished(&self) -> bool {
            self.instant || self.stopped.load(Ordering::Acquire)
        }
        fn wait(self) -> Result<Report, String> {
            Ok(report(self.position))
        }
    }

    const RESOURCES: Resources = Resources {
        threads: 1,
        tt_mib: 1,
    };

    fn spawn() -> (EngineService, async_channel::Receiver<EngineEvent>, Log) {
        let log = Log::default();
        let backend = FakeBackend { log: log.clone() };
        let (service, events) = EngineService::spawn(move || Ok((backend, RESOURCES)));
        (service, events, log)
    }

    fn job(id: u64, depth: u8) -> EngineCommand {
        EngineCommand::Start(SearchJob {
            id: SearchId(id),
            position: Position::initial(),
            spec: SearchSpec::new(Goal::Depth(depth)),
        })
    }

    /// The next event other than progress, waiting at most a second.
    fn next(events: &async_channel::Receiver<EngineEvent>) -> EngineEvent {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match events.try_recv() {
                Ok(EngineEvent::Progress { .. }) => {}
                Ok(event) => return event,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(2)),
                Err(error) => panic!("no event: {error:?}"),
            }
        }
    }

    /// No event other than progress arrives within 100 ms.
    fn quiet(events: &async_channel::Receiver<EngineEvent>) {
        thread::sleep(Duration::from_millis(100));
        while let Ok(event) = events.try_recv() {
            assert!(
                matches!(event, EngineEvent::Progress { .. }),
                "unexpected {event:?}"
            );
        }
    }

    #[test]
    fn a_finished_search_is_reported() {
        let (service, events, _) = spawn();
        assert!(matches!(next(&events), EngineEvent::Ready { .. }));
        service.send(job(1, 0));
        assert!(matches!(
            next(&events),
            EngineEvent::Finished {
                id: SearchId(1),
                ..
            }
        ));
    }

    #[test]
    fn a_new_search_drops_the_running_one() {
        let (service, events, log) = spawn();
        next(&events);
        service.send(job(1, 5));
        thread::sleep(Duration::from_millis(30));
        service.send(job(2, 0));
        assert!(matches!(
            next(&events),
            EngineEvent::Finished {
                id: SearchId(2),
                ..
            }
        ));
        quiet(&events);
        let log = log.lock().unwrap();
        assert_eq!(*log, ["start Depth(5)", "stop", "start Depth(0)"]);
    }

    #[test]
    fn queued_requests_start_only_the_newest() {
        let log = Log::default();
        let backend = FakeBackend { log: log.clone() };
        // Commands sent while the engine loads wait in the queue.
        let (service, events) = EngineService::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            Ok((backend, RESOURCES))
        });
        service.send(job(1, 5));
        service.send(job(2, 0));
        next(&events);
        assert!(matches!(
            next(&events),
            EngineEvent::Finished {
                id: SearchId(2),
                ..
            }
        ));
        assert_eq!(*log.lock().unwrap(), ["start Depth(0)"]);
    }

    #[test]
    fn cancel_drops_the_result_and_move_now_keeps_it() {
        let (service, events, _) = spawn();
        next(&events);
        service.send(job(1, 5));
        service.send(EngineCommand::Cancel);
        quiet(&events);
        service.send(job(2, 5));
        thread::sleep(Duration::from_millis(30));
        service.send(EngineCommand::MoveNow);
        assert!(matches!(
            next(&events),
            EngineEvent::Finished {
                id: SearchId(2),
                ..
            }
        ));
    }

    #[test]
    fn failures_are_reported() {
        let (service, events, log) = spawn();
        next(&events);
        service.send(job(1, 60));
        assert!(matches!(
            next(&events),
            EngineEvent::Failed {
                id: SearchId(1),
                ..
            }
        ));
        service.send(EngineCommand::Reconfigure(Resources {
            threads: 0,
            tt_mib: 1,
        }));
        assert!(matches!(next(&events), EngineEvent::ReconfigureFailed(_)));
        service.send(EngineCommand::ClearTt);
        service.send(EngineCommand::Reconfigure(RESOURCES));
        assert!(matches!(next(&events), EngineEvent::Ready { .. }));
        assert_eq!(*log.lock().unwrap(), ["clear", "reconfigure"]);

        let (_, events) =
            EngineService::spawn(|| Err::<(FakeBackend, Resources), _>("no model".into()));
        assert!(matches!(next(&events), EngineEvent::LoadFailed(_)));
    }
}

/// The real engine on the zero model: short searches, and stops across
/// threads.
#[cfg(test)]
mod engine_tests {
    use super::*;
    use deft_reversi_engine::{
        EngineOptions, Model, PatternModel, ProbCutSigmas, legacy_main_rules,
    };

    fn spawn() -> (EngineService, async_channel::Receiver<EngineEvent>) {
        let resources = Resources {
            threads: 2,
            tt_mib: 1,
        };
        EngineService::spawn(move || {
            let backend = EngineBackend::new(resources, |resources| {
                let model = Model::from_pattern(PatternModel::zero())
                    .with_calibration(legacy_main_rules(), ProbCutSigmas::NORMAL)
                    .map_err(|error| error.to_string())?;
                let options = EngineOptions::default()
                    .threads(resources.threads)
                    .tt_mib(resources.tt_mib);
                Engine::new(model, options).map_err(|error| error.to_string())
            })?;
            Ok((backend, resources))
        })
    }

    fn start(service: &EngineService, id: u64, goal: Goal) {
        service.send(EngineCommand::Start(SearchJob {
            id: SearchId(id),
            position: Position::initial(),
            spec: SearchSpec::new(goal),
        }));
    }

    /// The next event other than progress, waiting at most ten seconds.
    fn next(events: &async_channel::Receiver<EngineEvent>) -> EngineEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match events.try_recv() {
                Ok(EngineEvent::Progress { .. }) => {}
                Ok(event) => return event,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                Err(error) => panic!("no event: {error:?}"),
            }
        }
    }

    fn assert_legal(event: EngineEvent, id: u64) {
        let EngineEvent::Finished { id: got, report } = event else {
            panic!("expected a result, got {event:?}");
        };
        assert_eq!(got, SearchId(id));
        let Some(Action::Place(square)) = report.best_move else {
            panic!("no move in {report:?}");
        };
        assert!(Position::initial().legal_moves().contains(square));
    }

    #[test]
    fn searches_stop_and_report() {
        let (service, events) = spawn();
        assert!(matches!(next(&events), EngineEvent::Ready { .. }));
        start(&service, 1, Goal::Level(5));
        assert_legal(next(&events), 1);

        // An exact solve from the start never ends by itself.
        start(&service, 2, Goal::Level(60));
        thread::sleep(Duration::from_millis(100));
        service.send(EngineCommand::MoveNow);
        assert_legal(next(&events), 2);

        start(&service, 3, Goal::Level(60));
        thread::sleep(Duration::from_millis(50));
        service.send(EngineCommand::Cancel);
        service.send(EngineCommand::ClearTt);
        start(&service, 4, Goal::Level(3));
        assert_legal(next(&events), 4);

        // Shutting down stops a running search and joins it.
        start(&service, 5, Goal::Level(60));
        thread::sleep(Duration::from_millis(50));
        drop(service);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !events.is_closed() {
            assert!(Instant::now() < deadline, "engine-ctl did not end");
            thread::sleep(Duration::from_millis(5));
        }
    }
}
