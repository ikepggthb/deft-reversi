# NNUE学習の高速化: DataLoaderボトルネックの解消

対象: `deft-reversi-train/scripts/train_nnue_torch.py`
日付: 2026-07-09

## TL;DR

GPU学習なのにGPU使用率が3%しかなく、実体はデータ供給側(CPU/メモリ/ディスク)が詰まっていた。
以下の4つの修正で解消した。

| # | 修正 | 効果 |
|---|------|------|
| 1 | 特徴量生成をサンプル毎のPythonループから**numpyバッチ一括処理**に変更 | 特徴量生成が単コアで 16,337 → 98,396 samples/s(**6.02倍**) |
| 2 | サンプリング用インデックスをPython intリストから**numpy配列**に変更 | worker毎のメモリ複製(数十GB)が消滅、ページキャッシュが復活しディスク再読み込み(累計1.61TB)が停止 |
| 3 | 学習ループの損失集計を**GPU上のテンソルで累積**し、ログ時のみCPUへ転送 | 毎バッチのGPU同期待ちを排除 |
| 4 | 特徴量テンソルのパディング上限を 302 → **238** に縮小 | 転送量・Embedding参照が約21%減 |

修正前後で生成される特徴量IDの集合は**完全に一致**することを検証済み
(`verify_nnue_dataloader.py`: ランダム盤面20,000件 + 実データ20,000件)。

---

## 前提: 何がどこで計算されているか

学習データ(`.rd`ファイル)の1レコードは18バイトしかない:

```python
RD_DTYPE = np.dtype([("own", "<u8"), ("opponent", "<u8"), ("value", "<i2")])
```

つまり「自分の石のビットボード(u64)、相手の石のビットボード(u64)、評価値(i16)」だけ。
NNUEに入力する特徴量 —— 石の配置(128)、合法手(128)、象限別空きマス数(68)、
n-tupleパターン(42パターン×3^n通り)—— は**学習時に毎回その場で計算**する設計になっている。

この「盤面 → 特徴量ID列」の変換が、モデル本体(Embedding 256 + 32-32 dense の極小ネットワーク)
よりはるかに重く、ここが全体の律速だった。

### 修正前の観測値

- GPU使用率: **約3%**、パッケージ電力18W(ほぼアイドル)
- CPU: 223%、DataLoader worker 19個の多くが D state(ディスク待ち)または R state
- ホストメモリ: 62GiB中59GiB使用、available 2.6GiB
- Block I/O: **read 1.61TB**(データセット全体は23GBしかない = 約70回分の再読み込み)
- 実効スループット: 9,600〜29,800 samples/s で激しく変動

---

## 修正1: 特徴量生成のnumpyバッチベクトル化(主因)

### 修正前: 1サンプルずつ、純Pythonで数千演算

旧実装は `Dataset.__getitem__` の中で、**1サンプルごとに**以下を実行していた。

**(a) 合法手生成 `get_moves()`** —— Pythonの任意精度整数で約60回のシフト/マスク演算を、
盤面1つにつき実行(両視点で2回)。抜粋:

```python
def get_moves(player: int, opponent: int) -> int:
    p = u64(player)
    o = u64(opponent)
    m_o = o & HORIZONTAL_MASK
    flip7 = m_o & u64(p << 7)
    flip9 = m_o & u64(p << 9)
    ...(60行超のスカラー演算が続く)
```

Pythonのintは固定幅整数ではなくヒープ上のオブジェクトなので、`p << 7` の1回ごとに
新しいintオブジェクトの割り当てが発生する。さらに64bitに収めるための `u64()` マスクも毎回必要。

**(b) パターン特徴 `pattern_code()`** —— 42パターン × 各4〜9マスを二重ループでbit判定。
これを両視点で2回:

```python
def pattern_code(instance, own: int, opponent: int) -> int:
    code = 0
    n_squares, squares = instance
    for pos in squares[:n_squares]:        # マスを1個ずつ
        bit = 1 << pos
        if own & bit:      digit = 1
        elif opponent & bit: digit = 2
        else:              digit = 0
        code = code * 3 + digit            # 3進数として畳み込み
    return code
```

