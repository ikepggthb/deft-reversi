# 対局用book

`deft_reversi_book` に、参照・生成・強化・保存再開・対局利用を実装した。既存の `Engine::search` / `solve` はbookを参照しない。

エンジンとは別クレート。依存は `deft_reversi_book → deft_reversi_engine` の一方向で、エンジンはbookやSHA-256・シグナル処理の依存を持たない。保存形式と解析識別情報は分離前と互換。

## 構成

- **Book**：読取専用の対局用スナップショット。連続した局面・候補配列と、負荷率70%以下の固定ハッシュ表を持つ。ハッシュの上位ビットで索引し、完全な盤面を照合する。複数の対局から `Arc<Book>` を共有できる。
- **BookEditor**：編集用の正本。正規化した局面グラフ、直接探索値、手動由来の値、親への逆リンク、未完了の解析を持つ。正本は安定順序のBTreeMap、親の値は依存順で再計算する。
- **生成モジュール**：Editorと既存Engineを借用し、候補解析、展開、深掘り、予算、停止とチェックポイントを担当する。外側の局面処理は逐次。探索内部のスレッド数はEngineOptionsで指定する。
- **BookPlayer**：Engineと任意のbookを所有し、book選択と通常探索への移行を行う。専用のExecutorや汎用ジョブ基盤は作っていない。

実装は、型・対称変換・編集・生成・保存形式・参照配列・対局選択に分けた内部モジュールになっている。これらを独立した管理オブジェクトとして組み合わせる必要はない。

## 局面・評価・品質

局面キーは手番側と相手側の完全な2枚のビットボード。既存Boardと同じ8対称形・辞書順で正規化する。着手にも変換を適用し、参照時に元の座標へ戻す。パスは明示的なActionで、深さ・着手数を減らさない。bookは通常の4石以上の局面を扱う。

`Assessment` は親手番側からの石単位スコア、品質、境界、解析識別情報、由来を持つ。品質の深さは親局面からの有効な解析範囲であり、完全読みの場合は終局までの証明を示す。固定深さの `Bound::Exact` と、ゲームの最終石差が確定した `quality.solved` は異なる。

- `record(board, action, assessment)` は、その1候補の解析を登録する。登録済みの分岐にするには `register_move` も呼ぶ。
- 未評価はNone。上下限だけの候補は対局の選択に使わない。
- 直接探索した値と手動値は別に保持する。現在の識別情報に一致する手動値は有効値を優先するが、完全読みの証明にはしない。生成しても手動値を上書きしない。
- 子からの伝播値は、登録されたリンクに沿って計算する。直接探索した値以上の深さ・選択率を持つ場合に採用し、複数の親へ伝播する。
- 全合法手が有効な点評価を持つ場合に局面の評価を計算する。完全読み済みへの昇格は、全合法手の値が確定した場合だけ。初版では上下限による追加の証明は行わない。
- 低水準のrecordで入力する品質・値の根拠は呼出し元の責任。読込は形式・合法性・メタデータの整合性を検証するが、保存された全スコアを再探索して証明するものではない。

解析識別情報は、実際のモデル重み、展開済みの校正ルール、解析規約の版、実験用手順付けfeatureをSHA-256に含める。ファイル名やモデルの表示名には依存しない。Engineのbook用識別処理は参照・生成を準備する際だけ実行し、探索の再帰には入らない。

Editorの `retarget(&Engine)` は、互換でない推定の直接値を無効化し、完全読み済みの値と元の手動記録を保持する。旧モデルに紐づく手動値は保持するが自動では利用しない。生成を再設定して解析し直す。互換性のない結果を同一の解析として混ぜない。

## 対局用API

