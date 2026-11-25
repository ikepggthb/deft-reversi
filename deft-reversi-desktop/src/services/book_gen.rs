//! 指定局面からの Book 自動生成・修復を専用スレッドで実行するモジュール。
//!
//! エンジンスレッドから借り受けた [`EngineWorker`] を占有して探索を行い、
//! 10 秒ごとのチェックポイントで作業ファイル（`.dbook-work`）を保存しつつ進捗を通知する。
//! 生成中は他の通常探索（AI 思考や局面解析）は行われない。
use super::engine::EngineEvent;
use deft_reversi_book::{
    AnalysisSchedule, BookEditor, BookSide, GenerationConfig, GenerationLimits, GenerationRoot,
    GenerationStop, SchedulePolicy, analysis_identity,
};
use deft_reversi_engine::{Action, Color, EngineWorker, Position, StopHandle};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::Duration,
};

/// 編集中の作業用ブックエディタ（初回生成前は `None`）。
pub type SharedEditor = Arc<Mutex<Option<BookEditor>>>;

/// Book 生成または修復ジョブのパラメータ。
pub struct BookJob {
    /// 共有中の [`BookEditor`] スロット。
    pub editor: SharedEditor,
    /// 保存先の `.dbook-work` ファイルパス（対局用 `.dbook` は同じディレクトリに書き出される）。
    pub work: PathBuf,
    /// Book の展開起点となる局面。
    pub root: Position,
    /// 初期局面から `root` へ至る手順（先にリンク登録してルートへ到達可能にする；現在局面から直接展開する場合は空）。
    pub line: Vec<(Position, Action)>,
    /// `true` の場合は新規探索を行わず、既存リンクと保存済み評価値からブックを再計算・修復する。
    pub fix: bool,
    /// 展開レベル・最大手数・許容損失などの生成設定。
    pub settings: BookSettings,
    /// 停止要求を送るためのハンドル（停止または許容損失内の展開完了まで実行する）。
    pub stop: StopHandle,
}

/// Book ビルダーの生成パラメータ（作業ファイル内に保存され、次回開いた際に復元される）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookSettings {
    /// すべての局面を探索する固定レベル。
    pub level: u8,
    /// 初期局面からの最大手数（パスは含まない）。
    pub max_plies: u8,
    /// 黒番が最善手から何石損まで展開を続けるかの許容損失。
    pub black_loss: u8,
    /// 白番が最善手から何石損まで展開を続けるかの許容損失。
    pub white_loss: u8,
    /// 1 本の変化手順に沿った累積損失の上限（`None` で無制限）。
    pub cumulative_loss: Option<u16>,
    /// 未登録の末端手についてリンク化を許可する最大損失（`None` で上記損失設定に従う）。
    pub leaf_loss: Option<u8>,
}

impl BookSettings {
    /// 作業ファイルに保存されている設定、または既定値を返す。
    pub fn of(editor: Option<&BookEditor>) -> Self {
        let config = editor
            .and_then(BookEditor::generation_config)
            .cloned()
            .unwrap_or_default();
        // CLI 等で段階的に展開されたファイルは最も深い（最後の）レベルを採用する。
        let level = match &config.analysis {
            AnalysisSchedule::Levels(levels) => levels.last().copied(),
            _ => None,
        };
        Self {
            level: level.unwrap_or(16),
            max_plies: config.max_plies,
            black_loss: config.black_loss.unwrap_or(config.expansion_loss),
            white_loss: config.white_loss.unwrap_or(config.expansion_loss),
            cumulative_loss: config.cumulative_loss,
            leaf_loss: config.leaf_loss,
        }
    }

    /// 設定を [`GenerationConfig`] へ反映する（常に有望な変化を優先するスケジュールを使用）。
    fn apply(&self, config: &mut GenerationConfig) {
        config.analysis = AnalysisSchedule::Levels(vec![self.level]);
        config.max_plies = self.max_plies;
        config.expansion_loss = self.black_loss.max(self.white_loss);
        config.black_loss = Some(self.black_loss);
        config.white_loss = Some(self.white_loss);
        config.cumulative_loss = self.cumulative_loss;
        config.leaf_loss = self.leaf_loss;
        config.schedule = SchedulePolicy::Promising;
    }
}

