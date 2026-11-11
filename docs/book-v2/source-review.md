# bookのソースを読む

対象は`deft-reversi-book`の現在の実装（work v8 / play v4）。この文書は、各処理の入口、共有する情報、変更時に守る条件、対応する試験を案内する。操作方法は[運用](operations.md)、値そのものの意味は[評価の意味](value-model.md)を参照する。

## 最初に把握する三つの型

|型|所有する情報|主な入口|
|---|---|---|
|`BookEditor`|局面、登録した進行、評価記録、生成の設定と再開する仕事|`record_search`、`register_move`、`generate`、`save/load`|
|`Book`|対局で読む局面・候補と、その採用値・探索条件|`BookEditor::export`で作り、`probe`で読む|
|`BookPlayer`|使用するBookへの参照、評価の利用条件、手を選ぶ乱数の状態|`decide`は着手または使えない理由、`choose`は着手またはNoneを返す|

BookEditorを変更しても、以前にexportしたBookは変わらない。対局側へ新しい内容を渡すには再度exportし、`BookPlayer::set_book`などで差し替える。Engineの再帰探索はbookを参照しない。bookの手が使えなければ、呼出し側が通常探索する。

## 読む順序とファイルの担当

まず[lib.rs](../../deft-reversi-book/src/lib.rs)の公開型と`combine_values`を読み、次の順で進む。コードの行番号は編集で変わるため、この文書では関数・型名を目印にする。

|順序|ソース|確認する内容|
|---|---|---|
|1|[evidence.rs](../../deft-reversi-book/src/evidence.rs)|`ValueClaim`、`ValueAnalysis`、`CompletedSearch`、`SearchProjection`。点数と探索条件の区別|
|2|[values.rs](../../deft-reversi-book/src/values.rs)|`ValueStore::intern/get`。評価・探索記録・モデル識別値の共有と参照番号|
|3|[editor.rs](../../deft-reversi-book/src/editor.rs)|`Node/Edge`、`evidence`、`refresh_from`。どの値を採用し、親へどう反映するか|
|4|[generate.rs](../../deft-reversi-book/src/generate.rs)|設定、仕事一覧、`candidate_plan`、`active_paths`、`expand`。探索条件と登録範囲|
|5|[grow.rs](../../deft-reversi-book/src/grow.rs)|`generate`、`prepare`、`publish`、`BatchPool::run`。生成を一巡する処理|
|6|[schedule.rs](../../deft-reversi-book/src/schedule.rs)|`priority_job`と`priority`。次に調べる局面の選択|
|7|[snapshot.rs](../../deft-reversi-book/src/snapshot.rs) / [player.rs](../../deft-reversi-book/src/player.rs)|`from_editor`→`probe`→`moves`→`choose`。対局用への変換と利用条件|
|8|[format.rs](../../deft-reversi-book/src/format.rs)|`save_editor/load_editor`、`save_book/load_book`。現行の保存と旧版の検査・移行|

補助ファイルの担当は次のとおり。

|ソース|担当|
|---|---|
|[catalog.rs](../../deft-reversi-book/src/catalog.rs)|モデル等の説明と、生成を一回実行した際の設定・使用資源・停止理由|
|[value_codec.rs](../../deft-reversi-book/src/value_codec.rs)|評価の部品のバイナリ表現、旧形式の評価の読書きとハッシュ|
|[symmetry.rs](../../deft-reversi-book/src/symmetry.rs)|盤面と着手の回転・反転、逆変換|
|[stats.rs](../../deft-reversi-book/src/stats.rs)|全局面を走査する明示的な診断。対局の通常参照には使わない|
|[bin/book.rs](../../deft-reversi-book/src/bin/book.rs)|引数、モデル・ファイルの読書き、Ctrl-C、定期保存。生成の本体はライブラリへ委譲|

## ソース内の名前と意味

