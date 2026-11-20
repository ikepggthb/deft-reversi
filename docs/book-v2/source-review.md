# bookのソースを読む

最初から内部の型を全部覚える必要はない。まず、確認したい操作から読む。現在の評価は`Evaluation`一つで扱い、点数・探索条件・出所を直接読む。旧形式の検査は別の場所へ分けている。

## 評価を読む

`Book::probe`で局面を探し、`entry.moves()`で手を列挙する。それぞれの`value`が評価である。

|知りたいこと|読む箇所|
|---|---|
|点数と、点・下限・上限の区別|`value.result()`。`Score::Estimate`は推定、`Score::Final`は最終石差の範囲|
|中盤の深さ、終盤の選択率、混在する条件|`value.conditions()`|
|直接探索したのか、子から計算したのか、手動なのか|`value.source()`|
|実際に完了した一回の探索の詳細|出所が`Search`または`SearchMove`なら、その`CompletedSearch`|

例えば「この手は+4以下」という情報は+4の点評価ではない。着手に使える一点の値を読むなら`value.result().point_value()`を確認する。完全読み切りで石差が一点に確定したかは`is_solved()`で確認する。

通常の対局参照は借用した評価を返す。`to_owned()`で独立した`Evaluation`にできるが、点数・探索条件・出所の読み方は同じである。共有するための番号を呼出し側で扱う必要はない。

[評価を表す型](../../deft-reversi-book/src/evaluation.rs)、[点数と上下限](../../deft-reversi-book/src/score.rs)、[探索条件](../../deft-reversi-book/src/conditions.rs)を参照する。実際に探索した条件の記録は[search_record.rs](../../deft-reversi-book/src/search_record.rs)にある。

## 評価を追加する・登録した子の値を反映する

[editor.rs](../../deft-reversi-book/src/editor.rs)の次の入口から読む。

|操作|入口|
|---|---|
|一回の探索結果を取り込む|`record_search`|
|一つの手の評価を取り込む|`record_candidate`|
|手動値を設定する|`set_manual`|
|その手をbookの進行として登録する|`register_move`|
|登録を外す|`unregister_move`|

変更後は`refresh_from`が子から親の順に再計算する。ある局面へ複数の進行が合流する場合も、影響する親を更新する。

値の候補を集めるのが`move_values_at`、その中から採用する値を選ぶのが`MoveValues::choose`。採用規則だけを確認する場合は、共有番号や盤面の検索処理を飛ばして`choose`を読める。

1. 現在のモデルに対応する手動値があれば、それを使う。
2. 最終石差について確定した情報があれば、それを推定で置き換えない。
3. 登録した子に値があれば、その値を親の手番へ変換して使う。
4. 子の値がなければ、その手を直接調べた値を使う。
5. 手をまとめて調べた結果も確認する。最良手以外には上限だけを与える。子から戻した推定を、以前の深い直接推定へ戻さない。

手動値とは独立して、完全読み切りで得た最終石差の情報を保持する。確定情報の共通範囲が空になる変更は拒否し、編集前へ戻す。最終石差の統合は`merge_final_scores`、局面全体の最大値は[lib.rs](../../deft-reversi-book/src/lib.rs)の`combine_values`が担当する。

例として、子の手番から見て-4なら親の手の値は+4。子の最終石差の範囲が[-6,-2]なら親では[+2,+6]。パスでも符号は変えるが、石を置いていないので深さは増やさない。変換は`Evaluation::try_child_to_parent`にある。

## bookを生成する

設定と登録範囲は[generate.rs](../../deft-reversi-book/src/generate.rs)、探索を一巡する処理は[grow.rs](../../deft-reversi-book/src/grow.rs)を読む。

1. `configure_generation_roots`で開始局面・手番の色・設定を決める。
2. `generate`が次の仕事を取り出し、`prepare`が調べる手を決める。
3. `BatchPool::run`が探索を実行し、`publish`が完了結果を反映する。
4. `expand`が現在の損失・手数・予算を確認して手を登録する。
5. 次の段階や子の仕事を追加する。停止時は未完の仕事を探索待ちへ戻す。

「局面が存在する」と「その親から進む手として登録されている」は別である。`Edge.registered`が後者を表す。

