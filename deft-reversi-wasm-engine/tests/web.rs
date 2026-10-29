#![cfg(target_arch = "wasm32")]
use deft_reversi_wasm_engine::PatternModel;
use deft_reversi_wasm_engine::{WasmEngine, WasmPosition};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

fn engine() -> WasmEngine {
    let mut bytes = Vec::new();
    PatternModel::zero().write_to(&mut bytes).unwrap();
    WasmEngine::new(&bytes, 1).unwrap()
}
fn request(json: &str) -> JsValue {
    #[derive(serde::Serialize, Deserialize)]
    struct Input {
        goal: serde_json::Value,
    }
    let value: Input = serde_json::from_str(json).unwrap();
    value
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
        .unwrap()
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    best_move: Option<String>,
    score: Option<i32>,
    solved: bool,
    elapsed_ms: f64,
    nodes: String,
}

#[wasm_bindgen_test]
fn position_operations_and_errors() {
    let position = WasmPosition::new();
    let bits = WasmPosition::from_bits(0x0000000810000000, 0x0000001008000000).unwrap();
    assert_eq!(bits.cells(), position.cells());
    assert!(WasmPosition::from_bits(1, 1).is_err());
    assert_eq!(position.cells().iter().filter(|&&c| c != 0).count(), 4);
    assert_eq!(position.legal_moves(), ["D3", "C4", "F5", "E6"]);
    let next = position.play("f5").unwrap();
    assert_eq!(next.side_to_move(), "white");
    assert_eq!(position.side_to_move(), "black");
    assert!(position.play("pass").is_err());
    assert!(position.play("a1").is_err());
    assert!(WasmPosition::from_string("bad").is_err());
    assert_eq!(
        WasmPosition::from_string(&next.to_position_string())
            .unwrap()
            .cells(),
        next.cells()
    );
}

#[wasm_bindgen_test]
fn search_runs_with_browser_clock_and_validates_requests() {
    let mut engine = engine();
    let position = WasmPosition::new();
    let result = engine
        .search(&position, request(r#"{"goal":{"kind":"depth","value":2}}"#))
        .unwrap();
    let result: Output = serde_wasm_bindgen::from_value(result).unwrap();
    assert!(position.legal_moves().contains(&result.best_move.unwrap()));
    assert!(result.score.is_some());
    assert!(!result.solved);
    assert!(result.elapsed_ms.is_finite());
    assert!(result.nodes.parse::<u64>().unwrap() > 0);
    assert!(
        engine
            .search(&position, request(r#"{"goal":{"kind":"level","value":0}}"#))
            .is_err()
    );
    assert!(engine.search(&position, JsValue::NULL).is_err());
    assert!(engine.set_model(b"invalid").is_err());
    engine.clear_tt().unwrap();
    assert!(
        engine
            .search(&position, request(r#"{"goal":{"kind":"depth","value":1}}"#))
            .is_ok()
    );
}

#[wasm_bindgen_test]
fn terminal_and_forced_pass() {
    let mut engine = engine();
    let terminal = WasmPosition::from_string(&format!("{} X", "X".repeat(64))).unwrap();
    let result = engine
        .search(&terminal, request(r#"{"goal":{"kind":"endgame"}}"#))
        .unwrap();
    let result: Output = serde_wasm_bindgen::from_value(result).unwrap();
    assert!(result.best_move.is_none());
    assert_eq!(result.score, Some(64));
    assert!(result.solved);
    let pass = WasmPosition::from_string(&format!("-XO{} X", "O".repeat(61))).unwrap();
    assert!(pass.must_pass());
    assert!(!pass.is_terminal());
    assert_eq!(pass.play("pass").unwrap().legal_moves(), ["A1"]);
    let result = engine
        .search(&pass, request(r#"{"goal":{"kind":"endgame"}}"#))
        .unwrap();
    let result: Output = serde_wasm_bindgen::from_value(result).unwrap();
    assert_eq!(result.best_move.as_deref(), Some("pass"));
}

#[wasm_bindgen_test]
fn time_and_node_limits_return_legal_fallbacks() {
    let mut engine = engine();
    let position = WasmPosition::new();
    for (key, value) in [("timeLimitMs", 0), ("nodeLimit", 1)] {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Limited {
            goal: serde_json::Value,
            #[serde(flatten)]
            limits: std::collections::BTreeMap<String, u32>,
        }
        let request = Limited {
            goal: serde_json::json!({"kind": "depth", "value": 10}),
            limits: [(key.to_owned(), value)].into(),
        };
        let request = request
            .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
            .unwrap();
        let result = engine.search(&position, request).unwrap();
        let result: Output = serde_wasm_bindgen::from_value(result).unwrap();
        assert!(position.legal_moves().contains(&result.best_move.unwrap()));
    }
}
