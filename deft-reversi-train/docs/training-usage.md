# deft-reversi-train 学習手順

`deft-reversi-train` は、Egaroucid 形式の `.rd` データセットから次の2種類の評価関数を学習できます。

- `pattern`: 従来の n-tuple パターン評価
- `nnue`: n-tuple / 石配置 / 合法手を入力にした NNUE 評価

まずは小さい件数で動作確認し、その後に全データで長時間学習する流れを推奨します。

## データセット

想定しているデータセット配置は次の形です。

```text
egaroucid_rd_data_set/
  phase_12/
    train.rd
    valid.rd
    test.rd
  phase_13/
    train.rd
    valid.rd
    test.rd
  ...
```

各 `.rd` ファイルは `RDGBBVAL1\n` で始まり、その後に次の固定長レコードが並びます。

```text
u64 own_bits       little-endian
u64 opponent_bits  little-endian
i16 value          little-endian
```

`phase_XX` の `XX` は盤面上の石数です。エンジン内部の phase は盤面から次の式で計算します。

```text
engine_phase = min(stones - 4, 59)
```

このリポジトリで使うデータセット例:

```text
/home/azure/dev/deft-reversi/egaroucid_rd_data_set
```

## 全体の流れ

1. 小さい `--train-limit` / `--valid-limit` で smoke run する
2. `metrics.csv` / `metrics.jsonl` が出ることを確認する
3. loss と disc MAE のグラフを作る
4. 問題なければ `--release` で本番学習する
5. 途中停止に備えて `--checkpoint-dir` を必ず指定する
6. 出力された `.bin` をエンジンで読み込み、最後は自己対戦などで強さを確認する

## Pattern 評価の学習

Pattern 評価は、アクティブな n-tuple 特徴、mobility 特徴、bias を更新します。損失関数は Huber loss、最適化は sparse AdaGrad です。

まずは smoke run:

```sh
cargo run -p deft-reversi-train -- train pattern \
  --data /home/azure/dev/deft-reversi/egaroucid_rd_data_set \
  --out /tmp/pattern-smoke.bin \
  --epochs 1 \
  --batch-size 256 \
  --train-limit 10000 \
  --valid-limit 1000 \
  --shuffle-buffer 8192 \
  --prefetch-buffer 1024 \
  --progress-every-steps 10 \
  --log-dir /tmp/pattern-smoke-log
```

本番学習例:

```sh
cargo run -p deft-reversi-train --release -- train pattern \
  --data /home/azure/dev/deft-reversi/egaroucid_rd_data_set \
  --out runs/pattern/pattern-eval.bin \
  --epochs 3 \
  --batch-size 1024 \
  --lr 0.05 \
  --shuffle-buffer 65536 \
  --prefetch-buffer 8192 \
  --checkpoint-dir runs/pattern/checkpoints \
  --log-dir runs/pattern/logs \
  --progress-every-steps 1000
```

出力される `pattern-eval.bin` は `EngineFile v3` の `EvaluatorData::Pattern` です。

## NNUE 評価の学習

NNUE 評価は、現在の推論構造と同じ形で学習します。

```text
sparse input
  ├─ n-tuple パターン特徴
  ├─ player / opponent の石配置特徴
  └─ player合法手数 x opponent合法手数 の mobility pair

        ↓

FT accumulator 256 x 2 viewpoints
        ↓
clipped ReLU
        ↓
[side-to-move accumulator | opponent accumulator] 512
        ↓
dense 32
        ↓
dense 32
        ↓
disc score
```

学習中は float の重みを使い、最後にエンジンの推論で使う固定小数点の `i16` / `i32` 形式へ変換して `.bin` に保存します。最適化は Adam、損失関数は Huber loss です。

NNUE はパラメータ数と Adam の状態が大きいため、必ず `--release` を使ってください。

まずは smoke run:

```sh
cargo run -p deft-reversi-train --release -- train nnue \
  --data /home/azure/dev/deft-reversi/egaroucid_rd_data_set \
  --out /tmp/nnue-smoke.bin \
  --epochs 1 \
  --batch-size 128 \
  --train-limit 10000 \
  --valid-limit 1000 \
  --shuffle-buffer 8192 \
  --prefetch-buffer 1024 \
  --progress-every-steps 10 \
  --log-dir /tmp/nnue-smoke-log
```

本番学習例:

```sh
cargo run -p deft-reversi-train --release -- train nnue \
  --data /home/azure/dev/deft-reversi/egaroucid_rd_data_set \
  --out runs/nnue/nnue-eval.bin \
  --epochs 3 \
  --batch-size 1024 \
  --lr 0.001 \
  --shuffle-buffer 65536 \
  --prefetch-buffer 8192 \
  --checkpoint-dir runs/nnue/checkpoints \
  --log-dir runs/nnue/logs \
  --progress-every-steps 1000
```

出力される `nnue-eval.bin` は `EngineFile v3` の `EvaluatorData::Nnue` です。

## NNUE 評価のDocker GPU学習

GPUで学習する場合は、Rust実装ではなく PyTorch 実装のトレーナーを使います。標準手順では ROCm/PyTorch Docker コンテナ内で NNUE 重みを学習し、`NnueEvaluatorData` のJSONを作り、そのまま Rust の `pack-nnue` でエンジンが読む `EngineFile v3` の `.bin` へ変換します。

```text
.rd dataset
  -> Docker: train_nnue_torch.py --device cuda
  -> runs/nnue-gpu/nnue-eval.json
  -> Docker: cargo run -p deft-reversi-train --release -- pack-nnue
  -> runs/nnue-gpu/nnue-eval.bin
```

現在の主対象は `../nn-reversi` と同じ AMD GPU / ROCm 環境です。NVIDIA CUDA 用のcomposeは必要になった時点で別途追加します。ROCm 版 PyTorch でも、PyTorch API 上の device 名は `cuda` です。

ホスト側の前提:

- Docker から `/dev/kfd` と `/dev/dri` が見える
- 実行ユーザーが GPU デバイスを使える
- `torch.cuda.is_available()` が `True` になる

イメージをビルドします。

```sh
docker compose -f docker-compose.train.yml build
```

コンテナに入る場合:

```sh
docker compose -f docker-compose.train.yml run --rm deft-reversi-train
```

GPU確認:

```sh
docker compose -f docker-compose.train.yml run --rm deft-reversi-train \
  python3 -c "import torch; print(torch.__version__); print(torch.cuda.is_available()); print(torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'no gpu')"
```

`torch.cuda.is_available()` が `True` であることを確認してください。`False` の場合、`--device cuda` の学習は失敗します。

ホストから smoke run する例:

```sh
EPOCHS=1 TRAIN_LIMIT=10000 VALID_LIMIT=1000 BATCH_SIZE=1024 \
  deft-reversi-train/scripts/docker_train_nnue_gpu.sh
```

本番学習:

```sh
deft-reversi-train/scripts/docker_train_nnue_gpu.sh
```

スクリプトのデフォルト値:

```text
DATA=/workspace/deft-reversi/egaroucid_rd_data_set
RUN_DIR=runs/nnue-gpu
EPOCHS=3
BATCH_SIZE=4096
LR=16
VALID_LIMIT=200000
NUM_WORKERS=8
PREFETCH_FACTOR=4
PROGRESS_EVERY_STEPS=100
```

`TRAIN_LIMIT` / `VALID_LIMIT` を指定した場合、PyTorchトレーナーは `phase-range` 内の各phaseからおおむね均等に抽出します。起動時の `train_phase_counts=` / `valid_phase_counts=` で、各phaseから使った件数を確認できます。
同じ `SEED` と `TRAIN_LIMIT` なら、各epochで同じサンプル集合を使います。

全件使う場合は `TRAIN_LIMIT` を指定しないでください。主な上書き例:

GPU版の `LR` は、最終的に `i16` / `i32` へ量子化する重みの単位で指定します。`LR=0.001` のような通常のfloatモデル向けの値では更新が小さすぎ、lossがほぼ動きません。

```sh
RUN_DIR=runs/nnue-gpu-v2 EPOCHS=5 BATCH_SIZE=2048 LR=8 \
  deft-reversi-train/scripts/docker_train_nnue_gpu.sh
```

精度を上げたい場合は、まず `TRAIN_LIMIT` を増やし、epoch数も増やします。100万件は動作確認より少し大きい程度なので、本格的には 500万件以上を推奨します。