|名前|この実装での意味と用途|
|---|---|
|`Key`|手番側と相手側の石を、回転・反転で同じ配置へそろえた局面。黒白の絶対色は含まない|
|`Context`|Keyと手番の絶対色の組。色別の損失条件で仕事を選ぶために使う。局面自体を二重に持つわけではない|
|`registered`|その親からその手をbookの進行として登録したか。子が別経路から存在するだけでは登録にならない|
|`Assessment`|値と探索条件をまとめた、検査済みの読取結果。内部の全候補がこの大きな型を直接持つわけではない|
|`ValueId`|BookEditor内の共有評価を指す番号。1始まりの4バイト。圧縮時に変わるため外部へ公開しない|
|`direct`|その手を独立に調べた評価。登録した子の値を採用しても、元の直接評価を消さない|
|`propagated` / `backups`|子の値を親の手番から見た値へ変換したもの。backupsは変換済みの値を再利用するための保持欄|
|`Frontier`|一回の対象手集合の探索結果。集合の最良手の点評価と、それ以外の手の上限を提供する|
|`effective`|直接評価・子の値・手動値などの中から、現在採用する値。`CandidateEvidence::source`で選択元を確認する|
|`certificate` / `proof`|最終石差について確定した点または上下限。certificateは候補の手、proofは局面全体。推定や手動指定とは別に保つ|
|`revision`|変更の世代番号。BookEditor全体の番号は到達条件のキャッシュを無効にする。Nodeの番号は直接記録・登録状態の編集を表し、探索開始後に変わればその仕事をやり直す|
|`stage`|利用者が設定したレベルまたは深さ系列の段階番号。探索の深さそのものではない|
|`TT`|探索中に調べた局面の結果を再利用する置換表。bookデータのメモリ予算とは別|

## 一つの評価はどこに置かれるか

局面Pの候補A・B・Cをまとめて探索し、Aが最良で+4だったとする。

1. `CompletedSearch`一件が、P、対象手集合、A、+4、要求・予定・完了した探索条件を持つ。
2. `StoredAnalysis::Search`が、この探索記録の番号と`SearchProjection`を持つ。集合最大値・Aの点評価・他手の上限という三つの使い方を区別する。
3. `StoredValue`が、点または上下限、説明の番号、モデル等の番号を持つ。
4. `Node.frontier`が必要な評価の番号を保持する。各候補は集合の中での位置に応じて点評価または上限を読む。BとCを+4の点評価として扱わない。

これにより探索記録の盤面や条件を候補ごとに丸ごと複製しない。ただし、更新中に同じ内容を探す`ValueStore`の索引はキーを別に持つ。また、`Assessment`を返す際、保存形式の組立てや旧形式の検査では、展開した値を一時的に持つ。複製が一切ない設計という意味ではない。

`CandidateNotes`は個別の直接評価・手動値・最終石差の情報がある手だけに確保する。手が登録済みであること、子のKey、合法手としての位置は`Edge`に持つ。

`BookEditor::compact_evidence`は、現在参照される評価だけを新しい一覧へ移し、全参照番号を付け替える。保持するValueIdを追加したら、この走査への追加が必要。`Book`への変換では未参照の値と更新用の索引を除き、`EvaluationRef`で借用して読む。

## 値を一つ変更したときの流れ

入口は`record_search`、`record_candidate`、`set_manual`、`register_move`など。これらを分けて、集合の探索結果、個別の手の評価、手動指定を混同しないようにしている。

