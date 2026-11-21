//! The `storage` thread: autosave with a debounce, settings, export, load,
//! and the list of saved games. Writes run one at a time, in order.
use crate::{
    domain::Player,
    persistence::{
        atomic_file,
        paths::Paths,
        record::{self, LoadedGame, RecordV1},
        settings_file::SettingsV1,
    },
};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{self, RecvTimeoutError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// A save waits this long for the next change…
const QUIET: Duration = Duration::from_millis(200);
/// …but no longer than this after the first unsaved change.
const MAX_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum StorageCommand {
    /// Debounced; a newer record of the same game replaces it.
    SaveGame(RecordV1),
    SaveSettings(SettingsV1),
    Export {
        path: PathBuf,
        record: RecordV1,
    },
    Load(PathBuf),
    ListGames,
    DeleteGame(String),
}

#[derive(Clone, Debug)]
pub struct GameSummary {
    pub id: String,
    pub updated_at: String,
    pub players: [Player; 2],
    /// Discs placed, so the number of moves played.
    pub moves: usize,
    /// Black's and White's discs at the end, when finished.
    pub outcome: Option<(u32, u32)>,
}

#[derive(Debug)]
pub enum StorageEvent {
    /// The newest saved game, read when the thread starts.
    Restored(Option<LoadedGame>),
    Saved,
    SaveFailed {
        id: String,
        error: String,
    },
    Loaded(Result<LoadedGame, String>),
    Exported(Result<PathBuf, String>),
    Games(Vec<GameSummary>),
}

pub struct StorageService {
    commands: Option<mpsc::Sender<StorageCommand>>,
    thread: Option<JoinHandle<()>>,
}

struct Pending {
    record: RecordV1,
    first: Instant,
    last: Instant,
}

impl StorageService {
    pub fn spawn(paths: Paths) -> (Self, async_channel::Receiver<StorageEvent>) {
        let (commands, receiver) = mpsc::channel();
        let (events, event_receiver) = async_channel::unbounded();
        let thread = thread::Builder::new()
            .name("storage".into())
            .spawn(move || run(paths, receiver, events));
        let thread = match thread {
            Ok(thread) => Some(thread),
            Err(error) => {
                eprintln!("storage thread: {error}");
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

    pub fn send(&self, command: StorageCommand) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }

    /// Writes what is pending and waits for the thread to end.
    pub fn shutdown(&mut self) {
        // Closing the channel ends the thread after it flushes.
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for StorageService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(
    paths: Paths,
    commands: mpsc::Receiver<StorageCommand>,
    events: async_channel::Sender<StorageEvent>,
) {
    let send = |event| {
        let _ = events.send_blocking(event);
    };
    send(StorageEvent::Restored(newest_game(&paths)));

    let mut pending: Option<Pending> = None;
    loop {
        let command = match &pending {
            None => match commands.recv() {
                Ok(command) => command,
                Err(_) => return,
            },
            Some(p) => {
                let due = (p.last + QUIET).min(p.first + MAX_DELAY);
                match commands.recv_timeout(due.saturating_duration_since(Instant::now())) {
                    Ok(command) => command,
                    Err(RecvTimeoutError::Timeout) => {
                        write_game(&paths, pending.take(), &send);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        write_game(&paths, pending.take(), &send);
                        return;
                    }
                }
            }
        };
        match command {
            StorageCommand::SaveGame(record) => {
                let now = Instant::now();
                // Another game's save is written first, not dropped.
                if pending
                    .as_ref()
                    .is_some_and(|p| p.record.game_id != record.game_id)
                {
                    write_game(&paths, pending.take(), &send);
                }
                let first = pending.as_ref().map_or(now, |p| p.first);
                pending = Some(Pending {
                    record,
                    first,
                    last: now,
                });
            }
            StorageCommand::SaveSettings(settings) => {
                if let Err(error) =
                    atomic_file::write(&paths.settings, settings.to_json().as_bytes())
                {
                    eprintln!("[storage] settings: {error}");
                }
            }
            StorageCommand::Export { path, record } => send(StorageEvent::Exported(
                atomic_file::write(&path, record.to_json().as_bytes())
                    .map(|()| path)
                    .map_err(|e| e.to_string()),
            )),
            StorageCommand::Load(path) => send(StorageEvent::Loaded(
                atomic_file::read(&path, record::MAX_BYTES)
                    .map_err(|e| e.to_string())
                    .and_then(|text| RecordV1::parse(&text).map_err(|e| e.to_string())),
            )),
            StorageCommand::ListGames => {
                // The list shows what is on disk, including the latest save.
                write_game(&paths, pending.take(), &send);
                send(StorageEvent::Games(list_games(&paths)));
            }
            StorageCommand::DeleteGame(id) => {
                if pending.as_ref().is_some_and(|p| p.record.game_id == id) {
                    pending = None;
                }
                let path = paths.game(&id);
                let _ = fs::remove_file(atomic_file::backup_path(&path));
                if let Err(error) = fs::remove_file(&path) {
                    eprintln!("[storage] delete {}: {error}", path.display());
                }
                send(StorageEvent::Games(list_games(&paths)));
            }
        }
    }
}

fn write_game(paths: &Paths, pending: Option<Pending>, send: &impl Fn(StorageEvent)) {
    let Some(p) = pending else {
        return;
    };
    let id = p.record.game_id.clone();
    send(
        match atomic_file::write(&paths.game(&id), p.record.to_json().as_bytes()) {
            Ok(()) => StorageEvent::Saved,
            Err(error) => StorageEvent::SaveFailed {
                id,
                error: error.to_string(),
            },
        },
    );
}

/// Record files in the games folder; broken ones are left out.
fn game_files(paths: &Paths) -> Vec<(PathBuf, std::time::SystemTime)> {
    let Ok(entries) = fs::read_dir(&paths.games) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "dreversi"))
        .filter_map(|p| {
            let modified = fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((p, modified))
        })
        .collect()
}

/// The most recently written game, from its backup if the file is broken.
fn newest_game(paths: &Paths) -> Option<LoadedGame> {
    let mut files = game_files(paths);
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    files.into_iter().find_map(|(path, _)| {
        atomic_file::read_with_backup(&path, record::MAX_BYTES, RecordV1::parse)
            .inspect_err(|e| eprintln!("[storage] {}: {e}", path.display()))
            .ok()
    })
}

fn list_games(paths: &Paths) -> Vec<GameSummary> {
    let mut games: Vec<(String, GameSummary)> = game_files(paths)
        .into_iter()
        .filter_map(|(path, _)| {
            let text = atomic_file::read(&path, record::MAX_BYTES).ok()?;
            let loaded = RecordV1::parse(&text).ok()?;
            let file: RecordV1 = serde_json::from_str(&text).ok()?;
            let last = loaded.history.position(loaded.history.actions().len());
            let initial = loaded.history.position(0);
            let discs = |p: deft_reversi_engine::Position| {
                p.disc_count(deft_reversi_engine::Color::Black)
                    + p.disc_count(deft_reversi_engine::Color::White)
            };
            Some((
                file.updated_at.clone(),
                GameSummary {
                    id: loaded.game.id,
                    updated_at: file.updated_at,
                    players: loaded.players,
                    moves: (discs(last) - discs(initial)) as usize,
                    outcome: file.outcome.map(|o| (o.black, o.white)),
                },
            ))
        })
        .collect();
    games.sort_by(|a, b| b.0.cmp(&a.0));
    games.into_iter().map(|(_, g)| g).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{GameInfo, Grouping, History},
        persistence::{atomic_file::tests::temp_dir, record::EngineInfoV1},
    };
    use deft_reversi_engine::{Action, Position, Square};

    fn record(game: &GameInfo, moves: &[&str]) -> RecordV1 {
        let mut history = History::new(Position::initial());
        for m in moves {
            let square: Square = m.parse().unwrap();
            history
                .play(Action::Place(square), Grouping::NewGroup)
                .unwrap();
        }
        RecordV1::new(
            game,
            [Player::Human; 2],
            &history,
            EngineInfoV1 {
                version: "test".into(),
                model: "test".into(),
            },
        )
    }

    fn next(events: &async_channel::Receiver<StorageEvent>) -> StorageEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match events.try_recv() {
                Ok(event) => return event,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                Err(error) => panic!("no event: {error:?}"),
            }
        }
    }

    #[test]
    fn saves_are_debounced_listed_restored_and_deleted() {
        let paths = Paths::under(temp_dir("storage"));
        let (storage, events) = StorageService::spawn(paths.clone());
        assert!(matches!(next(&events), StorageEvent::Restored(None)));

        let game = GameInfo::new();
        let started = Instant::now();
        for moves in [&["f5"][..], &["f5", "d6"], &["f5", "d6", "c3"]] {
            storage.send(StorageCommand::SaveGame(record(&game, moves)));
        }
        // Only the newest of the burst is written, after the quiet period.
        assert!(matches!(next(&events), StorageEvent::Saved));
        assert!(started.elapsed() >= QUIET);
        let saved = fs::read_to_string(paths.game(&game.id)).unwrap();
        assert_eq!(RecordV1::parse(&saved).unwrap().history.actions().len(), 3);

        // A second game: the list is newest first.
        let other = GameInfo::new();
        storage.send(StorageCommand::SaveGame(record(&other, &[])));
        storage.send(StorageCommand::ListGames);
        assert!(matches!(next(&events), StorageEvent::Saved));
        let StorageEvent::Games(games) = next(&events) else {
            panic!("expected the list");
        };
        assert_eq!(games.len(), 2);
        assert_eq!(games.iter().find(|g| g.id == game.id).unwrap().moves, 3);

        // A restart restores the newest game.
        drop(storage);
        let (storage, events) = StorageService::spawn(paths.clone());
        let StorageEvent::Restored(Some(restored)) = next(&events) else {
            panic!("expected a restored game");
        };
        assert_eq!(restored.game.id, other.id);

        storage.send(StorageCommand::DeleteGame(other.id.clone()));
        let StorageEvent::Games(games) = next(&events) else {
            panic!("expected the list");
        };
        assert_eq!(games.len(), 1);
        assert!(!paths.game(&other.id).exists());
    }

    #[test]
    fn shutdown_writes_the_pending_save_and_failures_are_reported() {
        let dir = temp_dir("storage-shutdown");
        let paths = Paths::under(dir.clone());
        let (mut storage, events) = StorageService::spawn(paths.clone());
        next(&events);
        let game = GameInfo::new();
        storage.send(StorageCommand::SaveGame(record(&game, &["f5"])));
        storage.shutdown();
        assert!(paths.game(&game.id).exists());

        // The games folder is a file: saves fail and say so.
        let blocked = Paths::under(dir.join("blocked"));
        fs::create_dir_all(dir.join("blocked")).unwrap();
        fs::write(&blocked.games, "").unwrap();
        let (storage, events) = StorageService::spawn(blocked);
        next(&events);
        storage.send(StorageCommand::SaveGame(record(&game, &[])));
        storage.send(StorageCommand::ListGames);
        assert!(matches!(next(&events), StorageEvent::SaveFailed { .. }));
        assert!(matches!(next(&events), StorageEvent::Games(_)));

        // Loading a broken file reports the error.
        let broken = dir.join("broken.dreversi");
        fs::write(&broken, "{").unwrap();
        storage.send(StorageCommand::Load(broken));
        assert!(matches!(next(&events), StorageEvent::Loaded(Err(_))));
        let _ = fs::remove_dir_all(dir);
    }
}
