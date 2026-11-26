# エンジン API ガイド

`deft_reversi_engine` は、GUI や CUI のフロントエンドから呼び出して使うネイティブ専用のリバーシ探索エンジンである。wasm 向けの公開、対局の履歴や棋譜の管理、定石の参照、持ち時間の配分は呼び出し側のクレートが受け持つ。

## 設計方針

- **同期と非同期の 2 通りを用意する**：`Engine::search` は呼び出したスレッドで探索して完了まで待つ。`EngineWorker` は探索を別スレッドで動かし、GUI のイベントループを止めずに進捗を受け取れるようにする。
- **目的ごとに指定方法を 1 つにする**：探索の目的は `Goal`、確率的な枝刈りの厳密さ（選択率）は `selectivity` で指定する。完了した反復の種別は `ResultKind` の 1 つの列挙型で表す。
- **通常の公開範囲を絞る**：評価関数の構築や MPC（Multi-ProbCut）の内部ルールなどは、`tuning` feature を有効にしたときだけ公開する。
- **将来の拡張で破壊しない**：`EngineOptions`・`SearchStats`・`Progress`・`SearchResult`・`EngineError` には `#[non_exhaustive]` を付け、フィールド追加で呼び出し側が壊れないようにしている。

## 局面と盤面

`Position` は手番の色（黒・白）を持つ局面型、`Board` は現在の手番側から見た盤面型（探索への入力）である。`Position` は `From` 実装によりそのまま `Board` として探索関数へ渡せる。

盤面の石配置は 64 ビット符号なし整数（`u64`）の各ビットを 1 マスに対応させるビットボードで表す。A1 が最下位ビット（LSB）、H8 が最上位ビット（MSB）に対応する。

```rust
let pos = Position::initial();                          // 黒番の初期局面
let pos: Position = "---...XO--- X".parse()?;           // 64マス＋手番。X/x/* は黒、O/o は白、-/. は空き
println!("{pos}");                                      // 同じ書式（X・O・-、手番 X/O）

pos.side_to_move();                 // Color::Black / Color::White
pos.black(); pos.white();           // u64 ビットボード（A1 が最下位ビット、H8 が最上位ビット）
pos.legal_moves();                  // Moves（合法手の集合）
pos.flips(square);                  // その手で返る石（u64）。不正な手なら 0
pos.play(action)?;                  // 新しい局面を返す。不正な手は MoveError
pos.must_pass(); pos.is_terminal(); pos.empty_count();
pos.disc_count(Color::Black);
pos.final_score(Color::Black);      // 石差（空きマスは勝った側に加える）

let action: Action = "f5".parse()?; // a1〜h8（大文字も可）または "pass"
let square = Square::from_xy(5, 4).unwrap(); // x は列 a〜h、y が行 1〜8（0 始まり、Option<Square>）
```

`Moves` は合法手の集合で、`for square in moves` とイテレータで列挙できるほか、`len`・`is_empty`・`contains(square)`・`bits()` を持つ。

`Board` も同様の操作（`new`・`from_bits`・`legal_moves`・`flips`・`play`・`must_pass`・`is_terminal`・`empty_count`・`final_score`・`player_bits`・`opponent_bits`）を備える。黒白を区別しない計算や盤面の自前管理に向く。定石などで対称な局面をまとめるために、`symmetries()`（回転・反転の 8 通り）と `canonical()`（その最小形）も用意している。

## エンジンの起動と設定

```rust
let model = Model::from_file("model.dpr2")?;    // DPR2 形式。評価重みと、あれば ProbCut 校正を読む
let mut engine = Engine::new(model, EngineOptions::default().threads(8).tt_mib(512))?;

engine.clear_tt()?;                    // 新しい対局の始まりなどで置換表を空にする
engine.set_model(other_model)?;        // モデルを差し替える（置換表も無効化する）
engine.write_evaluation_model(out)?;   // 評価の重みと補助ニューラルネットを DPR2 形式で書き出す（校正なし）
engine.write_analysis_settings(out)?;  // 探索仕様と実際に使う MPC 校正を書き出す
```

