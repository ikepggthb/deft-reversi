//! Reversi engine for GUI and CUI front ends, with browser bindings in
//! `deft-reversi-wasm`. Game records, opening
//! books and clock management belong to the caller; see `docs/api.md`.
//!
//! ```no_run
//! use deft_reversi_engine::{
//!     Engine, EngineOptions, EngineWorker, Goal, Model, Position, SearchRequest,
//! };
//! use std::time::Duration;
//!
//! let model = Model::from_file("eval-v4.bin")?;
//! let mut engine = Engine::new(model, EngineOptions::default().threads(4))?;
//! let position = Position::initial().play("f5".parse::<deft_reversi_engine::Action>()?)?;
//!
//! // Blocking, with a report after every iteration.
//! let request = SearchRequest::new(Goal::Level(18)).time_limit(Duration::from_secs(2));
//! let result = engine.search_with(&position, &request, |p| println!("{:?} {:+}", p.kind, p.score))?;
//! println!("{:?} {:?}", result.best_move, result.score);
//!
//! // In the background, for an event loop.
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
pub use model::{Model, ModelMetadata};
pub use position::{Color, ParsePositionError, Position};
pub use search::{
    Engine, EngineError, EngineOptions, Goal, MoveScore, Progress, ResultKind, SearchRequest,
    SearchResult, SearchStats, Selectivity, StopHandle, StopReason,
};
pub use types::{Action, Moves, Square};
pub use worker::{EngineWorker, SearchHandle};

/// Model construction and search calibration, for evaluation and MPC tuning.
#[cfg(feature = "tuning")]
pub use {
    evaluation::{PATTERN_COUNT, PHASE_COUNT, PatternModel, PatternPhase},
    mpc::{EvalSearchMpcConfig, FinalSearchMpcConfig, MpcConfig, MpcParams, MpcRegression},
    search::ProbCutRule,
};