1. 入力盤面・合法手・対象手集合・モデル識別値などを検査する。探索結果を直接渡す`record_search`は盤面と結果が正規化座標である必要がある。`record_candidate`は入力のboard/actionを正規化するが、評価内の対象盤面・着手は正規化済みのものを検査する。
2. `checkpoint`で変更前の対象局面を保持し、編集世代を進めて変更する。
3. `commit_checkpoint`→`refresh_from`へ進む。`reconcile_certificates`で最終石差の各情報の共通範囲を求め、`evidence`で候補の採用値を決める。
4. `calculate`→`combine_values`で局面の最大値を計算する。手動値と独立した確定情報は`calculate_proof`で別にも計算する。
5. 値・確定情報・全候補の評価状態が変われば、登録した親へ進む。`order`は子を先に処理し、パスによって空きマス数が同じになる場合も順序を決める。
6. 成功後に必要な仕事を再投入する。途中の祖先で確定情報が矛盾した場合は、`refresh_from`が更新した局面群を戻し、`commit_checkpoint`が編集元を戻す。新規の子の削除などは操作側の後始末も読む。

基本の採用は、現在の手動値、確定した直接値、登録した子の値、直接値の順。ただし最終石差の上下限や集合探索の扱いが加わるため、最終的な条件は`evidence`と[子からの更新](propagation.md)で確認する。深さの大きい推定を常に選ぶ規則ではない。

例えば子の手番から見て-4なら、親の手の値は+4。区間[−6,−2]なら親では[+2,+6]。パスでも符号を反転するが、石を置いていないため到達深さを増やさない。これらは`try_child_to_parent`と`QualitySummary::shifted`が担当する。

## 生成を一巡する流れ

公開入口`generate_with_checkpoint`は`grow::generate`へ進む。設定の入口は`configure_generation_roots`。設定し直すと、既存の評価は残して段階の完了印と仕事を組み直す。

1. Engineのモデル等が一致することを確認し、仕事一覧をBookEditorから一時的に取り出す。
2. 登録範囲を再確認する仕事`expansions`を処理し、探索待ちの`fresh/deepen`から次の仕事を選ぶ。優先順位は探索順を変えるだけで、手の登録は許可しない。
3. `prepare`は登録した子の評価を再利用し、未登録手や範囲境界で評価が不足した手を探索対象へまとめる。十分な点評価と上限があれば探索を省ける。
4. `BatchPool::run`で独立した局面を探索する。同じ局面の仕事は同一バッチに重ねない。全体のスレッド数・TT・残りノードを分け、探索元ごとに全予算を使わせない。
5. 完了結果だけを`publish`する。対象局面の編集世代が変わっていれば再投入する。中断結果は診断用の記録に残し、候補の完了値にはしない。
6. `expand_and_schedule`→`expand`で、現在の1手損失、累積損失、新規手のleaf_loss、手数、残り資源を確認して登録する。子から値が戻るたびに残り候補を再確認する。
7. 必要ならPV（探索が返す最良の進行）に沿う次の仕事を優先する。段階が完了したら次へ進む。終了時は取り出した未完の仕事を戻し、Engineの資源設定を復元する。

子の値が親へ戻っただけでは、親のNode.revisionは変えない。親の盤面と探索対象の手集合に対する独立した探索結果は引き続き使えるためである。結果の採用後、新しい手を登録する前には、変化後の局面値で損失条件を改めて確認する。

`candidate_plan`で現在のレベル指定を解決する。現状は候補を打った子でレベルを解決して親の深さへ戻している。将来の「中盤レベル＝同じ探索元での要求深さ」はまだ実装していない。[レベル](levels.md)を参照。

### 合流する進行と損失制限

同じ局面へ、手数が短く損失が大きい進行と、手数が長く損失が小さい進行が合流することがある。`distances`の最短手数だけでは新規登録の根拠にならない。

`active_paths`は現在の条件を満たす実在の進行を調べ、手数・累積損失の組を残す。`pareto`は両条件で劣る組だけを除く。片方の最小値だけを選んで、存在しない好都合な進行を作ってはいけない。

`analysis_distance`は、値が不明になった既存の枝を調べ直すために範囲を広げる判定。新規登録には`active_paths`の確認が別途必要となる。[損失制限](loss-control.md)も参照。

## 保存と再開で何が残るか