**(c) bit抽出 `iter_bits()`** —— 石・合法手の立っているbitを1個ずつgeneratorでyield:

```python
def iter_bits(bitboard: int) -> Iterable[int]:
    bits = u64(bitboard)
    while bits:
        lsb = bits & -bits
        yield lsb.bit_length() - 1
        bits &= bits - 1
```

合計すると**1サンプルあたり数千回のPythonバイトコード実行**になる。CPythonは演算1回ごとに
型判定・メソッド解決・オブジェクト生成という定数コストを払うため、この構造のままでは
どれだけworkerを増やしてもコア単価が上がらない。実測で**単コア約16,000 samples/s**が上限だった。

### 修正後: バッチ全体を配列として一括計算

`__getitem__` は生の値を返すだけにし(`train_nnue_torch.py:434`)、
特徴量生成は `collate_fn` でバッチ(8,196サンプル)を丸ごとnumpy配列として処理する:

```python
def collate_nnue_records(batch):
    own      = np.fromiter((r["own"] for r in batch),      dtype=np.uint64,  count=len(batch))
    opponent = np.fromiter((r["opponent"] for r in batch), dtype=np.uint64,  count=len(batch))
    target   = np.fromiter((r["target"] for r in batch),   dtype=np.float32, count=len(batch))
    features, empties = active_feature_views_batch(own, opponent)
    return {"features": torch.from_numpy(features), ...}
```

以降の全処理がPythonループなしの配列演算になる。ポイントを順に:

**(a) 合法手生成 `get_moves_np()`**(`train_nnue_torch.py:216`)——
アルゴリズムは旧 `get_moves` と1行単位で同一だが、引数がスカラーではなく
`np.uint64` の**配列**になった:

```python
def get_moves_np(player: np.ndarray, opponent: np.ndarray) -> np.ndarray:
    p = player.astype(np.uint64, copy=False)
    o = opponent.astype(np.uint64, copy=False)
    m_o = o & np.uint64(HORIZONTAL_MASK)
    flip7 = m_o & (p << np.uint64(7))    # ← 8,196盤面ぶんが1命令で進む
    ...
```

`p << np.uint64(7)` はnumpy内部のCループで8,196要素を一括シフトする。
Pythonインタプリタを通るのは式1個につき1回だけ。おまけにuint64は自然にwrapするので
旧コードで毎回呼んでいた `u64()` マスクも不要になった。

**(b) ビットボードの展開 `bit_matrix()`**(`train_nnue_torch.py:335`)——
`iter_bits` のループを、ブロードキャストによる全マス一括判定に置き換え:

```python
BIT_SHIFTS = np.arange(N_BOARD_SQUARES, dtype=np.uint64)

def bit_matrix(boards: np.ndarray) -> np.ndarray:
    # [B] uint64 → [B, 64] bool
    return ((boards[:, None] >> BIT_SHIFTS) & np.uint64(1)).astype(bool, copy=False)
```

**(c) パターン特徴 = 行列演算**(`fill_feature_view`, `train_nnue_torch.py:376`)——
「各マスのdigit(自石=1/敵石=2/空=0)を求め、3進数の係数と内積を取る」と読み替えると、
42パターン全部・全サンプルが同時に計算できる。係数行列はモジュールロード時に1度だけ構築:

```python
# 事前構築: [42, 9] 各パターンの参照マス / 3進係数(短いパターンは係数0でパディング)
PATTERN_SQUARE_MATRIX, PATTERN_COEFF_MATRIX = ...

pattern_own      = own_bits[:, PATTERN_SQUARE_MATRIX]        # [B, 42, 9] bool
pattern_opponent = opponent_bits[:, PATTERN_SQUARE_MATRIX]
pattern_digits   = np.where(pattern_own, 1, np.where(pattern_opponent, 2, 0))
pattern_codes    = (pattern_digits * PATTERN_COEFF_MATRIX[None, :, :]).sum(axis=2)  # [B, 42]
pattern_features = NNUE_PATTERN_OFFSET + PATTERN_OFFSET_ARRAY[None, :] + pattern_codes
```

旧コードの `code = code * 3 + digit` の逐次畳み込みが、`digit × 3^k` の内積1発になっている。

