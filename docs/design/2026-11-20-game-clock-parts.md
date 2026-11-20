# 時間管理の部品構成・ログ・延長モード（2026-11-20、ロードマップ段階2）

`2026-11-20-time-control-roadmap.md` の段階2（2-1〜2-5）で行った変更の記録。動作（どの反復で止めるか、何秒使うか）は `0c53a14` と同じで、構造とログだけを変えた。今の方式の動作の説明は `2026-11-20-game-clock-explained.md` にある。

ブランチ `research/time-predict`。コミット：

| コミット | 内容 |
|---|---|
| `9025b07` | 4部品への分離、分布としての時間予測、GameContext（2-1〜2-3） |
| `0fcffc8` | JSONL のログ `DEFT_CLOCK_LOG`（2-4） |
| `2d04952` | 延長モード `DEFT_CLOCK_EXTEND`（2-5） |

## 1. 構成

`deft-reversi-cli/src/game_clock/`：

| ファイル | 内容 |
|---|---|
| `mod.rs` | `Budget`（手番の予算と探索の実行）、`Forecast`（反復ごとに3部品を呼ぶ）、`reflex`、`unsearched_turn`、ログの記録の型、延長モード、テスト |
| `parts.rs` | 4部品の trait と、やり取りする型 |
| `legacy.rs` | 今の動作を再現する4部品の実装 |
| `context.rs` | `GameContext`（一局の中で手番をまたいで持つ状態） |
| `journal.rs` | JSONL の書き出しと盤面の特徴量 |

エンジン（`deft-reversi-engine`）と、その公開 API は変えていない。

### 1.1 4部品

| 部品 | trait | 入力 | 出力 | 呼ばれる時 | 今の実装 |
|---|---|---|---|---|---|
| コスト予測器 | `CostModel::next` | `Observation`（終わった反復）、`GameContext` | `Option<CostEstimate>`（次の反復の時間。最後の反復なら None） | 反復が終わるたび | `LegacyCost`：遷移ごとの倍率（ENTRY、EXACT、1.25、直前の比） |
| 局面価値予測器 | `ValueModel::gain` | 同上 | `GainEstimate { expected_discs, instability }` | 反復が終わるたび | `LegacyValue`：不安定度（最善手が変わると1、確認ごとに半分）。`expected_discs` は None |
| 残り時間価値モデル | `ClockModel::hurdle` | `ClockState`、`GameContext` | `Hurdle { soft, hard, price }` | 手番の始め | `LegacyClock`：完全読みの予約、均等額、借り。`price` は None |
| 停止ポリシー | `StopPolicy::stop` | `Observation`、上の3つの出力 | 止めるか | 反復が終わるたび | `LegacyStop`：`経過 + 予測 > min(soft × (1 + 不安定度), hard)` |

コスト予測器と局面価値予測器は、探索ごとに新しく作る。探索の中の状態（直前の比、直前の最善手）はそれぞれが持つ。

### 1.2 やり取りする型

| 型 | フィールド | 意味 |
|---|---|---|
| `ClockState` | `empties`, `remaining`, `cap`, `protocol`（`Gtp` / `FlipSight`）, `threads` | 手番の始めの時計。`remaining` はマージンを引いた残り、`cap` はその手の上限（GTP では `remaining` と同じ、FlipSight では `budget_ms` 由来） |
| `Observation` | `empties`, `elapsed`, `cost`, `kind`, `next`, `best_move`, `score`, `nodes`, `pv` | 終わった反復。`cost` はその反復だけの時間 |
| `CostEstimate` | `median`（秒）, `log_sigma` | 次の反復の時間の対数正規分布。`p_finish(秒)` で、その時間内に終わる確率を返す。今の実装は `log_sigma = 0`（点推定。`p_finish` は0か1） |
| `GainEstimate` | `expected_discs`, `instability` | 次の反復の価値 |
| `Hurdle` | `soft`, `hard`, `price` | 手番の時間。`price` は「1秒が何石に値するか」を返すモデルのための欄 |
| `Transition` | `Mid`, `Entry`, `Stage`, `Exact`, `Back` | 遷移の種類 |

### 1.3 GameContext

毎手、残り時間から予算を計算し直す仕組みは変えていない。GameContext は、時計からは分からないことだけを手番をまたいで持つ。

