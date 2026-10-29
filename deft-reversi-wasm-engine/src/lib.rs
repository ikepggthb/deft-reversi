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
pub use board::bitboard::{Board, MoveError};
pub use diagnostics::perft;
pub use model::{Model, ModelMetadata};
pub use position::{Color, ParsePositionError, Position};
pub use search::{
    Engine, EngineError, EngineOptions, Goal, MoveScore, Progress, ResultKind, SearchRequest,
    SearchResult, SearchStats, Selectivity, StopHandle, StopReason,
};
pub use types::{Action, Moves, Square};
#[cfg(feature = "tuning")]
pub use {
    evaluation::{PATTERN_COUNT, PHASE_COUNT, PatternModel, PatternPhase},
    mpc::{EvalSearchMpcConfig, FinalSearchMpcConfig, MpcConfig, MpcParams, MpcRegression},
    search::ProbCutRule,
};

use serde::{Deserialize, Serialize};
use std::time::Duration;
use wasm_bindgen::prelude::*;

fn error(value: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&value.to_string())
}

/// A position including the side to move. Moves return a new position.
#[wasm_bindgen]
pub struct WasmPosition(Position);

#[wasm_bindgen]
impl WasmPosition {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(Position::initial())
    }

    #[wasm_bindgen(js_name = fromString)]
    pub fn from_string(value: &str) -> Result<WasmPosition, JsValue> {
        value.parse().map(Self).map_err(error)
    }

    /// Construct a side-to-move-relative position from bitboards.
    #[wasm_bindgen(js_name = fromBits)]
    pub fn from_bits(player: u64, opponent: u64) -> Result<WasmPosition, JsValue> {
        Position::from_colors(player, opponent, Color::Black)
            .map(Self)
            .ok_or_else(|| error("overlapping discs"))
    }

    #[wasm_bindgen(js_name = toString)]
    pub fn to_position_string(&self) -> String {
        self.0.to_string()
    }

    #[wasm_bindgen(js_name = sideToMove)]
    pub fn side_to_move(&self) -> String {
        match self.0.side_to_move() {
            Color::Black => "black",
            Color::White => "white",
        }
        .into()
    }

    /// A1..H8 order: 0 = empty, 1 = black, 2 = white.
    pub fn cells(&self) -> Vec<u8> {
        (0..64)
            .map(|i| {
                if self.0.black() & (1u64 << i) != 0 {
                    1
                } else if self.0.white() & (1u64 << i) != 0 {
                    2
                } else {
                    0
                }
            })
            .collect()
    }

    #[wasm_bindgen(js_name = legalMoves)]
    pub fn legal_moves(&self) -> Vec<String> {
        self.0
            .legal_moves()
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }

    pub fn play(&self, action: &str) -> Result<WasmPosition, JsValue> {
        let action: Action = action.parse().map_err(error)?;
        self.0.play(action).map(Self).map_err(error)
    }

    #[wasm_bindgen(js_name = mustPass)]
    pub fn must_pass(&self) -> bool {
        self.0.must_pass()
    }
    #[wasm_bindgen(js_name = isTerminal)]
    pub fn is_terminal(&self) -> bool {
        self.0.is_terminal()
    }
}
impl Default for WasmPosition {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum SearchGoal {
    Depth(u8),
    Level(u8),
    Endgame,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    goal: SearchGoal,
    time_limit_ms: Option<u32>,
    node_limit: Option<u32>,
    #[serde(default)]
    all_moves: bool,
}
impl Request {
    fn into_request(self) -> SearchRequest {
        let goal = match self.goal {
            SearchGoal::Depth(n) => Goal::Depth(n),
            SearchGoal::Level(n) => Goal::Level(n),
            SearchGoal::Endgame => Goal::Endgame,
        };
        let mut request = SearchRequest::new(goal);
        if let Some(ms) = self.time_limit_ms {
            request = request.time_limit(Duration::from_millis(ms.into()));
        }
        if let Some(n) = self.node_limit {
            request = request.node_limit(n.into());
        }
        if self.all_moves {
            request = request.all_moves();
        }
        request
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MoveResult {
    action: String,
    score: i32,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultDto {
    best_move: Option<String>,
    score: Option<i32>,
    depth: Option<u8>,
    selectivity: Option<u8>,
    solved: bool,
    pv: Vec<String>,
    moves: Vec<MoveResult>,
    stop_reason: String,
    // Decimal string preserves all u64 values without JavaScript number rounding.
    nodes: String,
    elapsed_ms: f64,
}

#[wasm_bindgen]
pub struct WasmEngine(Engine);
#[wasm_bindgen]
impl WasmEngine {
    /// Model bytes from fetch().arrayBuffer(); ttMiB must be 1..=256.
    #[wasm_bindgen(constructor)]
    pub fn new(model_bytes: &[u8], tt_mib: u32) -> Result<WasmEngine, JsValue> {
        if !(1..=256).contains(&tt_mib) {
            return Err(error("ttMiB must be 1..=256"));
        }
        let model = Model::from_bytes(model_bytes).map_err(error)?;
        Engine::new(
            model,
            EngineOptions::default().threads(1).tt_mib(tt_mib as usize),
        )
        .map(Self)
        .map_err(error)
    }
    #[wasm_bindgen(js_name = clearTt)]
    pub fn clear_tt(&mut self) -> Result<(), JsValue> {
        self.0.clear_tt().map_err(error)
    }
    #[wasm_bindgen(js_name = setModel)]
    pub fn set_model(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.0
            .set_model(Model::from_bytes(bytes).map_err(error)?)
            .map_err(error)
    }
    pub fn search(
        &mut self,
        position: &WasmPosition,
        request: JsValue,
    ) -> Result<JsValue, JsValue> {
        let request: Request = serde_wasm_bindgen::from_value(request).map_err(error)?;
        let result = self
            .0
            .search(position.0, &request.into_request())
            .map_err(error)?;
        let dto = ResultDto {
            best_move: result.best_move.map(|a| a.to_string()),
            score: result.score,
            depth: match result.kind {
                Some(ResultKind::Depth { depth, .. }) => Some(depth),
                _ => None,
            },
            selectivity: result.kind.map(|k| k.selectivity().percent()),
            solved: result.is_solved(),
            pv: result.pv.iter().map(ToString::to_string).collect(),
            moves: result
                .moves
                .iter()
                .map(|m| MoveResult {
                    action: m.action.to_string(),
                    score: m.score,
                })
                .collect(),
            stop_reason: format!("{:?}", result.stop_reason),
            nodes: result.stats.nodes.to_string(),
            elapsed_ms: result.elapsed.as_secs_f64() * 1000.0,
        };
        serde_wasm_bindgen::to_value(&dto).map_err(error)
    }
}
