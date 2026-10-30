# 比較ツール

現在使用する入口です。`docs/benchmarks/` のスクリプト・データは測定時の記録として保持します。Linuxの `taskset` とPython 3の標準ライブラリを使います。

## 現行版の変更前後

```sh
RUSTFLAGS="-Ctarget-cpu=native" cargo build --locked --release --example solve
python3 tools/bench/compare_versions.py --before /path/to/before-solve --after target/release/examples/solve --model MODEL --positions tools/bench/positions/ffo40-49.obf --out results.tsv --cpu 0 --rounds 6 --legacy-main-mpc
```

各局面を新しいプロセスで解き、1スレッド・主TT 256 MiBで比較します。`--legacy-main-mpc` はmain由来の変換モデルにだけ指定します。既定の完全読みでは完了・探索方式・深さ・スコア一致を検証します。結果TSVを各実行後に保存し、完走後に時間とファイルハッシュをJSONへ出力します。異なるノード数は許容します。途中失敗時のTSVは未完了の測定であり、最終比較には使いません。既存の出力を上書きするため、実行ごとに別の `--out` を使ってください。

構造変更とアルゴリズム変更を分ける場合は `compare_variants.py --base BEFORE --layout LAYOUT --final AFTER --model MODEL --positions POSITIONS --out results.tsv --rounds 6 --cpu 0 --legacy-main-mpc` を使います。各局面を6通りの実行順で比較します。6の倍数の周回で全順序が均等になります。

## 旧版とのAPI比較

```sh
RUSTFLAGS="-Ctarget-cpu=native" python3 tools/bench/compare_reference.py --old-engine /path/to/old/deft-reversi-engine -- OLD_MODEL NEW_MODEL POSITIONS.obf level:60 5
```

旧クレートの配置を固定せず、`target/reference-compare` に独立したCargoプロジェクトを生成します。`--project PATH` で生成先を変更、`--prepare-only` で生成だけ、`--offline` でキャッシュだけを使用できます。生成先はこのツール専用にしてください。初回に独立したlockfileを生成し、以降は `--locked` で実行します。再現する際は生成先の `Cargo.lock` と両エンジンのコミットを保存してください。旧エンジンの場所を変えた場合は別の生成先を使います。Rustはルートと同じ1.99.1です。

`level:60` は通常API同士の比較です。`exact` と固定深さは診断用途です。旧モデルと新モデルの評価・MPCが対応することを確認してください。

## Edax・Egaroucid

```sh
python3 tools/bench/compare_engines.py --root /path/to/binaries --edax-eval /path/to/edax/eval.dat --model MODEL --positions tools/bench/positions/ffo40-49.obf --answers tools/bench/positions/ffo40-49-answers.obf --out run --cpu 0 --rounds 8
```

`--root` の下に `deft/engine`・`edax/engine`・`egar_simd/engine`・`egar_avx512/engine` を配置します。Egaroucidの評価データなどはそれぞれの実行ディレクトリに、使用する版の要求どおり配置してください。AVX-512版を実行できるCPUが必要です。実行順は4周ごとに均等化します。

Deftは1スレッド・256 MiB・main由来MPC、EdaxとEgaroucidは1スレッド・レベル60・ハッシュ23・定石無効です。各版の完了深さと非選択探索の出力、公開正解のスコアを確認します。時間の出力精度・ノードの数え方はエンジン間で異なります。主TTの項目数を揃えてもメモリ量は一致しません。結果・プロセス出力を各実行で保存します。

Egaroucidの乱数ハッシュを固定する補助コードは `make_egar_hash.cpp` です。対応するEgaroucidソースとビルド・配置方法は [測定時の記録](../../docs/benchmarks/2026-10-27-three-engines/README.md) を参照してください。

## ツール自身の検証

```sh
python3 -m unittest discover -s tools/bench/tests -v
```