impl std::fmt::Debug for BookJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BookJob")
            .field("work", &self.work)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// チェックポイントまたは完了時点で保存された Book の進捗情報。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookProgress {
    /// 作業ブック内の総局面数。
    pub positions: usize,
    /// 今回の実行で新たに追加された局面数。
    pub new_positions: u64,
    /// 探索したノード数。
    pub nodes: u64,
    /// 経過時間。
    pub elapsed: Duration,
}

/// Book 生成または修復ジョブの完了結果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookDone {
    /// 最終進捗情報。
    pub progress: BookProgress,
    /// 終了理由（停止要求、または許容損失内の展開完了など）。
    pub reason: GenerationStop,
    /// 作業ファイルの隣に書き出された対局用 `.dbook` ファイルパス。
    pub book: PathBuf,
}

/// 作業用 `.dbook-work` パスに対応する対局用 `.dbook` パスを返す。
pub fn play_path(work: &std::path::Path) -> PathBuf {
    work.with_extension("dbook")
}

/// 実行中の Book 生成・修復スレッドを管理するハンドル。
pub struct BookHandle {
    handle: thread::JoinHandle<Result<BookDone, String>>,
    stop: StopHandle,
}

impl BookHandle {
    /// 実行中の Book 生成へ停止シグナルを送る。
    pub fn stop(&self) {
        self.stop.stop();
    }

    /// スレッドがすでに終了しているかを返す。
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// スレッドの終了を待機し、最終結果を返す。
    pub fn join(self) -> Result<BookDone, String> {
        self.handle
            .join()
            .unwrap_or_else(|_| Err("book generation panicked".into()))
    }
}

/// Book 生成・修復スレッド（`book-gen`）を起動する。
///
/// 途中のチェックポイント進捗は `events` チャネル経由で通知され、最終結果は
/// [`BookHandle::join`] の戻り値として返される。
pub fn spawn(
    worker: Arc<EngineWorker>,
    job: BookJob,
    events: async_channel::Sender<EngineEvent>,
) -> std::io::Result<BookHandle> {
    let stop = job.stop.clone();
    let handle = thread::Builder::new()
        .name("book-gen".into())
        // エンジン本体の探索スレッドと同様に深い再帰探索を行うため、既定より大きい 8 MiB のスタックを確保する。
        .stack_size(8 * 1024 * 1024)
        .spawn(move || worker.with(|engine| generate(engine, job, &events)))?;
    Ok(BookHandle { handle, stop })
}

/// 作業用 `.dbook-work` を保存し、同時に公開用の `.dbook` をエクスポートする。
fn save_and_export(
    editor: &BookEditor,
    work: &std::path::Path,
) -> Result<PathBuf, deft_reversi_book::BookError> {
    editor.save(work)?;
    let book = play_path(work);
    editor.export()?.save(&book)?;
    Ok(book)
}

fn generate(
    engine: &mut deft_reversi_engine::Engine,
    job: BookJob,
    events: &async_channel::Sender<EngineEvent>,
) -> Result<BookDone, String> {
    let text = |error: deft_reversi_book::BookError| error.to_string();
    let mut slot = job.editor.lock().unwrap_or_else(PoisonError::into_inner);
    if job.fix {
        let Some(editor) = slot.as_mut().filter(|e| !e.is_empty()) else {
            return Err("empty book".into());
        };
        return repair(editor, &job.work);
    }
    if slot.is_none() {
        *slot = Some(BookEditor::for_engine(engine).map_err(text)?);
    }
    let editor = slot.as_mut().expect("made above");
    let identity = analysis_identity(engine).map_err(|e| e.to_string())?;
    if editor.identity() != identity {
        return Err("model".into());
    }
    let side = match job.root.side_to_move() {
        Color::Black => BookSide::Black,
        Color::White => BookSide::White,
    };
    let mut config = editor.generation_config().cloned().unwrap_or_default();
    job.settings.apply(&mut config);
    config.checkpoint_interval = Duration::from_secs(10);
    // すでに目標レベル以上で探索済みの局面は再計算せず再利用する。
    config.reuse_completed = true;
    config.unregistered_only = false;
    let roots = [GenerationRoot {
        board: job.root.board(),
        side,
    }];
    for &(position, action) in &job.line {
        editor
            .register_move(position.board(), action)
            .map_err(text)?;
    }
    editor
        .configure_generation_roots(&roots, config)
        .map_err(text)?;
    // 時間制限を設けると局面ごとに読みの深さがばらついてブック全体の整合性が落ちるため、
    // ここでは時間無制限とし、ユーザーが停止するか許容損失内の展開が終わるまで実行する。
    let limits = GenerationLimits::default();
    let work = job.work.clone();
    let result = editor.generate_with_checkpoint(engine, limits, job.stop, |editor, report| {
        editor.save(&work)?;
        let _ = events.send_blocking(EngineEvent::BookProgress(BookProgress {
            positions: editor.len(),
            new_positions: report.new_positions,
            nodes: report.nodes,
            elapsed: report.elapsed,
        }));
        Ok(())
    });
    let report = match result {
        Ok(report) => report,
        Err(error) => {
            // エラー発生時も直前までに得られた局面は失わないよう保存を試みる。
            if let Err(save_error) = editor.save(&job.work) {
                eprintln!("[book] save after error: {save_error}");
            }
            return Err(text(error));
        }
    };
    let book = save_and_export(editor, &job.work).map_err(text)?;
    Ok(BookDone {
        progress: BookProgress {
            positions: editor.len(),
            new_positions: report.new_positions,
            nodes: report.nodes,
            elapsed: report.elapsed,
        },
        reason: report.reason,
        book,
    })
}