| フィールド | 内容 |
|---|---|
| `game` | プロセスの中での対局の番号（0から） |
| `previous` | 前の自分の手番：局面、使用時間、最後に終わった反復、pv（最初が指した手） |
| `reply_matched` | 今の局面が、前の手番の pv[0]・pv[1] を指した後の局面と一致するか（相手が予想どおりに応じたか）。pv が2手未満なら None |
| `misses` | この局で終わった反復の ln(実際 / 予測) と遷移の種類（最大256件） |
| `extended` | この局で延長モードの手番があったか |

- 新しい対局の判定：GTP は `clear_board`（と `boardsize`）で初期化する。両プロトコルとも、空き数が前の手番以上になったら新しい対局とみなす（FlipSight には対局の区切りの合図がない）。
- GTP は1つのプロセスが黒と白の両方を指すことがあるので、色ごとに1つずつ持つ。
- 探索しない手番（合法手が1つ、定石、モデル読み込み中、`reflex`）も `unsearched_turn` で記録するので、`previous` は常に直前の自分の手番になる。

## 2. 今と同じ動作であることの確かめ方

ロードマップでは「既存の GTP ログを再生して判断の一致を確かめる」としていた。ただ、ログには反復ごとの最善手がなく、最善手が変わったときの延長を再現できない。そこで、次の差分テストに置き換えた（`the_parts_decide_exactly_like_the_previous_game_clock`）。

- `0c53a14` の `Forecast` と `Budget::new` を、倍率の定数も含めてテストの中にそのまま写した。
- 20,000回のランダムな探索で、新旧の止める判断がすべて一致することを確かめる。
  - 中盤の深さの並び（2手ずつ、ときどき1手・3手）と終盤のステージ。
  - 反復の時間は0秒、1マイクロ秒〜10秒。
  - 最善手はときどき変わり、ときどき None になる。
  - soft・hard はランダム。
- 予算の計算は、空き0〜60 × 200通りの残り時間・上限で一致を確かめる。
- テストが違いを検出できるかも確かめた。次の書き換えはすべてテストを失敗させた。
  - ENTRY の 5.0 → 5.01、EXACT の 3.0 → 3.01、ステージ間 1.25 → 1.26
  - 直前の比の上限 6.0 → 6.1、不安定度の減衰 0.5 → 0.6、予約の 6ms → 6.1ms
- `>` → `>=`（予測が目標にちょうど等しい場合）は検出できない。連続な時間では、ちょうど等しくなることがほぼないため。
- 既存の単体テストは、内部の型が変わった分だけ書き換え、同じ条件・同じ期待値で通る。

テスト：CLI 38件、すべて通過。clippy の新しい警告なし（既存の2件は変更前からのもの）。

## 3. JSONL のログ（`DEFT_CLOCK_LOG=FILE`）

GTP・FlipSight の持ち時間制の対局で、1行に1つの JSON を追記する。既存の `DEFT_GTP_LOG`（`audit.py` などが使う）は変えずに残した。同じファイルに複数のプロセスが書いてよい（`session` で区別する）。

### 3.1 反復の記録（`"record": "iteration"`）

| 項目 | 内容 |
|---|---|
| `session`, `game`, `empties` | プロセスID、対局の番号、空き数 |
| `kind`, `next` | 終わった反復と、次に行う予定の反復（`d22`、`e85`、`e100` の形。最後なら null） |
| `elapsed`, `cost` | 探索開始からの秒数、この反復だけの秒数 |
| `nodes`, `score`, `best_move`, `pv` | エンジンの報告そのまま（nodes は全スレッドの合計） |
| `predicted`, `log_sigma` | コスト予測器による次の反復の時間（それだけの時間。`elapsed + predicted` が予測完了時刻） |
| `p_finish_hard` | 次の反復が hard までに終わる確率（今は0か1） |
| `instability`, `expected_discs` | 局面価値予測器の出力 |
| `stop` | 停止ポリシーがここで止めると判断したか |
| `after_decision`, `beyond_hard` | 延長モードの手番だけ：止めると判断したあとの反復か、hard より後に終わったか |

### 3.2 手番の記録（`"record": "turn"`）

