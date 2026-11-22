//! The `engine-ctl` thread: it owns the engine, runs one search at a time,
//! and reports to the UI over a channel the UI awaits. It also holds the
//! opening book, which answers a move search before the engine does.
use super::{
    book::SharedBook,
    book_gen::{self, BookDone, BookJob, BookProgress},
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

/// How often a running search is polled for commands and progress.
const POLL: Duration = Duration::from_millis(8);
/// At most about 15 progress reports a second reach the UI.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(66);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchSpec {
    pub goal: Goal,
    pub time_limit: Option<Duration>,
    /// Also scores every legal move, for the analysis panel.
    pub all_moves: bool,
    /// For `Goal::Endgame`; a level chooses its own.
    pub selectivity: Option<Selectivity>,
    /// A move search first asks the book, which plays its best move.
    pub book: bool,
    /// Searches only these root moves; the analysis reads one at a time.
    pub root_moves: Option<u64>,
    /// Recovers the whole line even when the table cut it short.
    pub recover_pv: bool,
}

impl SearchSpec {
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

/// The opening book the engine thread holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookStatus {
    None,
    Loaded {
        positions: usize,
        book: SharedBook,
    },
    /// Made with another model, so none of its values are used.
    OtherModel,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchJob {
    pub id: SearchId,
    pub position: Position,
    pub spec: SearchSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resources {
    pub threads: usize,
    pub tt_mib: usize,
}

#[derive(Debug)]
pub enum EngineCommand {
    /// Stops and drops a running search first.
    Start(SearchJob),
    /// Stops a running search; its result is dropped.
    Cancel,
    /// Stops a running search; its result is still reported.
    MoveNow,
    ClearTt,
    /// Builds a new engine after the running search is dropped.
    Reconfigure(Resources),
    /// Reads the book at the path, or drops the book.
    SetBook(Option<PathBuf>),
    /// Grows a book with the engine; nothing else runs until it is done.
    Generate(BookJob),
    Shutdown,
}

/// What a completed iteration read: so many moves ahead, or to the end.
/// `percent` is the ProbCut confidence; 100 means no pruning, and an
/// endgame at 100 is an exact solve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    Depth {
        depth: u8,
        percent: u8,
    },
    Endgame {
        percent: u8,
    },
    /// The move came from the opening book, not a search.
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

/// A search's state, from the side to move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub best_move: Option<Action>,
    pub score: Option<i32>,
    pub reading: Option<Reading>,
    pub pv: Vec<Action>,
    /// Every legal move, best first, when asked for and completed.
    pub moves: Vec<MoveScore>,
    pub nodes: u64,
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

#[derive(Debug)]
pub enum EngineEvent {
    Ready {
        resources: Resources,
        load_time: Duration,
    },
    LoadFailed(String),
    /// The engine keeps its previous resources.
    ReconfigureFailed(String),
    Book(BookStatus),
    BookProgress(BookProgress),
    /// The generation ended and the engine is free again.
    BookDone(Result<BookDone, String>),
    Progress {
        id: SearchId,
        report: Report,
    },
    Finished {
        id: SearchId,
        report: Report,
    },
    Failed {
        id: SearchId,
        error: String,
    },
}

/// The engine as `engine-ctl` sees it; tests use a fake.
pub trait Backend {
    type Running: Running;
    fn start(&mut self, position: Position, spec: SearchSpec) -> Result<Self::Running, String>;
    fn clear_tt(&mut self) -> Result<(), String>;
    fn reconfigure(&mut self, resources: Resources) -> Result<(), String>;
    fn set_book(&mut self, path: Option<&Path>) -> BookStatus;
    fn generate(
        &mut self,
        job: BookJob,
        events: async_channel::Sender<EngineEvent>,
    ) -> Result<thread::JoinHandle<()>, String>;
}

pub trait Running {
    /// The newest progress since the last call.
    fn latest(&mut self) -> Option<Report>;
    fn stop(&self);
    fn is_finished(&self) -> bool;
    fn wait(self) -> Result<Report, String>;
}

pub struct EngineService {
    commands: mpsc::Sender<EngineCommand>,
}

impl EngineService {
    /// `load` runs on the new thread, so the window does not wait for it.
    pub fn spawn<B, F>(load: F) -> (Self, async_channel::Receiver<EngineEvent>)
    where
        B: Backend,
        F: FnOnce() -> Result<(B, Resources), String> + Send + 'static,
    {
        let (commands, receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        let spawned = thread::Builder::new()
            .name("engine-ctl".into())
            .spawn(move || run(load, receiver, events));
        if let Err(error) = spawned {
            eprintln!("engine-ctl thread: {error}");
        }
        (Self { commands }, event_receiver)
    }

    /// A closed thread has already reported why, so a failed send is dropped.
    pub fn send(&self, command: EngineCommand) {
        let _ = self.commands.send(command);
    }
}

impl Drop for EngineService {
    fn drop(&mut self) {
        self.send(EngineCommand::Shutdown);
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
            backend
        }
        Err(error) => {
            send(EngineEvent::LoadFailed(error));
            return;
        }
    };

