use js_sys::Array;
use wasm_bindgen::prelude::*;

use deft_reversi_engine::{
    position_bit_to_num, Board, Evaluator, Solver, SolverResult, SolverType,
};
use serde::Serialize;

#[wasm_bindgen]
pub struct AiSolver {
    solver: Solver,
}

fn u64_from_parts(low: u32, high: u32) -> u64 {
    (low as u64) | ((high as u64) << 32)
}

fn u64_to_parts(x: u64) -> (u32, u32) {
    (x as u32, (x >> 32) as u32)
}

#[derive(Serialize)]
struct SolverTypeForJs {
    kind: String,
    depth: Option<i32>,
    selectivity_lv: i32,
}

impl From<SolverType> for SolverTypeForJs {
    fn from(value: SolverType) -> Self {
        match value {
            SolverType::Eval(depth, sel) => Self {
                kind: "Eval".to_string(),
                depth: Some(depth),
                selectivity_lv: sel,
            },
            SolverType::Final(sel) => Self {
                kind: "Final".to_string(),
                depth: None,
                selectivity_lv: sel,
            },
        }
    }
}

#[derive(Serialize)]
struct SolverResultForJs {
    best_move_low: u32,
    best_move_high: u32,
    eval: i32,
    solver_type: SolverTypeForJs,
    searched_nodes_low: u32,
    searched_nodes_high: u32,
    searched_leaf_nodes_low: u32,
    searched_leaf_nodes_high: u32,
}

impl From<SolverResult> for SolverResultForJs {
    fn from(result: SolverResult) -> Self {
        let (best_move_low, best_move_high) = u64_to_parts(result.best_move);
        let (searched_nodes_low, searched_nodes_high) = u64_to_parts(result.searched_nodes);
        let (searched_leaf_nodes_low, searched_leaf_nodes_high) =
            u64_to_parts(result.searched_leaf_nodes);
        Self {
            best_move_low,
            best_move_high,
            eval: result.eval,
            solver_type: result.solver_type.into(),
            searched_nodes_low,
            searched_nodes_high,
            searched_leaf_nodes_low,
            searched_leaf_nodes_high,
        }
    }
}

#[wasm_bindgen]
impl AiSolver {
    #[wasm_bindgen(constructor)]
    pub fn new(eval_string: &str) -> Result<Self, JsValue> {
        let evaluator = Evaluator::read_string(eval_string)
            .map_err(|e| JsValue::from_str(&format!("Evaluator load failed: {}", e)))?;
        Ok(Self {
            solver: Solver::new(evaluator),
        })
    }

    /// solver.solve と同じデータを返す
    #[wasm_bindgen]
    pub fn solver_result_for_turn_bits(
        &mut self,
        player_bits_low: u32,
        player_bits_high: u32,
        opponent_bits_low: u32,
        opponent_bits_high: u32,
        lv: i32,
    ) -> Result<JsValue, JsValue> {
        let player = u64_from_parts(player_bits_low, player_bits_high);
        let opponent = u64_from_parts(opponent_bits_low, opponent_bits_high);
        if (player & opponent) != 0 {
            return Err(JsValue::from_str("invalid board: overlap"));
        }
        let board = Board { player, opponent };
        if board.moves().count_ones() == 0 {
            let empty = SolverResultForJs {
                best_move_low: 0,
                best_move_high: 0,
                eval: 0,
                solver_type: SolverTypeForJs {
                    kind: "Eval".to_string(),
                    depth: Some(0),
                    selectivity_lv: 0,
                },
                searched_nodes_low: 0,
                searched_nodes_high: 0,
                searched_leaf_nodes_low: 0,
                searched_leaf_nodes_high: 0,
            };
            return serde_wasm_bindgen::to_value(&empty)
                .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)));
        }

        let result = self.solver.solve(&board, lv);
        let mapped: SolverResultForJs = result.into();
        serde_wasm_bindgen::to_value(&mapped)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    }
}
