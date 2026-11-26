//! 評価モデルファイルの探索と、探索エンジンバックエンド（[`EngineBackend`]）の構築を担うモジュール。
//!
//! リバーシ AI の強さや解析精度は評価モデル（`.dpr2` ファイル）に依存する。
//! このモジュールでは、実行環境からモデルファイルを探し出し、`engine-ctl` スレッド上で
//! エンジンを初期化するための設定とリトライ処理を提供する。
use crate::services::engine::{EngineBackend, Resources};
use deft_reversi_engine::{Engine, EngineError, EngineOptions, Model};
use std::{
    cell::Cell,
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::Instant,
};

/// AI の探索に使う既定のスレッド数を返す。
///
/// UI の描画や応答が重くならないよう論理 CPU を 1 つだけ UI 用に残し、
/// 過度にスレッドが増えないよう最大 8 スレッドまでに制限する。
pub fn default_threads() -> usize {
    let available = thread::available_parallelism().map_or(1, |n| n.get());
    available.saturating_sub(1).clamp(1, 8)
}

/// 既定の置換表（Transposition Table）サイズ（MiB）。
///
/// 置換表は探索中に一度計算した局面の評価値や最善手を記録しておくメモリ上のキャッシュであり、
/// 同じ局面に別手順でたどり着いたときの再計算を省くために使う。
pub const DEFAULT_TT_MIB: usize = 128;

/// 実行環境に応じた既定のエンジンリソース設定を返す。
pub fn default_resources() -> Resources {
    Resources {
        threads: default_threads(),
        tt_mib: DEFAULT_TT_MIB,
    }
}

/// GitHub リリース `model-v4` で配布される評価モデルファイル名。
pub const MODEL_FILE: &str = "deft-eval-v4.dpr2";

/// 評価モデルファイルを探索する。
///
/// 探索順序:
/// 1. 環境変数 `DEFT_REVERSI_MODEL`
/// 2. 実行ファイルと同階層の `resources/` ディレクトリ
/// 3. 開発ビルド向けにワークスペース内の `data/eval/` ディレクトリ
pub fn find_model() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DEFT_REVERSI_MODEL").filter(|v| !v.is_empty()) {
        return Some(path.into());
    }
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("resources").join(MODEL_FILE)));
    // 設定画面で相対表記（`..`）を含まない素直なパスを表示するため、親ディレクトリを辿る。
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("data").join("eval").join(MODEL_FILE));
    bundled
        .into_iter()
        .chain(workspace)
        .find(|path| path.is_file())
}

/// DPR2 ファイルヘッダからフォーマットバージョンとファイルサイズ（バイト数）を読み取る。
fn model_header(path: &Path) -> std::io::Result<(u32, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    let version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    Ok((version, file.metadata()?.len()))
}

/// 指定されたモデルパスとリソース設定から [`EngineBackend`] を構築する。
///
/// スレッド数や置換表サイズを変更してエンジンを作り直す際、モデルをメモリ上に二重保持すると
/// メモリ消費が跳ね上がるため、再生成のたびにファイルからモデルを読み直す設計になっている。
/// また、環境によっては大きな置換表を確保できない場合があるため、メモリ不足エラー時は
/// 下限 16 MiB までサイズを半分ずつ減らしながらリトライする。
pub fn backend(model_path: PathBuf, resources: Resources) -> Result<EngineBackend, String> {
    let header = model_header(&model_path);
    let path = model_path.clone();
    // Engine 構造体自体は読み込んだモデルが ProbCut（枝刈り）用の較正データを持つか公開しないため、
    // モデル読み込み直後にフラグを控えておき、設定画面のバージョン情報表示に使う。
    let calibrated = Rc::new(Cell::new(false));
    let seen = calibrated.clone();
    let mut backend = EngineBackend::new(resources, move |resources| {
        let mut tt_mib = resources.tt_mib;
        loop {
            let model_start = Instant::now();
            let model = Model::from_file(&model_path)
                .map_err(|error| format!("{}: {error}", model_path.display()))?;
            seen.set(model.has_calibration());
            let model_time = model_start.elapsed();
            let engine_start = Instant::now();
            let options = EngineOptions::default()
                .threads(resources.threads)
                .tt_mib(tt_mib);
            match Engine::new(model, options) {
                Ok(engine) => {
                    eprintln!(
                        "[engine] model {} ms, engine {} ms ({} threads, TT {tt_mib} MiB) from {}",
                        model_time.as_millis(),
                        engine_start.elapsed().as_millis(),
                        resources.threads,
                        model_path.display(),
                    );
                    return Ok(engine);
                }
                // メモリ不足時は置換表サイズを半減して再試行する（最小 16 MiB）。
                Err(EngineError::Memory(error)) if tt_mib > 16 => {
                    eprintln!(
                        "[engine] TT {tt_mib} MiB: {error}; trying {} MiB",
                        tt_mib / 2
                    );
                    tt_mib /= 2;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    })?;
    match header {
        Ok((format, bytes)) => backend.set_model(path, format, calibrated.get(), bytes),
        Err(error) => eprintln!("[engine] model header: {error}"),
    }
    Ok(backend)
}
