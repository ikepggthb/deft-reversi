# eval改善とメタデータ追加の次回実装計画

## Summary

次回は `deft-reversi-engine` の評価関数について、差分評価には入らず、現在の構造を保ったまま以下を実装する。

- 評価ファイルのメタデータを明確化する
- `PATTERN_TABLE_SIZES` を `PATTERN_DEFINITIONS` から `const fn` 生成する
- `EvalState::refresh()` で盤面64マスの状態を先に作り、pattern生成時のbit shift重複を減らす
- `Evaluator::read_file()` を `serde_json::from_reader` に変更する
- この計画を次回作業用の設計メモとして維持する

## Key Changes

`EvalFile` は次の形に変更する。旧schema互換は不要とする。

```rust
struct EvalFile {
    format_version: u32,
    metadata: EvalMetadata,
    weights: Vec<SerializedWeights>,
}

struct EvalMetadata {
    eval_name: String,
    eval_version: String,
    trained_at: Option<String>,
    engine_version: Option<String>,
    git_commit: Option<String>,
    n_data_set: u64,
    n_iteration: u64,
    score_scale: i32,
    n_phases: usize,
    n_patterns: usize,
    pattern_schema_version: u32,
}
```

- `Evaluator` は `metadata: EvalMetadata` を保持する。
- `version`, `n_data_set`, `n_iteration` の直持ちは削除する。
- `Evaluator::metadata(&self) -> &EvalMetadata` を追加する。
- `eval.rs` 内ではログ出力しない。呼び出し側が `metadata()` を見て必要に応じてログを出す。
- `Evaluator::default()` のmetadataは、`eval_name = "default"`, `eval_version = "0"`, `format_version = 1`, `score_scale = SCORE_SCALE`, `n_phases = N_PHASES`, `n_patterns = N_PATTERNS`, `pattern_schema_version = 1` とする。
- `data/eval/eval.json` は新schemaへ更新する。

## Implementation Changes

`PATTERN_TABLE_SIZES` は手書きをやめる。

```rust
pub const PATTERN_TABLE_SIZES: [usize; N_PATTERNS] = build_pattern_table_sizes();
```

`build_pattern_table_sizes()` は `PATTERN_DEFINITIONS[i].n_squares` から `POW3[...]` を返す。現在の `[2]` と `[1]` の入れ替わりを自然に解消する。

`EvalState::refresh()` は最初に `square_states: [u8; 64]` を作る。

```text
0 = empty
1 = opponent
2 = player
```

pattern生成側は `square_states[square as usize]` を読むだけにする。

`Evaluator::read_file()` は `fs::read_to_string` をやめる。

```rust
let file = fs::File::open(path)?;
let eval_file: EvalFile = serde_json::from_reader(file).map_err(invalid_data)?;
```

`Evaluator::read_string()` はテスト用・埋め込み用として残す。

`write_file()` は新schemaでJSONを書き出す。pretty出力は使わず、現状どおりcompact JSONでよい。

今回やらない項目:

- unsafe化
- accumulate完全展開
- `PATTERN_STARTS` 導入
- `[-63, 63]` clamp変更
- 差分評価
- SIMD
- 評価項目追加
- binary評価ファイル化

## Documentation

- 既存の `deft-reversi-engine/docs/evaluation.md` は新schema例へ更新する。
- `evaluation.md` からこの計画docへリンクを追加する。
- Edax/Egaroucidとの差分理由は、この計画docでは短く扱う。詳細比較は実装時に必要な範囲だけ参照する。

## Test Plan

- `cargo test -p deft_reversi_engine` を実行する。
- 既存の `json_round_trip_uses_new_schema` を新metadata schemaに合わせて更新する。
- `bundled_eval_file_loads` で `data/eval/eval.json` の新schemaが読み込めることを確認する。
- `metadata()` の値がdefault evaluatorとJSON読み込み後で期待どおり取得できるテストを追加する。
- `PATTERN_TABLE_SIZES` が `PATTERN_DEFINITIONS` の順序と一致するテストを追加する。
- `refresh_matches_recreated_state` と `slow_board_eval_matches_state_eval` で、`square_states` 化後も評価結果が変わらないことを確認する。

## Assumptions

- 評価関数ファイルの旧schema互換は不要。
- `eval.rs` はライブラリなので、読み込み時ログは出さない。
- メタデータは評価速度に関係しないため、runtime評価ループには入れない。
- 差分評価、SIMD、評価項目追加、binary評価ファイル化は今回の対象外。