**(d) 可変長特徴の左詰め = ソートトリック**(`train_nnue_torch.py:395`)——
石・合法手の特徴は盤面ごとに個数が違うため、最初のベクトル化版ではここだけ
「行ごとに `np.flatnonzero` して詰める」Pythonループが残り、3.65倍止まりだった。
これを次の性質で完全ベクトル化した:

> パディングID(`PADDING_FEATURE_ID = NNUE_INPUT_SIZE`)は**全ての有効特徴IDより大きい**。
> よって「非アクティブなマスをパディングIDに置換して行ごとに昇順ソート」すれば、
> アクティブ特徴が左に詰まり、右側は全てパディングになる。

```python
# 事前構築: 256列それぞれに対応する特徴ID
BITBOARD_FEATURE_COLUMNS = np.concatenate([
    NNUE_STONE_OFFSET      + np.arange(64),   # 自分の石
    NNUE_STONE_OFFSET + 64 + np.arange(64),   # 相手の石
    NNUE_LEGAL_MOVE_OFFSET      + np.arange(64),  # 自分の合法手
    NNUE_LEGAL_MOVE_OFFSET + 64 + np.arange(64),  # 相手の合法手
])

bitboard_mask = np.concatenate([own_bits, opponent_bits,
                                own_move_bits, opponent_move_bits], axis=1)   # [B, 256]
bitboard_features = np.where(bitboard_mask, BITBOARD_FEATURE_COLUMNS[None, :],
                             PADDING_FEATURE_ID)
output[:, :192] = np.sort(bitboard_features, axis=1)[:, :192]   # 左詰め完了
output[:, 192:196] = quadrant_features    # 象限特徴は固定4スロット
output[:, 196:]    = pattern_features     # パターン特徴は固定42スロット
```

先頭192スロットで足りる根拠: 石は両者合計64以下、合法手は各視点とも空きマス数以下なので
アクティブなビットボード特徴は最悪でも `64 + 2×空きマス数 ≤ 128`。
モデル側は `padding_idx` 付きEmbeddingの**総和**なので、特徴の並び順・スロット位置は結果に影響しない。

### なぜこれで速いのか(原理)

CPythonは `a & b` の1演算ごとに「型の確認 → 演算子メソッドの解決 → 結果オブジェクトの
ヒープ割り当て」という数十〜数百nsの固定費を払う。演算対象が1個のintでも8,196個の配列でも
この固定費は同じなので、**バッチをまとめるほど固定費が償却される**。numpyの内部ループは
コンパイル済みCコードで、メモリ上に連続配置されたuint64をSIMD含みで舐めるだけになる。

### 実測

`verify_nnue_dataloader.py` によるCPU単コア比較(ランダム+実データ40,000サンプル):

```
scalar_samples_per_sec = 16,337
batch_samples_per_sec  = 98,396   (6.02x)
```

worker 8個なら理論上約80万samples/sの供給能力になり、データ供給はボトルネックでなくなる。

---

## 修正2: Subsetインデックスのnumpy化(メモリ爆発 → ディスク再読込の停止)

### 修正前: 1億個のPython intがworker 19個に増殖

`--train-limit 100000000` 指定時、各phaseからランダム抽出するインデックスを
**Pythonのintのリスト**で保持していた:

```python
indices = sorted(rng.sample(range(len(dataset)), take))   # 旧
```

Pythonのintは1個約28バイトのヒープオブジェクトで、リストのポインタと合わせると
1億個で**約4GB**。さらに致命的なのは、DataLoaderが `num_workers=19` でforkした後の挙動:

- fork直後、親子はページを共有する(copy-on-write)
- しかしCPythonはオブジェクトに**触るだけで参照カウントを書き込む**
  (`Py_INCREF`)ため、workerがインデックスを読むだけでページが「書き込まれた」ことになる
- 結果、共有が壊れて**worker毎にコピーが物理メモリに増殖**していく

これがホストメモリ62GiBをほぼ食い潰し(観測: used 59GiB / available 2.6GiB)、
玉突きでOSの**ページキャッシュが追い出された**。データセットは23GBでRAMに丸ごと載る
サイズなのに、`shuffle=True` のランダムアクセスが毎回NVMeへの実読み込みになり、
Block I/O read が累計**1.61TB**(データの約70倍)に達していた。workerの多くが
D state(ディスク待ち)だったのはこのため。速度が9,600〜29,800/sで乱高下していたのも
キャッシュヒット率の変動で説明がつく。