    let mut current: Option<Current<B::Running>> = None;
    // A book being grown holds the engine; nothing else may use it.
    let mut generating: Option<thread::JoinHandle<()>> = None;
    let mut queue = VecDeque::new();
    loop {
        if generating.as_ref().is_some_and(|g| g.is_finished()) {
            generating = None;
        }
        // Idle: block on the next command. Searching: wake up to poll.
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
                        Ok(thread) => generating = Some(thread),
                        Err(error) => send(EngineEvent::BookDone(Err(error))),
                    }
                }
                EngineCommand::Start(job) => {
                    // Only the newest request matters.
                    let superseded = queue.iter().any(|c| {
                        matches!(
                            c,
                            EngineCommand::Start(_)
                                | EngineCommand::Cancel
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
                // A running search goes on; the next one asks the new book.
                EngineCommand::SetBook(path) => {
                    send(EngineEvent::Book(backend.set_book(path.as_deref())))
                }
                EngineCommand::Shutdown => {
                    drop_search(&mut current);
                    return;
                }
            }
        }

        let Some(c) = current.as_mut() else {
            continue;
        };
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
        if c.running.is_finished() {
            let c = current.take().expect("checked above");
            send(match c.running.wait() {
                Ok(report) => EngineEvent::Finished { id: c.id, report },
                Err(error) => EngineEvent::Failed { id: c.id, error },
            });
        }
    }
}

/// Stops a search and waits for its thread, without reporting it.
fn drop_search<R: Running>(current: &mut Option<Current<R>>) {
    if let Some(c) = current.take() {
        c.running.stop();
        let _ = c.running.wait();
    }
}

/// The real engine. `make` builds an engine for given resources; it runs
/// again when the resources change.
pub struct EngineBackend {
    worker: Arc<EngineWorker>,
    make: Box<dyn FnMut(Resources) -> Result<Engine, String>>,
    /// What the model and its calibration make; a book made by another
    /// model has values that do not fit this one.
    identity: Option<AnalysisIdentity>,
    book: Option<Arc<Book>>,
    /// Made from the book when a search first asks it, as that needs the
    /// engine idle.
    player: Option<BookPlayer>,
}

impl EngineBackend {
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
            player: None,
        })
    }

    /// The book's move when it has one for this position.
    fn book_move(&mut self, position: Position) -> Option<Report> {
        let book = self.book.clone()?;
        if self.player.is_none() {
            let policy = super::book::use_policy();
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64);
            self.player = self
                .worker
                .with(|engine| BookPlayer::new(engine, Some(book), policy, seed))
                .map_err(|error| eprintln!("[book] {error}"))
                .ok();
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

/// A search, or a move the book answered at once.
pub enum Answer {
    Search(SearchHandle),
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
        // The old engine is freed first, so its table and the new one are
        // not held at the same time.
        let engine = (self.make)(resources)?;
        self.worker = Arc::new(EngineWorker::new(engine));
        // The player is made again from the new engine.
        self.player = None;
        Ok(())
    }

    fn set_book(&mut self, path: Option<&Path>) -> BookStatus {
        self.book = None;
        self.player = None;
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
    ) -> Result<thread::JoinHandle<()>, String> {
        // The book in play may come from the work file; it is read again
        // when the generation has written it.
        self.player = None;
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
        ) -> Result<thread::JoinHandle<()>, String> {
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