|対象|workでの扱い|主なコード|
|---|---|---|
|盤面・合法手・登録状態|保存する。子のKeyは着手から作り直す|`save_editor/load_editor`|
|直接評価・手動値・候補の確定情報・集合探索|共有一覧とその参照を保存する|`ValuePool::editor`、`ValueStore::write/read`|
|局面の現在値、親向けの値、親の逆引き|局面欄の値としては保存せず、読込後に再計算する。一時的な保存用評価一覧にはこれらの記録も含まれる|`repair`、`rebuild_parents`|
|開始局面・色・設定・段階・探索待ち・登録確認待ち・PVの優先仕事と手のヒント|保存する|`configw/configr`、`save_editor/load_editor`|
|優先順位の索引・到達条件のキャッシュ・登録確認済みの印|作り直す|`PriorityIndex`、`ReachabilityCache`、`ExpansionStamp`|
|モデル等の説明・実行の設定と停止理由|保存する|`catalogw/catalogr`|

playはその時点の採用値と確定情報を保持し、生成再開の仕事を持たない。`ValuePool`は保存・変換中の一時一覧であり、常駐する共有一覧`ValueStore`と同じものではない。内部ValueIdは1始まり、playの配列とファイルの値参照は0始まり。番号の変換箇所を飛ばして読まない。

`atomic_save`は同じディレクトリの一時ファイルへ本文を書き、長さとハッシュを付け、同期後に置換する。`open`は全体長とハッシュを先に確認し、各読込処理が件数・割当上限・盤面・参照先・評価の意味も検査する。ハッシュ一致だけで正しいbookとはしない。

定期保存は`grow::save`が仕事一覧をeditorへ戻してからコールバックを呼ぶ。コールバック失敗時も一覧を実行側へ戻す。エラー終了では最後の保存コールバックを保証しないため、呼出し側は必要に応じて終了後のeditorを明示的に保存する。

## 旧形式の処理を読むとき

現行の処理と混同しやすいものを挙げる。対応版の一覧と移行操作は[互換性](compatibility.md)を参照。

|残っている名前|残す理由と使用範囲|
|---|---|
|`PropagationPolicy::Quality`|旧workで何が採用されていたかを再現して比較するため。現在の編集・生成はBookに固定|
|`Derivation::Maximum/Child/Intersection`、`Catalog::derivations`|旧v7/v3の計算元の関係を検査するため。検査後に過去の入力関係を捨て、現在の対象を表すCurrentへ変換|
|`Pending`|旧方式に残った完了済みの候補結果を取り込むため。現在の生成器は新規作成せず、現行workの読込でも許可しない|
|`LegacyUnknown`|旧bookや外部入力に探索方法が残っていないことを表す。深さから中盤・終盤を推測して埋めない|
|`MigrationReport`|変換元の版、採用値の変更件数、仕事を作り直したかを利用者へ知らせる。対局の選択条件には使わない|

`load_editor`は旧形式を検査してから現在の値を再計算し、以前の採用値と比較する。採用規則や値が変わる場合は、完了した評価記録を残して開始局面・設定から仕事を作り直す。読み込むだけでは元ファイルを更新しない。

旧形式のハッシュは旧形式のバイト列から計算する。`value_codec::write/fingerprint`や`validate_catalog`の当時の解釈を、現行形式に合わせて不用意に書き換えない。

旧play v3で手動値と旧探索を合成した値は、当時の集計で手動値の表示深さが探索の最低深さへ混ざる場合がある。旧入力関係を検査した後、`rebuild_legacy_play_quality`で入力から順番に現在の品質を集計し直す。旧形式の検査を省いたり、現行v4の不一致を許可したりはしない。

## 対局の一手が選ばれるまで

`Book::probe`で正規化した局面を探し、`BookEntry::moves`で入力座標の着手と借用した評価を列挙する。評価内の盤面は正規化座標のまま。`candidates()`は詳細なAssessmentへ展開する診断用の入口。