/// `book repair`: 探索は行わず、リンク関係と既存の評価値からブック全体を整合修復して再保存する。
fn repair(editor: &mut BookEditor, work: &std::path::Path) -> Result<BookDone, String> {
    let text = |error: deft_reversi_book::BookError| error.to_string();
    let started = std::time::Instant::now();
    editor.repair().map_err(text)?;
    let book = save_and_export(editor, work).map_err(text)?;
    Ok(BookDone {
        progress: BookProgress {
            positions: editor.len(),
            new_positions: 0,
            nodes: 0,
            elapsed: started.elapsed(),
        },
        reason: GenerationStop::Exhausted,
        book,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_book::{Book, ReadLimits};
    use deft_reversi_engine::{Engine, EngineOptions, Model, PatternModel};

    /// A book grown here opens again, in the builder and for play.
    #[test]
    fn a_grown_book_loads_again() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let dir = std::env::temp_dir().join(format!("deft-book-gen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let work = dir.join("grown.dbook-work");
        let (events, _rx) = async_channel::unbounded();
        let job = BookJob {
            editor: SharedEditor::default(),
            work: work.clone(),
            // Every move ends the game, so no search needs a calibrated model.
            root: {
                let occupied = u64::MAX << 24;
                let white = 1u64 << 27;
                Position::from_colors(occupied ^ white, white, Color::Black).unwrap()
            },
            settings: BookSettings {
                level: 12,
                max_plies: 0,
                black_loss: 0,
                white_loss: 0,
                cumulative_loss: None,
                leaf_loss: None,
            },
            line: Vec::new(),
            fix: false,
            stop: StopHandle::new(),
        };
        let editor = job.editor.clone();
        let fix_job = BookJob {
            editor: editor.clone(),
            work: work.clone(),
            root: job.root,
            line: Vec::new(),
            fix: true,
            settings: job.settings.clone(),
            stop: StopHandle::new(),
        };
        let done = generate(&mut engine, job, &events).unwrap();
        assert!(done.progress.positions > 0);
        let loaded = BookEditor::load(&work, ReadLimits::default()).unwrap();
        assert_eq!(BookSettings::of(Some(&loaded)).level, 12);
        Book::load(&done.book, ReadLimits::default()).unwrap();
        // A repair searches nothing and adds no position.
        let fixed = generate(&mut engine, fix_job, &events).unwrap();
        assert_eq!(fixed.progress.positions, done.progress.positions);
        Book::load(&fixed.book, ReadLimits::default()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repair_refuses_an_empty_book_without_writing_a_file() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let dir = std::env::temp_dir().join(format!("deft-book-gen-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let work = dir.join("empty.dbook-work");
        let (events, _rx) = async_channel::unbounded();
        let job = BookJob {
            editor: SharedEditor::default(),
            work: work.clone(),
            root: Position::initial(),
            line: Vec::new(),
            fix: true,
            settings: BookSettings::of(None),
            stop: StopHandle::new(),
        };
        assert!(generate(&mut engine, job, &events).is_err());
        assert!(!work.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
