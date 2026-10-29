# book実装の検証（2026-10-29）

参照・生成・再開・品質更新の正しさと、既存探索への影響を確認した。大規模bookや対局強度の検証は行っていない。

環境：Intel Xeon Platinum 8573C / x86_64、Rust 1.99.1、release、`RUSTFLAGS='-C target-cpu=native'`。比較元は `527c906c75bb87f8f9eae0225a451193820069c1`。

## 自動試験

| 対象 | 結果 | 記録 |
|---|---|---|
| 通常構成の全体試験、ignoredストレス試験を含む | 94成功 | [tests-base.log](tests-base.log) |
| 全featureの全体試験、同ストレス試験を含む | 94成功 | [tests-all-features.log](tests-all-features.log) |
| 最終の生成品質修正後、通常構成のbook・CLI試験 | 26成功 | [tests-final-book.log](tests-final-book.log) |
| 同修正後、全featureのbook・CLI・確保回数試験 | 27成功 | [tests-final-book-all-features.log](tests-final-book-all-features.log) |
| 全target・全featureのClippy、警告をエラー化 | 成功 | [clippy.log](clippy.log) |

最終試験には、既存解析より深く・選択的にする変更と、浅く・非選択的にする変更の両方を追加した。双方の強い条件で解析し、比較不能な新しい値を破棄したまま段階だけ完了することを防ぐ。探索カーネルはこの修正の対象ではない。

8対称形、独立全列挙、強制パスでの色切替、両色の生成文脈、疎bookと厳格モード、品質・モデル不一致、手動値、段階更新、ノード・局面・メモリ制限、チェックポイント失敗と再開、保存先の衝突、破損・件数偽装・不正リンク・未知セクションを検証している。実際の `BookPlayer::choose_move` のヒット経路を100回呼ぶ専用試験ではヒープ確保0回。

再実行例（リポジトリ直下）：

```sh
RUSTFLAGS='-C target-cpu=native' cargo test --locked --release --features book-cli -- --include-ignored
RUSTFLAGS='-C target-cpu=native' cargo test --locked --release --all-features -- --include-ignored
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

## 実モデルでの小規模確認

main由来の変換済み `eval-main.dpr2` と `--legacy-main-mpc` を使用。モデルSHA-256は `3a07488cc364975eed03fe01c1806a6ff2737e99f5879f70f21dc023d64a6069`。

FFO40から合法手を進めた残り5マスの [開始局面](root.obf) を使い、最大2着手の範囲だけ生成した。ノード予算1で中断・保存した後、新しいプロセスで再開し、4局面・9候補のbookを作成。inspect・verify・exportを通し、再出力とバイト一致した。[実行記録](commands.jsonl) と [集計](summary.json) を保存している。

確認用の [tiny.dbook](tiny.dbook) は348バイト。独立した座標ベースのルール実装で局面値と全9候補を完全列挙し、保存された確定値が全て一致する。小さな確認用データであり、実用の序盤bookではない。

```sh
python3 docs/benchmarks/2026-10-29-book/verify_tiny_book.py docs/benchmarks/2026-10-29-book/tiny.dbook

cargo build --locked --release --features book-cli --bin book --example bench_book
# MODELは上記SHAの変換済みDPR2を指定する。生成先は新しいパスにする。
target/release/book build --model MODEL --legacy-main-mpc --tt-mib 1 \
  --positions docs/benchmarks/2026-10-29-book/root.obf --max-plies 2 \
  --work small.dbook-work --book small.dbook --nodes 1
target/release/book resume --model MODEL --legacy-main-mpc --tt-mib 1 \
  --work small.dbook-work --book small.dbook --nodes 1000000
```

## 参照の小規模測定

[測定結果](probe-benchmark.json)：4局面・9候補、常駐配列等の使用量347バイト、読込18.292µs。開始局面1件をウォームアップし、同局面の問い合わせを10,000回実行。全てヒットし、p50 **66ns**、p95 **67ns**、p99 **67ns**。正規化・ハッシュ照合・全候補の座標復元・計時を含む。着手選択処理全体の時間ではない。

```sh
target/release/examples/bench_book small.dbook docs/benchmarks/2026-10-29-book/root.obf 10000
```

極小の同一局面反復であり、大規模bookのキャッシュミス・衝突分布・読込費用を代表しない。常駐量は内部配列等の集計であり、プロセスRSSではない。百万局面の測定は別環境で行う。

## 既存探索の照合

FFO40–49の10局面を、それぞれ新しいEngineプロセス・1スレッド・TT256MiB・完全読みで実行した。ここでは比較元に合わせ、モデル埋込の既定校正を使い、`--legacy-main-mpc` は付けない。

比較元は [前回の結果](../2026-10-28-pull-comparison/results.tsv) の `version=after, round=1`。全10局面で **スコア・着手・モード・深さ・ノード数・葉数・停止理由が一致**。合計は **627,055,807ノード**。[各局面](search-regression.tsv) と [条件・照合結果](search-regression.json) を保存した。

探索の再帰内にbook参照・分岐・ロックは追加していない。本照合は結果と探索量の回帰確認であり、壁時計時間の統計的な性能差や対局勝率の保証ではない。
