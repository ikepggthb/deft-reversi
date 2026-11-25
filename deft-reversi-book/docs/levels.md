# レベル、深さ、選択率

Book を育成するときは、強さレベルで指定する方法と、中盤の深さ・選択率で指定する方法のどちらか一方を選ぶ。

## 指定方法

CLI では `--levels 4,8,12`（または単一の `--level 22`）か、`--stages 12,16,20 --selectivity 95` のどちらか一方を指定する。両方の同時指定は拒否される。Rust API では `GenerationConfig.analysis` に `AnalysisSchedule::Levels` または `AnalysisSchedule::Stages` を設定する。

- **レベル系列**: 1〜60 の範囲で厳密な昇順（例: `4,8,12`）。
- **深さ系列**: 2〜60 の範囲で深さを厳密に増やし、選択率を下げない。

```sh
target/release/deft-reversi-cli book build --model MODEL --work trial.dbook-work \
  --levels 2,4 --max-plies 2 --seconds 10 --tt-mib 64
target/release/deft-reversi-cli book extend --model MODEL --work trial.dbook-work \
  --levels 4,8,12 --max-plies 12 --seconds 60
target/release/deft-reversi-cli book resume --model MODEL --work trial.dbook-work --seconds 60
```

## 現在のレベル解釈

レベル指定を使うと、探索する親局面の空きマス数に対して `Engine` のレベル表を適用する。序盤・中盤のレベル N は深さ N・選択率 83%（MPC83%）になる。終盤は空きマス数が減るにつれて段階的に選択的終盤や完全読みへ切り替わる。たとえばレベル 22 では空き 28 から選択的終盤（85%）、空き 21 から完全読み（100%）になる。

探索の記録には次の 3 つを分けて残す。

- **要求**: `RequestedAnalysis`（例: `RootLevel` や `ParentDepth`）
- **予定**: `Engine::search_plan` で判定した探索種別（`SearchKind`）
- **完了**: `SearchResult` から得られた実際に完了した方式・深さ・選択率

旧 `ChildLevel` の記録は当時の探索条件として読み取る。既存の確定値を守るために探索を強化するときは、強化後の条件を記録する。

## 続行と再設定

- `resume`: 保存されているレベルや段階の設定をそのまま引き継いで続行する。
- `extend`: レベル系列や探索範囲を変更したいときに使う。
- `reanalyze`: モデル変更などで同じ結果の再利用を無効にしたいときに使う。

旧形式に保存された完了結果は、条件変更の前に取り込む。

関連文書: [評価の意味](value-model.md) / [互換性](compatibility.md)