`UsePolicy::check_parts`は、中盤・終盤・旧データ・手動値の条件を分けて確認する。子から計算した値に複数の探索方法が混ざる場合も、それぞれの最低条件を確認する。最終石差の確定情報と推定値のモデル互換性は同じ扱いにしない。

`player::choose`は、利用条件を満たす点評価と上限から基準点を求め、許容損失以内の登録手で点評価があるものを選ぶ。未登録手も基準点に関わる。例えば登録手A=+4、未登録手B<=+6なら、BestではAを選ぶ根拠が不足するため通常探索へ戻す。許容損失4ならAは基準+6から2以内で候補になる。

選んだ手だけ詳細を展開する。通常の手の選択で問い合わせごとのヒープ確保は行わない。

## 変更する箇所に対応する試験

最初は次の対応で読み、個々の期待値も確認する。テスト成功だけで設計の妥当性が確定するわけではない。

|確認する条件|試験の所在と目印|
|---|---|
|集合最大値・最良手・他手の上限が一件の探索を共有する|values.rs内の`projections_share_one_completed_search_and_survive_the_shared_codec`|
|局面全体と候補・一部の手集合を取り違えない|[value_contract.rs](../../deft-reversi-book/tests/value_contract.rs)の`a4_subset_search_cannot_be_recorded_as_a_full_position_or_excluded_move`|
|上限を各候補の点数にしない|[frontier.rs](../../deft-reversi-book/tests/frontier.rs)の`one_root_search_covers_unexpanded_moves_without_fabricating_point_values`|
|矛盾が祖先で見つかっても編集を戻す|value_contract.rsの`conflicts_during_ancestor_propagation_and_registration_roll_back_the_edit`|
|手動の採用値とは別に最終石差を残す|value_contract.rsの`independent_position_proof_survives_manual_book_values_and_propagates_separately`|
|子の値を、深い直接推定で上書きし直さない|[review_regressions.rs](../../deft-reversi-book/tests/review_regressions.rs)の`book_policy_prefers_actual_child_backup_over_estimated_frontier`|
|合流した進行の手数と損失を両方保つ|generate.rs内の`transposition_retains_short_costly_and_long_cheap_root_paths`|
|古い世代のバッチ結果を採用しない|grow.rs内の`stale_batch_result_is_requeued_without_publishing_or_advancing_stage`|
|中断・保存エラー後も再開できる|frontier.rsの`pv_growth_stop_and_checkpoint_error_leave_all_work_resumable`|
|旧形式の評価を残し、仕事を組み直す|format.rs内の`old_quality_work_rebuilds_jobs_without_losing_completed_search_records`、[simple_design.rs](../../deft-reversi-book/tests/simple_design.rs)、[固定入力の説明](../../deft-reversi-book/tests/fixtures/README.md)|
|手動値を含む旧play v3の品質を、旧情報の検査後に集計し直す|format.rs内の`old_play_with_manual_and_legacy_searches_rebuilds_quality_after_validation`|
|通常の対局参照でメモリ確保しない|[book_alloc.rs](../../deft-reversi-book/tests/book_alloc.rs)の`book_hit_performs_no_heap_allocations`|

リポジトリのルートで、関心のある名前を末尾に付ければ絞って実行できる。

```sh
cargo test --offline --locked --release -p deft_reversi_book --features book-cli
cargo doc --offline --locked -p deft_reversi_book --all-features --no-deps --document-private-items
python3 tools/render_book_docs.py --check
```

Rustの型・関数をたどるHTMLは`target/doc/deft_reversi_book/index.html`へ生成される。日本語ガイドのHTMLは`tools/requirements-book-docs.txt`の依存を入れた環境で`tools/render_book_docs.py`を実行して更新する。

実book・メモリ・速度の測定範囲と未検証の事項は[検証](validation.md)に分けている。今回のコメント・文書整備は処理を変更するものではなく、それらの性能測定を再実施したという意味ではない。
