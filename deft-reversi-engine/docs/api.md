# エンジン API の設計

`deft_reversi_engine` はネイティブ専用のクレートで、GUI と CUI から呼び出す。wasm 向けの公開、対局の履歴・棋譜・定石、持ち時間の配分は、呼び出し側のクレートが受け持つ。

## 方針

- **同期と非同期の2段**：`Engine::search` は呼び出したスレッドで探索してブロックする。`EngineWorker` はそれを別スレッドで動かし、GUI のイベントループから使えるようにする。
- **1つの目的に1つの書き方**：探索の目的は `Goal`、選択率は `selectivity` だけで指定する。結果の種類は `ResultKind` の1つの欄で表す。
- **公開範囲は最小限**：評価関数の組み立て、MPC の内部、ProbCut の個別ルールは `tuning` feature のときだけ公開する。
- **欄を足しても壊れない**：`EngineOptions`・`SearchStats`・`Progress`・`SearchResult`・`EngineError` には `#[non_exhaustive]` を付ける。

## 局面

`Position` は手番の色を持つ局面、`Board` は手番側から見た盤面（探索の入力）である。`Position` は `Board` に変換できる。

```rust
let pos = Position::initial();                          // 黒番の初期局面
let pos: Position = "---...XO--- X".parse()?;           // 64マス＋手番。X/x/* は黒、O/o は白、-/. は空き
println!("{pos}");                                      // 同じ書式（X・O・-、手番 X/O）

pos.side_to_move();                 // Color::Black / Color::White
pos.black(); pos.white();           // u64。A1 が最下位ビット、H8 が最上位ビット
pos.legal_moves();                  // Moves
pos.flips(square);                  // その手で返る石（u64）。不正な手なら 0
pos.play(action)?;                  // 新しい局面。不正な手は MoveError
pos.must_pass(); pos.is_terminal(); pos.empty_count();
pos.disc_count(Color::Black);
pos.final_score(Color::Black);      // 石差。空きマスは勝った側に加える

let action: Action = "f5".parse()?; // a1〜h8（大文字も可）と "pass"
let square = Square::from_xy(5, 4); // x が列 a〜h、y が行 1〜8（0 始まり）
```

`Moves` は合法手の集合で、`for square in moves` で列挙できる。`len`・`is_empty`・`contains(square)`・`bits()` を持つ。

`Board` も同じ操作（`new`・`from_bits`・`legal_moves`・`flips`・`play`・`is_terminal`・`empty_count`・`final_score`・`player_bits`・`opponent_bits`）を持つ。色を扱わない呼び出し側や、盤面を自前で管理する呼び出し側のためのもの。

## エンジン

```rust
let model = Model::from_file("eval-v4.bin")?;   // v3/v4 の JSON・bincode、DPR2。重みと MPC 設定を読む
let mut engine = Engine::new(model, EngineOptions::default().threads(8).tt_mib(512))?;

engine.clear_tt()?;                 // 新しい対局の始まりなど
engine.set_model(other_model)?;     // 置換表も無効にする
engine.metadata();                  // モデルのメタデータ（あれば）
```

`EngineOptions` の既定値は 1 スレッド・256 MiB。スレッド数は呼び出し側のスレッドを含む。

### 要求

```rust
let request = SearchRequest::new(Goal::Level(18))
    .time_limit(Duration::from_secs(5))
    .node_limit(1_000_000_000)
    .all_moves();                   // 全合法手の値も求める（時間は数倍かかる）
let stop = request.stop_handle();   // 別スレッドから止める
```

| `Goal` | 意味 |
|---|---|
| `Depth(n)` | n 手先まで読む（0〜60）。パスは手数に数えない |
| `Endgame` | 終局まで読む。選択率が `Full` なら完全読み |
| `Level(n)` | 参照実装のレベル（1〜60）。深さ・終盤への切り替え・選択率をレベルが決める |

`.selectivity(Selectivity)` は `Depth` と `Endgame` の確率的な枝刈り（MPC）の強さを決める。既定は `Full`（枝刈りしない）。`Level` と一緒に指定すると要求のエラーになる。

### 結果

```rust
let result = engine.search(&pos, &request)?;
let result = engine.search_with(&pos, &request, |p: &Progress| {
    println!("{:?} {:+} {}", p.kind, p.score, p.nodes);
})?;

result.best_move;       // Option<Action>。None は終局。探索が途中で止まっても合法手を返す
result.score;           // Option<i32>。手番側から見た石差。None は完了した反復が無い
result.kind;            // Option<ResultKind>。Depth { depth, selectivity } / Endgame { selectivity }
result.is_solved();     // 選択率 Full の終盤読みが完了した
result.pv;              // Vec<Action>
result.moves;           // Vec<MoveScore>。all_moves を指定したときだけ。値の大きい順
result.stop_reason;     // Completed / Requested / TimeLimit / NodeLimit / WorkerFailed
result.stats;           // ノード数など。並列の内訳は診断用で、将来変わることがある
result.elapsed;
```

`Progress` は反復が1つ完了するたびに渡す。`kind`・`score`・`best_move`・`pv`・`nodes`・`elapsed` を持つ。値は最終結果と同じく手番側から見たもの。

### 非同期

```rust
let worker = EngineWorker::new(engine);
let search = worker.start(&pos, request)?;          // 探索中なら EngineError::Busy

while let Ok(progress) = search.events().try_recv() { /* GUI は毎フレーム覗く */ }
for progress in search.events() { /* CUI は待ちながら受け取る */ }

search.stop();
let result = search.wait()?;                        // is_finished() で覗くこともできる
worker.with(|engine| engine.clear_tt())?;           // 探索中なら終わるまで待つ
```

探索ごとに 8 MiB のスタックを持つスレッドを作る。`SearchHandle` を `wait` せずに捨てると、その探索を止める。進捗は std の mpsc チャネルで送り、非同期ランタイムには依存しない。

## `tuning` feature

評価関数やMPCの調整に使う API。通常の利用では不要。

- `PatternModel`・`PatternPhase`・`PATTERN_COUNT`・`PHASE_COUNT`、`Model::from_pattern`
- `MpcConfig` とその構成要素、`Engine::mpc_config`・`set_mpc_config`
- `ProbCutRule`・`Engine::set_probcut_rules`、旧 main の MPC を使う `Engine::set_legacy_main_mpc`

## 呼び出し側に任せるもの

- 持ち時間の配分。エンジンは1回の探索の `time_limit` と停止だけを持つ。
- 対局の履歴・undo・棋譜。`Position::play` を重ねて作る。
- 定石。