`EngineOptions` の既定値は `threads: 1`（呼び出し側のスレッドを含む、上限 256）・`tt_mib: 256`（MiB）である。置換表（TT：Transposition Table）は、一度探索した局面の結果を記憶して再計算を省くためのハッシュ表を指す。

最後の `write_evaluation_model` と `write_analysis_settings` は、同じ評価関数・同じ探索条件で求めた解析結果どうしかを見分けるための出力である。定石クレート（`deft_reversi_book`）の値に付ける識別情報には、この 2 つの出力のハッシュを使う。

## 探索の指定と実行

### 探索リクエスト（`SearchRequest`）

```rust
let request = SearchRequest::new(Goal::Level(18))
    .time_limit(Duration::from_secs(5))
    .node_limit(1_000_000_000)
    .all_moves();                   // 全合法手のスコアも求める（時間は数倍かかる）
let stop = request.stop_handle();   // 別スレッドから停止を要求するハンドル
let batch = SearchRequest::new(Goal::Depth(12)).with_stop(stop.clone()); // 1つのハンドルで複数探索を止める
```

探索の目的 `Goal` には次の 4 種類がある。

| `Goal` | 意味 |
|---|---|
| `Depth(n)` | `n` 手先まで読む（0〜60）。パスは手数に数えない |
| `Endgame` | 終局まで読む。選択率が `Full` なら完全読みになる |
| `Level(n)` | 強さレベル（1〜60）。中盤を深さ `n`・選択率 83% で読み、空きマスが減ったらレベル表の選択率で終局まで読む |
| `Plan(SearchPlan)` | 中盤の深さや終盤の選択率切替を自分で組み立てる（下記） |

- **ProbCut と選択率（`Selectivity`）**：ProbCut は、浅い探索の結果から「このまま深く読んでも見込みが薄い」と統計的に判断した枝を刈る手法である。`.selectivity(Selectivity)` で `Depth` と `Endgame` の枝刈り強度を指定できる。段階は `Percent83`・`Percent85`・`Percent95`・`Percent98`・`Percent99`・`Full`（83/85/95/98/99/100%、一覧は `Selectivity::ALL`）があり、既定値は枝刈りなしの `Full` である。
- `Level` や `Plan` と同時に `.selectivity(...)` を指定すると `EngineError::InvalidRequest` になる。
- `Full` 以外の選択率を使うには、モデルに ProbCut の校正データが含まれている必要がある。校正なしのモデルで指定すると `EngineError::InvalidCalibration` になる。

`SearchPlan` では、中盤の深さ・選択率と、終盤探索に切り替える空きマス数の上限を段階ごとに指定できる。複数の段階が当てはまる局面では、精度の高い段階が使われる。

```rust
// レベル24をベースに、完全読みの開始だけを空き26マスに変更する
let plan = SearchPlan::level(24)?.endgame(Selectivity::Full, 26);

// すべて自分で決める：中盤は深さ20・83%、空き24以下は95%、空き20以下は完全読み
let plan = SearchPlan::midgame(20, Selectivity::Percent83)
    .endgame(Selectivity::Percent95, 24)
    .endgame(Selectivity::Full, 20);
let request = SearchRequest::new(Goal::Plan(plan));
```