```rust,no_run
use deft_reversi_engine::{Board, Engine, EngineOptions, SearchRequest};
use deft_reversi_book::{Book, BookPlayer, PlaySource, ReadLimits, UsePolicy};
use std::sync::Arc;

let mut engine = Engine::from_file("model.dpr2", EngineOptions::default())?;
// main由来の変換モデルに対応する校正を使う場合、接続前に指定する。
engine.set_legacy_main_mpc()?;
let book = Arc::new(Book::load("openings.dbook", ReadLimits::default())?);
let mut player = BookPlayer::new(engine, Some(book), UsePolicy::default(), 42)?;
let result = player.choose_move(SearchRequest::for_level(Board::new(), 20))?;
match result.source {
    PlaySource::Book(choice) => println!("book: {:?} {}", choice.action, choice.value.score),
    PlaySource::Search(search) => println!("search: {:?} {:?}", search.action, search.score),
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`choose_move` に渡すSearchRequestは、book不使用時の**フォールバック探索**を指定する。常に完全読みを実行したい用途は既存のEngine::searchを使う。book結果を架空のSearchResultへ変換しない。

既定のUsePolicyは次のとおり。

| 設定 | 既定 |
|---|---|
| 着手 | Best：登録候補中の最高評価。同点は正規化座標順 |
| 必要品質 | 深さ12以上、選択率95%以上。証明済みの完全読みは別扱い |
| 疎book | 利用する。全合法手の評価済みを要求しない |
| 未展開候補 | 参照できるが、着手には選ばない |

`require_all_evaluated=true` では、全合法手が設定品質を満たす場合だけ使う。`BookEntry::all_moves_evaluated` は保存された点評価の網羅状況を示し、任意のUsePolicyを満たす保証ではない。BookCandidateから候補ごとの品質を取得できる。

`MovePolicy::RandomWithinLoss { max_loss }` は、適格な登録候補の最高評価から指定差以内の手を均等に選ぶ。固定のSplitMix64と棄却法を使い、同じbook・設定・seed・問い合わせ順で再現できる。生成時の展開幅とは独立している。

Playerは停止済み・時間切れの要求をbookで迂回しない。探索へ移るときは参照に使った時間を差し引く。book利用自体は探索ノードを使わない。モデルを差し替える場合は `replace_engine` を使い、識別情報を再計算する。Engineを直接変更できる可変参照は公開しない。

Book::probeは借用ビューを返し、正規化・座標復元・候補選択で問い合わせごとのヒープ確保やファイルI/Oはしない。候補の順序は正規化座標順であり、入力盤面の座標順ではない。

## 生成と再開

`BookEditor::for_engine` → `configure_generation_roots` → `generate` または `generate_with_checkpoint` を使う。開始局面は `GenerationRoot { board, side: BookSide::Black/White }` で絶対手番色を指定する。簡易API `configure_generation(&[Board], ...)` は全開始局面の手番を黒とする。

- 各合法手の子を既存Engineで個別に検索する。親深さdの着手では子にd−1、パスではdを指定する。子の残り空き数まで調べる場合はExactへ切り替える。
- 同じ段階の全候補が完了してから、その段階を公開する。途中の完了候補はpending領域に保持して保存できる。
- 予算切れや停止の結果で、以前の完成済み値を上書きしない。より強い既存値も弱い結果で上書きしない。設定変更で深さと選択率が比較不能になる場合は、双方の強い側を満たす条件で再解析する。
- 既定は新規解析3件：深掘り1件の比率。重みは変更でき、両方0にはできない。新規は浅い局面、深掘りは浅い解析段階から進める。同順位は正規化キー順。片方の仕事がなければもう片方を進める。
- 有力な分岐を展開し、評価変更で親の拡張資格も再判定する。許容幅を外れた既存リンクは保持するが、自動強化では現在の有力分岐を通って到達できる局面だけを処理する。再び有力になれば未完了の強化を再開する。
- 開始地点・絶対手番色・距離とキューは生成状態に保持する。黒白それぞれの展開幅を設定でき、パスでも色を切り替える。同一局面に異なる色の文脈から到達した場合は、解析値を共有しつつ到達条件を別々に判定する。キューの距離は候補に使い、実行時には現在有効な経路と実際の距離を再確認する。累積損失などの条件は将来この文脈を拡張して扱う。

| GenerationConfig | 既定 |
|---|---|
| stages | 親深さ12→16→20、選択率95% |
| expansion_loss | 双方とも2石。black_loss / white_lossで色別に上書き可能 |
| new_job_weight / deepen_job_weight | 3 / 1 |
| max_plies | 各開始局面から24着手。パスは含めない |
| max_memory_bytes | book管理領域の見積もり512 MiB |
| checkpoint_interval | 60秒 |
| reuse_completed | 同等以上の完了解析を再利用する |

時間・探索ノード・追加局面数のいずれかの予算が必須。追加局面数はconfigureで作成した開始局面を除き、generate呼出し中に新規登録した局面の数。既存局面の再解析時間まで制限したい場合は、時間・ノード予算も指定する。

ノード予算は個々のEngine要求へ残量を渡し、各要求の実ノード数を合算する。メモリ制限はEditorとキューの保守的な管理量見積もりで、プロセスRSSのハード制限ではない。Engine、スナップショットの出力先、入出力バッファは別に扱う。

チェックポイントのコールバックは候補解析の終了境界と正常停止時に呼ぶ。長い候補解析を途中で周期ごとに打ち切ることはしないため、保存間隔は厳密な上限ではない。Ctrl-CはStopHandleへ伝え、実行中の探索が停止した後で保存する。再開では新しいStopHandleを使う。

## CLI

```sh
cargo build --locked --release -p deft_reversi_book --features book-cli --bin book

# 小規模の動作確認用。強いbookの既定解析より浅く設定している。
target/release/book build --model MODEL --work small.dbook-work --book small.dbook \
  --stages 4,6 --selectivity 95 --max-plies 2 --seconds 30 --new-positions 100

# 保存された設定・解析途中の結果をそのまま再開する。
target/release/book resume --model MODEL --work small.dbook-work --book small.dbook --seconds 30