| 項目 | 内容 |
|---|---|
| `session`, `game`, `empties` | 同上 |
| `searched` | 時間を使って探索したか（合法手1つ・定石・読み込み中・`reflex` は false） |
| `mobility`, `opponent_mobility`, `regions`, `odd_regions` | 手番側と相手の合法手の数、空きマスの連結領域の数（上下左右のつながり）と、そのうち奇数の領域の数 |
| `protocol`, `threads`, `remaining`, `cap`, `soft`, `hard`, `price` | 時計と予算（予算を計算しなかった手番は null） |
| `used` | この手番の秒数（時間管理のコードの中で測ったもの） |
| `stop_reason`, `reached`, `pending`, `played`, `score` | 終わり方（`TimeLimit` は Forecast か hard で止めた、`Completed` は最後の反復まで終えた）、最後に終わった反復、次に予定していた反復、指した手、評価値 |
| `previous_empties`, `previous_used`, `previous_reached` | 前の自分の手番 |
| `reply_matched` | 相手が予想どおりに応じたか |
| `extended`, `after_extension`, `decision_elapsed`, `decision_reached` | 延長モードの情報（4章） |

### 3.3 動作確認

調整用の開局（番号0）で、GTP 2秒と FlipSight 2秒を1組ずつ指した。

- 各プロトコルで、4プロセス × 26手番の手番の記録と、反復の記録（GTP 216行、FlipSight 189行）が出た。
- 探索しない手番は各プロトコルで4。時間切れなし。

分かったこと：

- `reply_matched` は約半分の手番で null だった。探索で読み筋を取り直さない設定（`skip_pv_recovery`）のため、反復の約4割で pv が1手しかない（中盤 107/257、終盤 40/148）。
  - 取り直すと探索の時間が増えるので、ここは変えない。
  - 置換表が温まっているかの判別には、その手番の最初の反復の時間・ノード数も手がかりとして使える（ログに入っている）。
- ノード数がログに入るようになった（段階1では、probe のデータになかったので調べられなかった）。

## 4. 延長モード（`DEFT_CLOCK_EXTEND=EMPTIES:SECONDS`）

時計が止めると判断した後の反復の、本当の時間を取るためのデータ生成専用の動作。

- 各対局で、空き数が EMPTIES 以下になった最初の自分の手番だけが対象。その局で2回目はない（GameContext の `extended`）。
- その手番では、止めると判断しても探索を続け、SECONDS（hard の方が長ければ hard）まで反復を記録する。
- 指す手は、時計が止めていた場合に指した手にする。
  - 止めると判断した時点の最善手。
  - または、判断の前に hard を過ぎた場合は、hard の前に終わった最後の反復の手。
- 延長した分の時間と置換表は、その局の残りに影響する。以後の手番は `after_extension: true` と記録する。データとしては使わず、局はそこで打ち切る前提（ロードマップ 2-5 の方針）。

動作確認：GTP 2秒、`DEFT_CLOCK_EXTEND=34:2`。

- 時計は d15（0.05秒）で止めると判断し、その後 d25（1.93秒）まで反復を記録した。
- 指した手は判断した時点の手（h4）。
- 記録の例（34空き）：予測 0.151秒に対して、d17 の実際の時間は0.145秒。
- 延長した側は、2秒の持ち時間を使い切って時間切れで負けた。想定どおり。
- **段階3のデータ生成では、レフェリー（`tools/time-control/referee.py`）が延長した手番のあとで局を打ち切り、勝敗を数えないようにする必要がある（未対応）。**

テスト：

- 設定の文字列の読み取り（`26:10`、`30:0.5` は有効。`26`、`26:0`、`26:inf` などは無効）。
- 評価0のモデルでの実際の探索で、次を確かめる。
  - 判断後も探索が続くこと。
  - 判断の時刻が hard 以内であること。
  - 指す手が判断時の手であること。
  - 延長しない場合は判断した反復で止まること。

## 5. 残った課題

- レフェリーの延長モード対応（延長した手番のあとで局を打ち切る）。段階3の前に必要。
- `>` と `>=` の違いは差分テストで検出できない（影響は、予測がちょうど目標に等しい場合だけ）。
- `reply_matched` は約半分の手番で分からない。
- 価値予測器・残り時間価値モデルの新しい実装は、段階4〜5で入れる。
