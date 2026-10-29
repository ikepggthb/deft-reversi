//! Native Reversi engine. Search I/O and application history belong to adapters.
mod board;
mod diagnostics;
mod evaluation;
mod model;
mod mpc;
mod parallel;
mod search;
#[cfg(test)]
mod test_util;
mod transposition;
mod types;
pub use board::bitboard::{Board, MoveError};
pub use diagnostics::perft;
pub use evaluation::{PATTERN_COUNT, PHASE_COUNT, PatternModel, PatternPhase};
pub use search::{
    Bound, Engine, EngineError, EngineOptions, ProbCutRule, SearchMode, SearchRequest,
    SearchResult, SearchStats, Selectivity, StopHandle, StopReason,
};
pub use types::{Action, Square};

pub use model::{EngineModel, ModelMetadata};
pub use mpc::{EvalSearchMpcConfig, FinalSearchMpcConfig, MpcConfig, MpcParams, MpcRegression};