### 修正後: 1行の変更

```python
indices = np.array(sorted(rng.sample(range(len(dataset)), take)), dtype=np.int64)  # 新
```

(`train_nnue_torch.py:534`)

numpy配列の中身は参照カウントを持たない生のバイト列(1億個で800MB)なので、
fork後もcopy-on-write共有が維持される。メモリが空けばデータセット全体が
ページキャッシュに常駐し、2周目以降のディスク読み込みはほぼゼロになる。

---

## 修正3: 学習ループのGPU同期削減

### 修正前

毎バッチ、損失を `float(...cpu())` でホストに取り出していた:

```python
batch_loss = float(per_sample_loss.mean().detach().cpu())   # 旧: 毎バッチ同期
loss_sum += batch_loss * batch_size
```

`.cpu()` は「GPU上の全先行処理の完了を待ってから転送」を意味するので、
CPU側がここで毎バッチブロックし、GPUキューが空になる時間が生じる。

### 修正後

集計値をGPU上の0次元テンソルとして持ち、CPUへの転送はログ出力時
(`--progress-every-steps` ごと)とエポック末だけにした(`train_nnue_torch.py:691,724,747`):

```python
loss_sum_tensor = torch.zeros((), device=device)
...
loss_sum_tensor += batch_loss_tensor.detach()      # GPU上で累積(同期なし)
...
if ログを出すタイミング:
    avg_loss = float((loss_sum_tensor / max(1, samples)).detach().cpu())  # ここでだけ同期
```

なお空きマス別集計(`collect_by_empties=True`)はvalidationでしか使わないため
毎バッチ転送のままにしてある(数値の互換性を優先)。

---

## 修正4: パディング上限の適正化

特徴量テンソルは `[B, 2, MAX_ACTIVE]` の固定形状で、足りない分をパディングIDで埋める。
旧上限は過大だった:

```python
# 旧: 42 + 128 + 128 + 4 = 302 (石・合法手それぞれ128個埋まる前提の粗い見積り)
# 新: 42 + 64 + 128 + 4  = 238 (石は両者合計64以下)
NNUE_MAX_ACTIVE_FEATURES_PER_VIEW = 42 + N_BOARD_SQUARES + NNUE_LEGAL_MOVE_FEATURES + 4
```

(`train_nnue_torch.py:39`)

これでworker→メインプロセス→GPUの転送量と、GPU側のEmbedding gather対象が約21%減る。

---

## 検証

`deft-reversi-train/scripts/verify_nnue_dataloader.py` で以下を確認済み:

1. **等価性**: 旧スカラー実装 `active_feature_views`(検証用に残置)と
   新 `active_feature_views_batch` の出力について、パディングを除いた特徴ID集合が
   完全一致すること(順序はEmbedding総和なので不問)。
   - ランダム盤面 20,000件: OK
   - 実データ(`egaroucid_rd_data_set`)20,000件: OK
   - 空きマス数の一致も併せて検証
2. **通し実行**: Dockerイメージ(`deft-reversi-train-rocm:latest`)内でCPU学習
   (20万サンプル・1エポック・workers=4)が完走し、loss低下・validation・
   `nnue-eval.json` エクスポートまで正常動作。CPUのみで約14,700 samples/s。

再実行方法:

```bash
docker compose -f docker-compose.train.yml run --rm deft-reversi-train \
  python3 deft-reversi-train/scripts/verify_nnue_dataloader.py
```

## 運用上の注意

- ベクトル化後はworker 1個あたりの供給能力が約6倍になったため、
  `NUM_WORKERS=19` は過剰。**8程度から始めて** `speed=` ログとGPU使用率
  (`rocm-smi`)を見て調整するのがよい。workerを減らすこと自体がメモリ余裕
  → ページキャッシュ常駐 → I/O安定化につながる。
- CLI引数・metrics出力・`nnue-eval.json` フォーマット・checkpoint互換性は
  すべて維持している(モデル定義は無変更)。
