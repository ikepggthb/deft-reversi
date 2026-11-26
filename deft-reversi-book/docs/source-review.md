# ソースの読み方

最初から内部の型をすべて覚える必要はない。確認したい操作の入口から順に追うと理解しやすい。現在の評価は `Evaluation` 一つで扱い、点数・探索条件・出所を直接読める。旧形式の検査は `legacy` モジュールへ分けている。

## 1. 評価を読む

対局用データから評価を読むときは `Book::probe` で局面を探し、`entry.moves()` で候補手を列挙する。各候補の `value`（`EvaluationRef`）が評価を表す。

| 知りたいこと | 読む箇所 |
|---|---|
| 点数と「点・下限・上限」の区別 | `value.result()`（`Score::Estimate` は推定、`Score::Final` は最終石差の範囲） |
| 中盤の深さ、終盤の選択率、混在する条件 | `value.conditions()` |
| 直接探索・子からの計算・手動値の区別 | `value.source()` |
| 実際に完了した 1 回の探索の詳細 | 出所が `Search` または `SearchMove` なら、その中の `CompletedSearch` |

たとえば「この手は +4 以下」という上限情報は、+4 の点評価ではない。着手に使える一点の値を読むときは `value.result().point_value()` を確認する。完全読み切りで石差が一点に確定したかは `is_solved()` で確認する。

通常の対局参照はコピーを避けるため借用した評価（`EvaluationRef`）を返す。`to_owned()` で独立した `Evaluation` を作れるが、点数・探索条件・出所の読み方は同じである。内部の共有番号を呼び出し側で意識する必要はない。

関連ファイル: [evaluation.rs](../src/evaluation.rs)（評価の型）、[score.rs](../src/score.rs)（点数と上下限）、[conditions.rs](../src/conditions.rs)（探索条件）、[search_record.rs](../src/search_record.rs)（探索記録）

## 2. 評価を追加する・子の値を親へ反映する

編集の入口は [editor.rs](../src/editor.rs) にある。

| 操作 | 入口のメソッド |
|---|---|
| 1 回の探索結果を取り込む | `record_search` |
| 1 つの手の評価を取り込む | `record_candidate` |
| 手動値を設定する | `set_manual` / `record_manual` |
| その手を Book の進行として登録する | `register_move` |
| 登録を外す | `unregister_move` |

変更後は `refresh_from` が子から親の順に再計算する。ある局面へ複数の進行が合流する場合も、影響するすべての親を更新する。

候補手の値を集める処理は `move_values_at`、その中から採用する値を選ぶ処理は `MoveValues::choose` が担う。採用規則だけを確認したい場合は、内部番号や盤面の検索処理を飛ばして `choose` だけを読める。採用の優先順は次のとおり。

1. 現在のモデル識別値に対応する有効な手動値があれば、それを使う。
2. 最終石差の確定情報がある場合は、未確定の推定で置き換えない。
3. 登録した子局面に値があれば、その値を親の手番へ変換して使う。
4. 子の値がなければ、その手を直接調べた値を使う。
5. 複数手をまとめて調べた結果（`MoveSearch`）も確認する。最良手以外には上限だけを与える。なお、子から戻した推定値を、以前の深い直接推定へ戻すことはしない。

手動値とは独立して、完全読み切りで得た最終石差の情報も保持する。確定情報の共通範囲が空になるような矛盾する変更は拒否し、編集前の状態へ戻す。最終石差の統合は `merge_final_scores`、局面全体の最大値は [lib.rs](../src/lib.rs) の `combine_values` が担当する。

子から親への変換は `Evaluation::try_child_to_parent` にある。子の手番から見て -4 なら親の手の値は +4、子の最終石差の範囲が `[-6, -2]` なら親では `[+2, +6]` になる。パスでも符号は反転するが、石を置いていないため深さは増やさない。

## 3. Book を生成する

生成の設定と登録範囲の判定は [generate.rs](../src/generate.rs)、探索のメインループは [grow.rs](../src/grow.rs) にある。

1. `configure_generation_roots` で開始局面・手番の色・設定を決める。
2. `generate` が次の仕事を取り出し、`prepare` が調べる手を決める。
3. `BatchPool::run` が探索を実行し、`publish` が完了結果を反映する。
4. `expand` が現在の損失・手数・予算を確認して手を登録する。
5. 次の段階や子の仕事を追加する。停止時は未完の仕事を探索待ちへ戻す。