一回の集合探索は`MoveSearch`に保持する。最良手の点評価と、それ以外の手の上限を同じ探索から読む。探索範囲に含まない手を評価済みにしたり、一部の手の最大値を局面全体の値にしたりしない。

`candidate_plan`はレベル・深さ指定をEngineの条件へ変換する。現在の子基準のレベル解釈は維持しており、将来の「同じ探索元で中盤レベル＝深さ」へは変更していない。

開始局面から到達できるかは`active_paths`で判定する。同じ局面へ短いが損失の大きい進行と、長いが損失の小さい進行が合流し得るため、最短距離だけでは新規登録を許可しない。`analysis_distance`は既存の不明値を調べ直すための判定であり、新規登録の根拠には使わない。

[優先順位](../../deft-reversi-book/src/schedule.rs)は探索の順序だけを変える。並列探索では全体のスレッド数・置換表・残りノードを仕事へ分配する。

## 保存して再開する

[format.rs](../../deft-reversi-book/src/format.rs)の`save_editor/load_editor`が生成再開用、`save_book/load_book`が対局用。書き込む形式はwork v8 / play v4で変更していない。

|生成再開用に保存するもの|読み込んだ後で作るもの|
|---|---|
|盤面、登録した手、直接評価、手動値、最終石差の情報、集合探索|子の盤面、親の逆引き、現在の局面値と親へ戻す値|
|開始局面、色、設定、完了段階、未完の仕事|探索の優先順位の索引、到達条件のキャッシュ|
|モデル等の説明、実行条件と停止理由|保存に必要のない補助索引|

定期保存は`grow::save`が仕事一覧をeditorへ戻してからコールバックを呼ぶ。`atomic_save`は一時ファイルへ書いて同期後に置換する。読み込むだけでは元ファイルを更新しない。

旧形式の「導出」「投影」「旧採用方式」といった表現は[legacy](../../deft-reversi-book/src/legacy/mod.rs)内で検査・変換する。現行の局面データには旧方式の途中状態を保持せず、読込中に完了した候補だけを取り込む。旧形式を変更しないレビューでは、この内部表現を追う必要はない。

## 共有する保存方法を変更する場合

ここまでの値の意味とは別に、[values.rs](../../deft-reversi-book/src/values.rs)がメモリ上の共有配置を担当する。局面や手は4バイトの参照番号を持つ。記録一覧は点数・出所・探索記録・モデル識別値を共有する。

番号は外部へ公開しない。`compact_evidence`で使わない記録を除くときに全参照を付け替える。新しい参照の保持欄を追加した場合は、この走査も修正する。

対局用への変換は[snapshot.rs](../../deft-reversi-book/src/snapshot.rs)、使用条件と着手の選択は[player.rs](../../deft-reversi-book/src/player.rs)。統計のための全走査は[stats.rs](../../deft-reversi-book/src/stats.rs)に分けている。

## 確認する試験

|変更するもの|読む試験|
|---|---|
|評価の対象範囲と、推定・最終石差の区別|evaluation.rs内、[value_contract.rs](../../deft-reversi-book/tests/value_contract.rs)|
|子からの反映、手動値、矛盾時の復元|[review_regressions.rs](../../deft-reversi-book/tests/review_regressions.rs)、value_contract.rs|
|集合探索、並列・中断・再開|[frontier.rs](../../deft-reversi-book/tests/frontier.rs)、grow.rs内|
|損失・手数と、合流する進行|generate.rs内、[book.rs](../../deft-reversi-book/tests/book.rs)|
|既存のv8/v4をそのまま保存できること|[current_format.rs](../../deft-reversi-book/tests/current_format.rs)|
|旧形式の読込|[simple_design.rs](../../deft-reversi-book/tests/simple_design.rs)、format.rs内、[固定入力の説明](../../deft-reversi-book/tests/fixtures/README.md)|
|対局の通常参照でメモリ確保しないこと|[book_alloc.rs](../../deft-reversi-book/tests/book_alloc.rs)|

```sh
cargo test --offline --locked --release -p deft_reversi_book --features book-cli
cargo doc --offline --locked -p deft_reversi_book --all-features --no-deps --document-private-items
python3 tools/render_book_docs.py --check
```

[今回の見直し](maintainability.md)には、何を統合・削除し、どの点に複雑さが残るかを記録している。