# 設定を変えて拡張する。互換で十分な完了解析は再利用する。
target/release/book extend --model MODEL --work small.dbook-work \
  --stages 12,16,20 --max-plies 24 --seconds 3600

# 指定段階を再計算する。モデル変更時は推定値を無効化して解析し直す。
target/release/book reanalyze --model NEW_MODEL --work small.dbook-work --seconds 3600

target/release/book export --work small.dbook-work --book small.dbook
target/release/book inspect --work small.dbook-work
target/release/book verify --book small.dbook
```

main由来DPR2に対応する校正を使う場合は、生成系コマンドの各呼出しで `--legacy-main-mpc` を付ける。通常の統合モデルでは埋込MPCを使う。モデルの指定だけでmain用校正を推測しない。

- `--positions FILE.obf` で任意の開始局面を指定できる。OBFのX/Oで絶対手番色も指定する。指定しないbuildは標準初期局面・黒番から。
- `--loss` は共通の展開許容差。`--black-loss` / `--white-loss` で上書きできる。`--new-weight` / `--deepen-weight` で新規解析と深掘りの比率を指定する。
- extend/reanalyzeは明示的な再設定で、段階途中のpendingは破棄する。完成済み値は保持する。途中候補も使って続けたい場合はresumeを使う。
- resumeは設定変更を拒否する。Engineのスレッド数・TTメモリと実行予算は変更できる。
- reanalyzeでモデルを変更しても、元の開始局面・生成設定を引き継ぐ。
- `--threads`、`--tt-mib`、`--memory-mib`、`--checkpoint-seconds` も指定できる。
- 読込制限は `--read-mib`（既定512）、`--read-positions`（200万）、`--read-candidates`（3200万）。大規模運用では生成側・読込側双方の制限を調整する。
- workとbookの出力先を同じファイルには指定できない。buildは既存workファイルの上書きを拒否する。再開や更新は対応する別コマンドを使う。

## 形式と更新

`.dbook` と `.dbook-work` はそれぞれ8バイトのマジックを持つ。版・ルール番号はu32、ペイロード長はu64、SHA-256を付ける。数値はlittle-endian。モデル識別情報、生成元のパッケージ版、件数を保持する。

レコードの内部配列配置をそのまま保存しない。読込ではファイル長、チェックサム、件数、割当制限、盤面、合法手、パス、重複、リンク、品質、段階・キューを検査する。workの伝播値・逆リンクは正本から再構築する。

末尾の長さ付き拡張セクションには、種類・長さ・データを持たせる。未知の必須セクションは拒否する。未知の任意セクションは無編集の読込・保存で保持するが利用しない。編集や生成の変更時は、意味の分からない派生情報を残さないよう破棄する。新たな必須の意味・レコード構造の変更には形式版の更新と変換を用意する。

保存は同じディレクトリの排他的な一時ファイルへストリーム出力し、flush・sync後にrenameする。Unixでは親ディレクトリもsyncする。途中の失敗では既存ファイルを保持する。再開用ファイルと対局用ファイルは別々に原子的に更新し、両方を一つのトランザクションとして扱わない。

## 今回の範囲と検証

自動生成、段階的な強化、候補解析の公開、親への伝播、参照、着手選択、保存再開、CLIを初版として実装した。結合、任意の枝刈り・部分抽出、棋譜処理、ライン数・精度分類・うみがめ値、外側の局面並列は後続機能。GUI・他エンジン形式互換は対象外。

小終盤の完全列挙、8対称形、強制パス、転置、品質不足、モデル交換、手動値の保持、段階更新、途中停止と再開、保存失敗、破損・意味的に不正なファイル、任意拡張セクション、CLI一巡をテストする。bookヒット経路は専用のアロケーター試験で確保0回を検証する。

大規模bookはこの環境で生成しない。1000個の小さい局面を登録して参照・保存往復を確認する試験と、別環境での実モデル生成・メモリ・読込時間・参照p50/p95/p99測定を区別する。bookの強さやElo向上は、十分な生成と対局試験を行うまで保証しない。

検証結果と小さな確認用データは [2026-10-29の検証記録](benchmarks/2026-10-29-book/README.md) に保存した。既存のストレス試験を含む全体試験に加え、最後の生成品質の修正後にbook関連の試験を両feature構成で再実行している。

## 別クレート化の確認

`deft-reversi-book` への分離後、全feature構成でbookの29試験と既存エンジンの64試験が成功。残る重い完全列挙の1試験はdebug実行を中断し、releaseで成功を確認した（合計94成功、既存のignoredストレス試験1件は今回対象外）。全workspace・全target・全featureのClippyも警告なし。エンジン単体の依存からsha2・ctrlcが外れたことを確認した。

分離前の小規模 `.dbook` を新CLIで検証し、実モデルで保存した `.dbook-work` のコピーをノード予算0で再開した。解析識別情報が一致し、再保存も元ファイルとバイト一致した。