「局面が存在する」ことと「その親から進む手として登録されている」ことは別であり、`Edge.registered` が後者を表す。

1 回の集合探索の結果は `MoveSearch` に保持する。最良手の点評価と、それ以外の手の上限を同じ探索結果から読み取る。探索範囲に含めなかった手を評価済みにしたり、一部の手だけの最大値を局面全体の値にしたりはしない。

`candidate_plan` はレベル・深さ指定を `Engine` の探索条件へ変換する。開始局面から到達できるかは `active_paths` で判定する。同じ局面へ「短いが損失の大きい進行」と「長いが損失の小さい進行」が合流し得るため、最短距離だけでは新規登録を許可しない。`analysis_distance` は既存の未評価手を調べ直すための判定であり、新規登録の根拠には使わない。

[schedule.rs](../src/schedule.rs) は探索の優先順だけを変える。並列探索では全体のスレッド数・置換表・残りノード数を仕事へ分配する。

## 4. 保存して再開する

[format.rs](../src/format.rs) の `save_editor` / `load_editor` が `.dbook-work` 用、`save_book` / `load_book` が `.dbook` 用である。書き込む形式は通常の表現で収まる場合が work v8 / play v4、集合探索の上界を含む場合が work v9 / play v5 となる。

| `.dbook-work` に保存するもの | 読み込んだ後にメモリ上で作るもの |
|---|---|
| 盤面、登録した手、直接評価、手動値、最終石差の情報、集合探索 | 子の盤面、親の逆引き、現在の局面値と親へ戻す値 |
| 開始局面、色、設定、完了段階、未完の仕事 | 探索の優先順位の索引、到達条件のキャッシュ |
| モデル等の説明、実行条件と停止理由 | 保存に必要のない補助索引 |

定期保存は `grow::save` が未完の仕事一覧を `BookEditor` へ戻してからコールバックを呼ぶ。`atomic_save` は一時ファイルへ書いて同期した後に置換するため、読み込むだけで元のファイルを更新することはない。

旧形式の「導出」「投影」「旧採用方式」といった表現は [legacy](../src/legacy/mod.rs) 内で検査・変換する。現行の局面データには旧方式の途中状態を残さず、読み込み中に完了した候補だけを取り込む。旧形式を触らないレビューでは、この内部表現を追う必要はない。

## 5. メモリ配置・対局・統計

評価の共有とメモリ配置は [values.rs](../src/values.rs) が担当する。局面や手は 4 バイトの内部番号（`ValueId`）を持ち、記録一覧で点数・出所・探索記録・モデル識別値を共有する。この番号は外部へ公開しない。`compact_evidence` で不要な記録を除くときに全参照を付け替えるため、新しい参照欄を追加した場合はこの走査も修正する。

対局用スナップショットへの変換は [snapshot.rs](../src/snapshot.rs)、使用条件と着手の選択は [player.rs](../src/player.rs)、統計のための全走査は [stats.rs](../src/stats.rs) に分かれている。

## 6. テストの対応

変更した箇所に応じて次のテストを確認する。

| 変更対象 | 確認するテスト |
|---|---|
| 評価の対象範囲、推定と最終石差の区別 | `evaluation.rs` 内、[value_contract.rs](../tests/value_contract.rs) |
| 子からの反映、手動値、矛盾時の復元 | [review_regressions.rs](../tests/review_regressions.rs)、`value_contract.rs` |
| 集合探索、並列実行、中断と再開 | [frontier.rs](../tests/frontier.rs)、`grow.rs` 内 |
| 損失・手数、合流する進行 | `generate.rs` 内、[book.rs](../tests/book.rs) |
| 現行形式の保存往復 | [current_format.rs](../tests/current_format.rs) |
| 旧形式の読み込み | [simple_design.rs](../tests/simple_design.rs)、`format.rs` 内、[fixtures/README.md](../tests/fixtures/README.md) |
| 対局の通常参照でメモリ確保しないこと | [book_alloc.rs](../tests/book_alloc.rs) |

テストとドキュメント生成の実行コマンドは次のとおり。

```sh
cargo test --offline --locked --release -p deft_reversi_book
cargo doc --offline --locked -p deft_reversi_book --all-features --no-deps --document-private-items
```

設計整理の背景は [保守性の見直し](maintainability.md) にまとめている。
