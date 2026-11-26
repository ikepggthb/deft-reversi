//! 棋譜の自動保存・設定保存・エクスポート・読み込み・対局一覧取得を担う `storage` スレッドサービス。
//!
//! ディスクへの書き込みを専用の単一スレッドにまとめることで、UI の描画を止めず、
//! かつファイル操作が同時に走って競合するのも防いでいる。
//! また、対局中の自動保存には「デバウンス（連続した変更を一定時間待ってから 1 回にまとめる仕組み）」を使い、
//! 連打で Undo や着手を繰り返したときの無駄なディスク書き込みを抑えている。
use crate::{
    domain::{GameInfo, Player},
    persistence::{
        atomic_file,
        paths::Paths,
        record::{self, LoadedGame, RecordV1},
        settings_file::SettingsV1,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// 連続した棋譜変更を 1 回の保存にまとめるための待機時間（デバウンス時間）。
/// 最後の変更からこの時間だけ追加の変更がなければディスクへ書き出す。
const QUIET: Duration = Duration::from_millis(200);
/// 変更が途切れず続いた場合でも、最初の未保存変更から遅くともこの時間以内には必ず書き出す上限。
const MAX_DELAY: Duration = Duration::from_secs(1);

/// `storage` スレッドへ送信するコマンド。
#[derive(Debug)]
pub enum StorageCommand {
    /// 棋譜を自動保存する（デバウンス対象。同一対局 ID の新しい記録で上書きされる）。
    SaveGame(RecordV1),
    /// 設定ファイルを保存する。
    SaveSettings(SettingsV1),
    /// 指定パスへ棋譜をエクスポートする。
    Export {
        /// 出力先ファイルパス。
        path: PathBuf,
        /// エクスポートする棋譜データ。
        record: RecordV1,
    },
    /// 既存の自動保存ファイルを元の対局 ID のまま開く。
    Load(PathBuf),
    /// 外部ファイルをインポートし、新しい対局 ID を採番したコピーとして開く。
    Import(PathBuf),
    /// 保存済み対局の一覧を要求する。
    ListGames,
    /// 指定 ID の対局ファイル（およびバックアップ）を削除する。
    DeleteGame(String),
}

/// 対局一覧に表示するための 1 局分のサマリ情報。
#[derive(Clone, Debug)]
pub struct GameSummary {
    /// 対局 ID。
    pub id: String,
    /// 最終更新日時（RFC 3339）。
    pub updated_at: String,
    /// 黒番・白番のプレイヤー設定。
    pub players: [Player; 2],
    /// 初期局面から打たれた手数（石数の増分）。
    pub moves: usize,
    /// 終局済みの場合の `(黒石数, 白石数)`。
    pub outcome: Option<(u32, u32)>,
}

/// `storage` スレッドから UI 側へ通知されるイベント。
#[derive(Debug)]
pub enum StorageEvent {
    /// スレッド起動時に読み込まれた最新の保存済み対局。
    Restored(Option<LoadedGame>),
    /// 自動保存が完了した。
    Saved,
    /// 自動保存に失敗した。
    SaveFailed {
        /// 失敗した対局 ID。
        id: String,
        /// エラー内容。
        error: String,
    },
    /// 棋譜の読み込み／インポート結果。
    Loaded(Result<LoadedGame, String>),
    /// エクスポート結果。
    Exported(Result<PathBuf, String>),
    /// 保存済み対局一覧の取得結果。
    Games(Vec<GameSummary>),
}

/// 永続化バックグラウンドスレッドへの操作ハンドル。
pub struct StorageService {
    commands: Option<mpsc::Sender<StorageCommand>>,
    thread: Option<JoinHandle<()>>,
}

/// デバウンス待ちの未保存棋譜データ。
struct Pending {
    record: RecordV1,
    first: Instant,
    last: Instant,
}

impl StorageService {
    /// `storage` スレッドを起動し、コマンド送信ハンドルとイベント受信チャネルを返す。
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

    /// `storage` スレッドへコマンドを送信する。
    pub fn send(&self, command: StorageCommand) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }

    /// 保留中の自動保存をディスクへ書き出してからスレッドの終了を待機する。
    pub fn shutdown(&mut self) {
        // チャネルを閉じることで、スレッド側が保留分をフラッシュした後に終了する。
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
                let now = Instant::now();
                if now >= due {
                    write_game(&paths, pending.take(), &send);
                    continue;
                }
                match commands.recv_timeout(due.saturating_duration_since(now)) {
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
                // 別対局の保存に切り替えられた場合は、古い方の保留データを破棄せず先に書き出す。
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
            StorageCommand::Load(path) => {
                write_game(&paths, pending.take(), &send);
                send(StorageEvent::Loaded(load_record(&path)));
            }
            StorageCommand::Import(path) => {
                let game = load_record(&path).map(|mut game| {
                    game.game.id = GameInfo::new().id;
                    game
                });
                send(StorageEvent::Loaded(game));
            }
            StorageCommand::ListGames => {
                // ディスク上の最新状態（未フラッシュの保留分を含む）を一覧へ反映する。
                write_game(&paths, pending.take(), &send);
                send(StorageEvent::Games(list_games(&paths)));
            }
            StorageCommand::DeleteGame(id) => {
                if pending.as_ref().is_some_and(|p| p.record.game_id == id) {
                    pending = None;
                } else {
                    // 別対局の保留分を先にフラッシュし、返却する一覧の整合性を保つ。
                    write_game(&paths, pending.take(), &send);
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

fn load_record(path: &Path) -> Result<LoadedGame, String> {
    let text = atomic_file::read(path, record::MAX_BYTES).map_err(|error| error.to_string())?;
    RecordV1::parse(&text).map_err(|error| error.to_string())
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

/// 保存先フォルダ内の `.dreversi` ファイルパス一覧を収集する。
fn game_paths(paths: &Paths) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(&paths.games) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "dreversi"))
        .collect()
}

/// 保存先フォルダ内の `.dreversi` ファイルとその更新日時を収集する。
fn game_files(paths: &Paths) -> Vec<(PathBuf, std::time::SystemTime)> {
    game_paths(paths)
        .into_iter()
        .filter_map(|p| {
            let modified = fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((p, modified))
        })
        .collect()
}

/// 最も新しく更新された対局を読み込む（本体破損時は `.bak` からの復旧も試みる）。
fn newest_game(paths: &Paths) -> Option<LoadedGame> {
    let mut files = game_files(paths);
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    files.into_iter().find_map(|(path, _)| {
        atomic_file::read_with_backup(&path, record::MAX_BYTES, |text| {
            match RecordV1::parse(text) {
                Ok(loaded) => atomic_file::ParseOutcome::Ok(loaded),
                Err(error @ record::LoadError::NewerVersion(_)) => {
                    atomic_file::ParseOutcome::Unsupported(error)
                }
                Err(error) => atomic_file::ParseOutcome::Corrupt(error),
            }
        })
        .inspect_err(|e| eprintln!("[storage] {}: {e}", path.display()))
        .ok()
    })
}

fn list_games(paths: &Paths) -> Vec<GameSummary> {
    let mut games: Vec<GameSummary> = game_paths(paths)
        .into_iter()
        .filter_map(|path| {
            let text = atomic_file::read(&path, record::MAX_BYTES).ok()?;
            let parsed = RecordV1::parse_with_meta(&text).ok()?;
            let loaded = parsed.loaded;
            let last = loaded.history.position(loaded.history.actions().len());
            let initial = loaded.history.position(0);
            let discs = |p: deft_reversi_engine::Position| {
                p.disc_count(deft_reversi_engine::Color::Black)
                    + p.disc_count(deft_reversi_engine::Color::White)
            };
            Some(GameSummary {
                id: loaded.game.id,
                updated_at: parsed.updated_at,
                players: loaded.players,
                moves: (discs(last) - discs(initial)) as usize,
                outcome: parsed.outcome,
            })
        })
        .collect();
    games.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    games
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
    fn importing_an_old_export_preserves_the_autosave_and_its_backup() {
        let dir = temp_dir("storage-import");
        let paths = Paths::under(dir.clone());
        let (mut storage, events) = StorageService::spawn(paths.clone());
        assert!(matches!(next(&events), StorageEvent::Restored(None)));
        let game = GameInfo::new();
        let old = record(&game, &["f5"]);
        let export = dir.join("old.dreversi");
        storage.send(StorageCommand::Export {
            path: export.clone(),
            record: old,
        });
        assert!(matches!(next(&events), StorageEvent::Exported(Ok(_))));

        let saved = paths.game(&game.id);
        for moves in [&["f5", "d6"][..], &["f5", "d6", "c3"]] {
            storage.send(StorageCommand::SaveGame(record(&game, moves)));
            storage.send(StorageCommand::Load(saved.clone()));
            assert!(matches!(next(&events), StorageEvent::Saved));
            let StorageEvent::Loaded(Ok(loaded)) = next(&events) else {
                panic!("expected the saved game");
            };
            assert_eq!(loaded.game.id, game.id);
            assert_eq!(loaded.history.actions().len(), moves.len());
        }

        let backup = atomic_file::backup_path(&saved);
        let expected = fs::read(&saved).unwrap();
        let expected_backup = fs::read(&backup).unwrap();
        let mut copy_ids = Vec::new();
        for _ in 0..2 {
            storage.send(StorageCommand::Import(export.clone()));
            let StorageEvent::Loaded(Ok(copy)) = next(&events) else {
                panic!("expected an imported game");
            };
            assert_ne!(copy.game.id, game.id);
            assert!(!copy_ids.contains(&copy.game.id));
            assert_eq!(copy.game.created_at, game.created_at);
            assert_eq!(copy.history.actions().len(), 1);
            storage.send(StorageCommand::SaveGame(record(&copy.game, &["f5"])));
            copy_ids.push(copy.game.id);
        }
        storage.shutdown();
        assert_eq!(fs::read(&saved).unwrap(), expected);
        assert_eq!(fs::read(&backup).unwrap(), expected_backup);
        assert!(copy_ids.iter().all(|id| paths.game(id).is_file()));
        fs::remove_dir_all(dir).unwrap();
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
        storage.send(StorageCommand::Import(broken));
        assert!(matches!(next(&events), StorageEvent::Loaded(Err(_))));
        let _ = fs::remove_dir_all(dir);
    }
}
