# 保存形式と旧データの引継ぎ

## 現在のファイル形式

| ファイル | 読み込み可能な版 | 新しく保存する版 | 用途 |
|---|---|---|---|
| `.dbook-work` | v1〜v9 | v8 または v9 | 生成・更新の元データと再開状態 |
| `.dbook` | v1〜v5 | v4 または v5 | 対局用の読み取りデータ（生成再開には使わない） |

2 つのファイル形式で、点数・探索方式・品質の意味は共通している。work v9 / play v5 は、中盤のしきい値探索で求めた集合全体の上限を旧版が処理できない場合に使う。従来の表現で保存できる内容には work v8 / play v4 を使う。`.dbook-work` から直接 `export()` して対局に使えるため、`.dbook` ファイルの出力は任意である。

現在の形式では、モデル等、完了した探索、値の説明、評価値をそれぞれの一覧へ保存し、番号で対応付ける。1 回の探索結果を「最良手の値」と「ほかの手の上限」に使っても、元の探索記録は 1 件しか保存しない。過去の計算に使った入力記録の連鎖は保存しない。

ヘッダーは識別文字 8 バイト、形式版 `u32`、ルール版 `u32`、本文長 `u64`、本文の SHA-256 の 32 バイトで合計 56 バイト（数値は little-endian）となる。Rust の構造体配置をそのままファイルへ書くことはない。

生成設定は、レベル系列か深さ・選択率系列かを表すタグと、選んだ系列だけを保存する。旧版の 2 つの設定欄は読み込み時だけ解釈する。

## 旧形式からの変換で残すもの

局面、登録した手、直接探索の既知の値・条件、手動値、最終石差について分かっている上下限、生成設定を残す。旧形式に残った完了済みの結果も更新手順を通して取り込み、未完了の仕事は続行対象にする。

- 失われた探索方式は不明のまま扱い、深さが空きマス数と同じという理由だけで終盤探索や完全読み切りへ変更しない。
- 旧 v7 / v3 の計算元参照は読み込み時に検査し、新しい保存へは引き継がない。計算した値は現在の情報から作り直す。
- 旧 play v3 で手動値と旧探索の評価を合成した場合（旧版では手動値の表示深さも探索の最低深さへ含めていた）、旧入力関係を当時の規則で検査してから、入力に残る探索条件を現在の方法で集計し直す。探索品質を変更した場合は、旧品質を前提にしているか判断できない任意拡張を破棄する。
- 旧 work は子の値を親へ反映する方式へ移行する。以前の直接探索結果は残すが、採用値やその品質が変わる場合がある。

CLI は変換元の版、局面数、登録手数、採用する局面値・候補値が変わった件数、仕事一覧を組み直したか（`jobs_rebuilt`）を標準エラーへ報告する。Rust API では `migration_report()` で同じ情報（`MigrationReport`）を取得できる。この変換報告自体は再開データの意味に影響しない。

元ファイルを壊さないよう、変換時はコピーを作ってから検証・修復する。

```sh
cp openings.dbook-work next.dbook-work
target/release/deft-reversi-cli book verify --work next.dbook-work
target/release/deft-reversi-cli book repair --work next.dbook-work --book next.dbook
target/release/deft-reversi-cli book verify --work next.dbook-work
target/release/deft-reversi-cli book verify --book next.dbook
```

`verify` は入力を書き換えない。`repair` は指定先を現在の形式で保存する。旧バイナリは新しい形式を読めないため、元ファイルを残しておく。

## ファイル互換性と探索の互換性

ファイルが読み込めることと、その推定値を現在のモデルと混ぜてよいことは別である。`analysis_identity` はモデル重み、校正、sigma、探索 feature、探索仕様の revision から作る 32 バイトの識別値である。

現在の探索仕様の revision（`SEARCH_SEMANTICS_REVISION`）は `2` である。レベルの意味・枝刈り・点数の意味が変わる場合は、別途 revision の更新を判断する。

モデル等が変わった場合はコピーに `reanalyze` を使う。互換性のない推定は無効にし、登録リンク・手動の原記録・ルールが一致する最終石差の確定情報は残す。古いモデルの手動値は黙って現在の値として使わない。

読み込み時は、長さ、件数、割り当て上限、盤面、合法手・パス、参照番号、探索対象、品質、上下限、登録先を検査する。不明な必須拡張は拒否する。未知の任意拡張は無編集の往復で保持し、意味が変わる更新では破棄する。

## Rust API の変更対応表

旧 API からの移行では、以下の対応で呼び出し側を修正する。

| 以前 | 現在 |
|---|---|
| `Assessment` | `Evaluation` |
| `claim()` / `ValueClaim` | `result()` / `Score`（推定と最終石差をここで区別） |
| `analysis().summary()` | `conditions()`（中盤・終盤それぞれの条件を直接読む） |
| `analysis()` / `Derivation` / `CalculationSubject` | `source()`（計算対象の盤面や手は直接の欄にある） |
| `project(SearchProjection::BestMove)` | `best_move_value()` |
| `project(SearchProjection::OtherMoveUpper)` | `other_moves_upper_bound()` |
| `AnalysisQuality` の表示欄 | `depth()`・`selectivity()`・`is_solved()`（編集可能な確定フラグは持たない） |
| `Bound::Exact` / `ProvenLower` / `ProvenUpper` | `Bound` は `Point`・`Lower`・`Upper` だけ。最終石差かは `Score::Final` または `final_bounds()` で確認 |
| `candidate_evidence()` | `move_values()`（`direct`・`child`・`manual`・`final_score` と `selected` / `selected_from` を返す） |
| `BookEntry::proof()` | `final_score()`（手動値とは独立した最終石差の情報） |
| `UsePolicy.derived` / `legacy` | `allow_calculated` / `allow_imported`（真偽値） |
| `GenerationConfig.legacy` / `frontier_only` | `allow_imported` / `unregistered_only` |
| `SchedulePolicy::Frontier` | `SchedulePolicy::Promising`（CLI の `--schedule frontier` も引き続き受理） |
| `frontier_candidates` などの統計名 | `unregistered_candidates` など、未登録手であることを示す名前 |

外部の旧入力は `legacy::ImportedValue` で明示し、`Evaluation::import` または `record_imported` へ渡す。`legacy` 内の旧分類は通常の評価処理へ持ち込まない。旧方式の途中状態は局面データから削除し、読み込み中だけ扱う。

関連文書: [保存場所と API](architecture.md) / [ソースの読み方](source-review.md) / [保守性の見直し](maintainability.md) / [検証](validation.md)
