# Evaluation

この文書は `deft-reversi-engine` の評価関数の内部設計メモである。

次回以降の改善計画は [evaluation-improvement-plan.md](/home/azure/dev/deft-reversi/deft-reversi-engine/docs/evaluation-improvement-plan.md) にまとめている。

評価関数は外部安定 API ではない。探索速度と実装の見通しを優先して変更してよい内部コンポーネントとして扱う。

## 役割

評価は 2 つの型に分ける。

- `Evaluator`: 評価重みを持つ immutable な型。探索 thread 間で共有できる。
- `FeatureIndexes`: ある局面の pattern code を持つ探索状態。探索 path ごとに持つ。

探索中の標準 API は次の形である。

```rust
let engine_file = EngineFile::read_file("data/eval/eval.bin")?;
let (_metadata, evaluator, _mpc) = engine_file.into_parts()?;
let state = FeatureIndexes::from_board(&board);
let score = evaluator.evaluate(&board, &state);
```

単発評価や test では `evaluate_board_slow` を使える。

```rust
let score = evaluator.evaluate_board_slow(&board);
```

`evaluate_board_slow` は board から feature を作り直すため、探索本体では使わない。

## 評価値

評価値は現在手番側から見た値である。

内部では raw score を `SCORE_SCALE = 128` で割り、四捨五入して disc score に変換する。返り値は `[-64, 64]` に clamp する。

```text
raw score -> rounded disc score -> clamp [-64, 64]
```

## Pattern Feature

pattern code は board square を base-3 で encode した値である。

```text
0: empty
1: opponent
2: player
```

`FeatureIndexes` は `N_PATTERNS * N_ROTATIONS` 個の pattern code を flat な `[u16; N_FEATURES]` として持つ。現在は 11 pattern x 4 rotations である。

```text
feature_idx = pattern_idx * N_ROTATIONS + rotation
```

将来の差分更新用に、`SQUARE_TO_FEATURES` を `PATTERN_DEFINITIONS` から compile time に生成する。各要素は、その square が影響する `feature_idx` と base-3 の桁重みを持つ。

## Phase

現在の phase は 60 段階で、`Board::move_count()` をそのまま使う。

```rust
phase = board.move_count().min(N_PHASES - 1)
```

現在の `Board` は current-player-relative なので、黒番用/白番用の評価重みは持たない。評価重みは `[phase]` の 1 次元構造にする。

## File Format

評価ファイルは binary で保存する。serde 用の schema と runtime 構造は分けたまま、`EngineFile` を `bincode` で読み書きする。

内部schemaは次の形である。

```json
{
  "format_version": 2,
  "metadata": {
    "eval_name": "default",
    "eval_version": "0",
    "trained_at": "",
    "engine_version": "",
    "git_commit": "",
    "n_data_set": 0,
    "n_iteration": 0,
    "score_scale": 128,
    "n_phases": 60,
    "n_patterns": 11,
    "pattern_schema_version": 1
  },
  "evaluator": {
    "phases": [
      {
        "pattern_weights": [[0]],
        "mobility_weights": [0],
        "bias": 0
      }
    ]
  },
  "mpc": {
    "eval_search": {
      "search_lv_by_depth": [0, 0, 0],
      "a": { "constant": 0.0, "empties": 0.0, "depth": 0.0, "mpc_depth": 0.0, "parity": 0.0, "parity_times_empties": 0.0, "parity_times_mpc_depth": 0.0 },
      "b": { "constant": 0.0, "empties": 0.0, "depth": 0.0, "mpc_depth": 0.0, "parity": 0.0, "parity_times_empties": 0.0, "parity_times_mpc_depth": 0.0 },
      "e_std": { "constant": 0.0, "empties": 0.0, "depth": 0.0, "mpc_depth": 0.0, "parity": 0.0, "parity_times_empties": 0.0, "parity_times_mpc_depth": 0.0 }
    },
    "final_search": {
      "search_lv_by_empties": [0, 0, 0],
      "a": { "constant": 0.0, "empties": 0.0, "depth": 0.0, "mpc_depth": 0.0, "parity": 0.0, "parity_times_empties": 0.0, "parity_times_mpc_depth": 0.0 },
      "b": { "constant": 0.0, "empties": 0.0, "depth": 0.0, "mpc_depth": 0.0, "parity": 0.0, "parity_times_empties": 0.0, "parity_times_mpc_depth": 0.0 },
      "e_std": { "constant": 0.0, "empties": 0.0, "depth": 0.0, "mpc_depth": 0.0, "parity": 0.0, "parity_times_empties": 0.0, "parity_times_mpc_depth": 0.0 }
    }
  }
}
```

`mpc` は評価値そのものではなく、探索側で使う ProbCut 用の統計値を持つ。`metadata` には入れず、学習済み重みと同じファイルに別 payload として保存する。

`evaluator.phases` は `[phase]` の順で、60 要素を持つ。読み込み時に pattern weight は flat な runtime table に変換する。評価時は pattern ごとに累積 offset を進めながら参照する。

## 次の改善

今回の構造分離では、feature はまだ board から再計算する。次の段階では edax / Egaroucid と同じく、着手位置と flip bit から `EvalState` を差分更新する。