レベル表の内容と決め方は [deft-reversi-notes/search-control/2026-11-15-level-table](https://github.com/ikepggthb/deft-reversi-notes/blob/main/search-control/2026-11-15-level-table/README.md) に記録している。

このほか、持ち時間内でレベルを順に上げる `SearchRequest::timed_levels(level, limit)`、候補手を絞る `root_moves(mask)`、特定の手を詳しく評価する `review_move(square)`、全合法手の PV も返す `all_moves_with_pv()` などのビルダーメソッドがある。

### 結果と進捗（`SearchResult`・`Progress`）

```rust
let result = engine.search(&pos, &request)?;
let result = engine.search_with(&pos, &request, |p: &Progress| {
    println!("{:?} {:+} {}", p.kind, p.score, p.nodes);
})?;

result.best_move;       // Option<Action>。None は終局。途中で止まっても合法手を 1 つ返す
result.score;           // Option<i32>。手番側から見た石差。完了した反復が無ければ None
result.kind;            // Option<ResultKind>。Depth { depth, selectivity } または Endgame { selectivity }
result.is_solved();     // 全合法手に対して選択率 Full の終盤読みが完了したか
result.pv;              // Vec<Action>（PV：最善応手手順）
result.moves;           // Vec<MoveScore>。all_moves / review_move 指定時のみ。評価の高い順
result.stop_reason;     // Completed / Requested / TimeLimit / NodeLimit / WorkerFailed
result.stats;           // 訪問ノード数などの統計（並列の内訳は診断用で将来変わり得る）
result.elapsed;         // 探索全体の経過時間
```

`Progress` は反復深化の各段階（または `all_moves` 時の各手）が終わるたびにコールバックへ渡される。`kind`・`score`・`best_move`・`pv`・`nodes`・`elapsed`・`next`・`moves` を持ち、スコアは `SearchResult` と同じく手番側から見た石差である。

### 非同期実行（`EngineWorker`）

```rust
let worker = EngineWorker::new(engine);
let search = worker.start(&pos, request)?;          // 別の探索が実行中なら EngineError::Busy

while let Ok(progress) = search.events().try_recv() { /* GUI は毎フレーム確認する */ }
for progress in search.events() { /* CUI は完了まで受け取り続ける */ }

search.stop();
let result = search.wait()?;                        // is_finished() で終了確認もできる
worker.with(|engine| engine.clear_tt())?;           // 探索中なら終わるまで待ってから実行する
```

`EngineWorker::start` は探索ごとに 8 MiB のスタックを持つスレッドを 1 つ起動する。返された `SearchHandle` を `wait()` せずに破棄（ドロップ）すると、その探索へ停止要求を送る。進捗イベントは標準ライブラリの `std::sync::mpsc` チャネルで送るため、特定の非同期ランタイムには依存しない。

## `tuning` feature

評価関数の学習や MPC パラメータの調整に使う内部 API であり、通常の対局や解析では不要である。

- **評価モデル関連**：`PatternModel`・`PatternPhase`・`PATTERN_COUNT`（11）・`PHASE_COUNT`（60）・`Features::of(&board)`・`global_inputs`・`mirror_weights`・`pattern_table_sizes`・`Model::from_pattern`・`Model::weights_from_file`（校正を読まずに重みだけを読む）・`Model::with_calibration`・`Model::calibration`。
  - 補助ニューラルネット（盤面パターンと合法手数などから線形評価を補正する小型ニューラルネット）を持たないモデルでは、`Features` が指す重みとバイアスの和を 128 で割って四捨五入すると `PatternModel::evaluate` と一致する。補助ニューラルネットを持つモデルでは、128 で割る前にその出力を加算する。
- **MPC 関連**：`ProbCutRule`・`ProbCutSigmas`・`Engine::set_probcut_rules`・`Engine::set_probcut_calibration(rules, sigmas)`・`Engine::set_legacy_main_mpc`・`legacy_main_rules()`。
  - `ProbCutSigmas` は選択的な 5 段階（`ProbCutSigmas::STAGES`：83・85・95・98・99%）ごとに、予測誤差の標準偏差（deviation）の何倍で枝を刈るかを中盤と完全読みで別々に指定する。
  - 既定値 `ProbCutSigmas::NORMAL` は正規分布の両側分位点（`[1.37220, 1.43953, 1.960, 2.326, 2.576]`）であり、`set_probcut_rules` などはこれを使う。実際の探索誤差は正規分布より裾が重いため、`deft-reversi-train mpc` は実測した分位点をモデルファイルに埋め込む。正規分布以外の倍率を使うと `write_analysis_settings` の出力にその倍率が加わる。

## 呼び出し側が受け持つ処理

- **持ち時間の配分**：エンジンは 1 回の探索に対する `time_limit` と外部停止だけを持つ。
- **対局の履歴・待った・棋譜**：`Position::play` を重ねて呼び出し側で管理する。
- **定石**：別クレート `deft-reversi-book` が定石の参照・生成・着手の選択を受け持つ。定石に手が見つからない局面で、このエンジンを呼び出して探索する。
