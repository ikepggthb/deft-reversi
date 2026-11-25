//! GUI や CUI のフロントエンドから利用するネイティブリバーシエンジンです。
//! 棋譜の管理・Book（Opening Book：序盤の定石データ）・持ち時間制御は呼び出し側の責務とします（詳細は `docs/api.md` を参照してください）。
//!
//! ```no_run
//! use deft_reversi_engine::{
//!     Engine, EngineOptions, EngineWorker, Goal, Model, Position, SearchRequest,
//! };
//! use std::time::Duration;
//!
//! // 評価関数の重みと ProbCut（浅い探索から枝刈りを予測する手法）の較正パラメータを読み込みます。
//! let model = Model::from_file("model.dpr2")?;
//! let mut engine = Engine::new(model, EngineOptions::default().threads(4))?;
//! let position = Position::initial().play("f5".parse::<deft_reversi_engine::Action>()?)?;
//!
//! // ブロッキング実行（各探索深さの完了時に進捗コールバックを受け取ります）。
//! let request = SearchRequest::new(Goal::Level(18)).time_limit(Duration::from_secs(2));
//! let result = engine.search_with(&position, &request, |p| println!("{:?} {:+}", p.kind, p.score))?;
//! println!("{:?} {:?}", result.best_move, result.score);
//!
//! // イベントループ向けのバックグラウンド非同期探索です。
//! let worker = EngineWorker::new(engine);
//! let search = worker.start(&position, SearchRequest::new(Goal::Endgame).all_moves())?;
//! while !search.is_finished() {
//!     for progress in search.events().try_iter() {
//!         println!("{:?}", progress.pv);
//!     }
//!     std::thread::sleep(Duration::from_millis(16));
//! }
//! for entry in search.wait()?.moves {
//!     println!("{} {:+}", entry.action, entry.score);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
mod board;
mod diagnostics;
#[cfg_attr(not(feature = "tuning"), allow(dead_code))]
mod evaluation;
mod model;
#[cfg_attr(not(feature = "tuning"), allow(dead_code))]
mod mpc;
mod parallel;
mod position;
mod search;
#[cfg(test)]
mod test_util;
mod transposition;
mod types;
mod worker;
pub use board::bitboard::{Board, MoveError};
pub use diagnostics::perft;
pub use model::Model;
pub use position::{Color, ParsePositionError, Position};
pub use search::{
    Engine, EngineError, EngineOptions, Goal, MoveScore, Progress, ResultKind,
    SEARCH_SEMANTICS_REVISION, SearchPlan, SearchRequest, SearchResult, SearchStats, Selectivity,
    StopHandle, StopReason, level_reading,
};
pub use types::{Action, Moves, Square};
pub use worker::{EngineWorker, SearchHandle};

/// 評価関数および MPC（Multi-ProbCut：複数の信頼度段階で枝刈りを行う手法）のチューニング用に、
/// モデル構築や探索パラメータの調整を行う API を公開します。
#[cfg(feature = "tuning")]
pub use {
    evaluation::{
        FEATURE_COUNT, Features, MOBILITY_WEIGHT_COUNT, PATTERN_COUNT, PATTERN_WEIGHT_COUNT,
        PEN_DIM, PEN_GLOBALS, PEN_HIDDEN, PEN_INPUTS, PHASE_COUNT, PatternModel, PatternPhase,
        PenPhase, global_inputs, mirror_weights, pattern_table_sizes,
    },
    search::{ProbCutRule, ProbCutSigmas},
};

/// 旧メインブランチ互換の ProbCut ルール一覧を返します（旧形式モデルからの変換用です）。
#[cfg(feature = "tuning")]
pub fn legacy_main_rules() -> Vec<ProbCutRule> {
    mpc::legacy_main_rules()
}

/// 外部解析結果のメタデータに記録するクレートバージョンです。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// このビルドに使用された Git リビジョンです（ローカルに未コミットの変更がある場合は接尾辞が付きます）。
pub const GIT_REVISION: &str = env!("DEFT_GIT_REVISION");

/// クレートバージョンとソースリビジョンを結合したビルド識別文字列です。
pub const BUILD_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (git ",
    env!("DEFT_GIT_REVISION"),
    ")"
);