```sh
TRAIN_LIMIT=5000000 \
EPOCHS=8 \
BATCH_SIZE=8192 \
VALID_LIMIT=500000 \
LR=8 \
NUM_WORKERS=12 \
PREFETCH_FACTOR=4 \
PROGRESS_EVERY_STEPS=25 \
RUN_DIR=runs/nnue-gpu-5m-lr8 \
deft-reversi-train/scripts/docker_train_nnue_gpu.sh
```

GPU使用率が低い場合は、多くの場合 active feature生成と合法手生成のCPU側処理が律速です。`NUM_WORKERS` を 8, 12, 16 のように上げ、GPUメモリに余裕があれば `BATCH_SIZE=8192` や `BATCH_SIZE=16384` を試します。

中断した学習を checkpoint から再開する場合:

```sh
RESUME=1 deft-reversi-train/scripts/docker_train_nnue_gpu.sh
```

別の checkpoint を指定して再開する場合:

```sh
RESUME=runs/nnue-gpu/checkpoint.pt deft-reversi-train/scripts/docker_train_nnue_gpu.sh
```

出力:

```text
runs/nnue-gpu/nnue-eval.json
runs/nnue-gpu/nnue-eval.bin
runs/nnue-gpu/checkpoint.pt
runs/nnue-gpu/logs/metrics.csv
runs/nnue-gpu/logs/metrics.jsonl
runs/nnue-gpu/logs/valid_by_empties.csv
runs/nnue-gpu/logs/valid_by_empties.jsonl
runs/nnue-gpu/logs/training_metrics.svg
runs/nnue-gpu/logs/valid_by_empties.svg
```

Dockerスクリプトは学習完了後に `training_metrics.svg` と `valid_by_empties.svg` を必ず生成します。手動で再生成する場合:

```sh
python3 deft-reversi-train/scripts/plot_training_metrics.py \
  runs/nnue-gpu/logs/metrics.csv \
  --out runs/nnue-gpu/logs/training_metrics.svg
```

コンテナ内で手動実行したい場合は、次の2段階を直接実行できます。

```sh
python3 deft-reversi-train/scripts/train_nnue_torch.py \
  --data /workspace/deft-reversi/egaroucid_rd_data_set \
  --out-json runs/nnue-gpu/nnue-eval.json \
  --checkpoint runs/nnue-gpu/checkpoint.pt \
  --epochs 3 \
  --batch-size 4096 \
  --lr 16 \
  --valid-limit 200000 \
  --device cuda \
  --num-workers 8 \
  --prefetch-factor 4 \
  --log-dir runs/nnue-gpu/logs \
  --progress-every-steps 100

cargo run -p deft-reversi-train --release -- pack-nnue \
  --input runs/nnue-gpu/nnue-eval.json \
  --out runs/nnue-gpu/nnue-eval.bin \
  --eval-name nnue-gpu \
  --eval-version 1
```

注意点:

- 現在のGPU学習対象は NNUE のみです。
- active feature生成と合法手生成はCPU側で行い、NNUE本体とoptimizerをGPUで動かします。
- `input_weights` は巨大なため、`nnue-eval.json` はかなり大きくなります。最終的には `pack-nnue` で `.bin` にしてください。
- GPUが使えない環境で `--device cuda` を指定するとエラーになります。検証用には `DEVICE=cpu` でも動かせます。

## ログとグラフ化

`--log-dir DIR` を指定すると、学習ログがファイルに出力されます。

```text
DIR/metrics.csv
DIR/metrics.jsonl
```

`--log-dir` を省略して `--checkpoint-dir` だけ指定した場合は、checkpoint ディレクトリの下にログが出ます。

標準エラーには次のような進捗ログが出ます。

```text
epoch=001/003 train batch=1000/51234 step=1000 samples=1024000 total_samples=1024000 avg_loss=1.234567 disc_mae=2.345 last_loss=1.111111 last_disc_mae=2.100 lr=0.001000 speed=12345.6/s elapsed=1m23s eta_epoch=1h10m00s eta_total=3h30m00s
epoch=001/003 summary train_loss=1.100000 train_disc_mae=2.200 valid_loss=1.300000 valid_disc_mae=2.500 samples=1024000 total_samples=1024000 step=1000
```

主な項目:

- `avg_loss`: その epoch 内の平均 Huber loss
- `disc_mae`: その epoch 内の平均絶対誤差。単位は石数差
- `last_loss`: 直近 batch の Huber loss
- `last_disc_mae`: 直近 batch の平均絶対誤差
- `valid_loss`: validation データでの Huber loss
- `valid_disc_mae`: validation データでの平均絶対誤差
- `lr`: 学習率
- `speed`: 1秒あたりの処理サンプル数
- `eta_epoch`: 現在 epoch の残り時間目安
- `eta_total`: 全体の残り時間目安

`metrics.csv` の `kind=train` 行は step ごとの推移、`kind=summary` 行は epoch ごとの train / validation 比較に使います。

SVG グラフを作るには次を実行します。

```sh
python3 deft-reversi-train/scripts/plot_training_metrics.py \
  runs/nnue/logs/metrics.csv \
  --out runs/nnue/logs/training_metrics.svg
```

Pattern の場合:

```sh
python3 deft-reversi-train/scripts/plot_training_metrics.py \
  runs/pattern/logs/metrics.csv \
  --out runs/pattern/logs/training_metrics.svg
```

## Checkpoint と再開

`--checkpoint-dir` を指定すると、途中経過が保存されます。

```text
checkpoint.json
pattern-checkpoint.bin
nnue-checkpoint.bin
```

中断した学習を再開する例:

```sh
cargo run -p deft-reversi-train --release -- train nnue \
  --data /home/azure/dev/deft-reversi/egaroucid_rd_data_set \
  --out runs/nnue/nnue-eval.bin \
  --resume runs/nnue/checkpoints \
  --checkpoint-dir runs/nnue/checkpoints \
  --log-dir runs/nnue/logs
```

Pattern の場合も同じです。

```sh
cargo run -p deft-reversi-train --release -- train pattern \
  --data /home/azure/dev/deft-reversi/egaroucid_rd_data_set \
  --out runs/pattern/pattern-eval.bin \
  --resume runs/pattern/checkpoints \
  --checkpoint-dir runs/pattern/checkpoints \
  --log-dir runs/pattern/logs
```

## よく使う引数

- `--data`: `.rd` データセットのルートディレクトリ
- `--out`: 学習後に出力する評価ファイル
- `--epochs`: データセットを何周するか
- `--batch-size`: 1回の更新で使うサンプル数
- `--lr`: 学習率
- `--train-limit`: 学習データを先頭から指定件数に制限する。動作確認用
- `--valid-limit`: validation 件数
- `--phase-range`: 使う phase 範囲。標準は `12..63`
- `--shuffle-buffer`: ストリーミング読み込み時の局所シャッフルサイズ
- `--prefetch-buffer`: 背景読み込みキューサイズ
- `--checkpoint-dir`: checkpoint 保存先
- `--log-dir`: `metrics.csv` / `metrics.jsonl` の保存先
- `--progress-every-steps`: 何 optimizer step ごとに進捗を出すか
- `--resume`: checkpoint から再開するディレクトリ

## 学習結果の見方

まず見るべき値は `valid_disc_mae` です。これは validation データに対して、評価値が正解値から平均で何石分ずれているかを表します。

学習が進むと、通常は次のようになります。

- `avg_loss` が下がる
- `disc_mae` が下がる
- `valid_loss` / `valid_disc_mae` も下がる
- train だけ下がって valid が悪化する場合は過学習の可能性がある

ただし、loss が低いことと対局で強いことは同じではありません。最終的な品質確認では、出力した評価ファイルを `deft_reversi_engine` で読み込み、探索付きの自己対戦や既存評価との比較を行ってください。

## 推奨手順

最初に Pattern を短く回して、データ読み込み・ログ・出力ファイル生成を確認します。

```sh
cargo run -p deft-reversi-train -- train pattern \
  --data /home/azure/dev/deft-reversi/egaroucid_rd_data_set \
  --out /tmp/pattern-smoke.bin \
  --epochs 1 \
  --train-limit 10000 \
  --valid-limit 1000 \
  --log-dir /tmp/pattern-smoke-log
```

次に NNUE も短く確認します。

```sh
cargo run -p deft-reversi-train --release -- train nnue \
  --data /home/azure/dev/deft-reversi/egaroucid_rd_data_set \
  --out /tmp/nnue-smoke.bin \
  --epochs 1 \
  --train-limit 10000 \
  --valid-limit 1000 \
  --log-dir /tmp/nnue-smoke-log
```

問題なければ、本番用の `runs/` ディレクトリに checkpoint と log を残しながら学習します。
