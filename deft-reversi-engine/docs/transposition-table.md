# Transposition Table

## 位置づけ

この文書は `deft-reversi-engine` の探索内部で使う transposition table の設計メモである。

TT は外部安定 API ではない。探索器の速度と実装のきれいさを優先して変更してよい内部コンポーネントとして扱う。CLI/WASM などの外部利用者は TT の型や内部データ構造に依存しない。

実装は以下の方針を優先する。

- 探索中の `probe` / `store` を最短経路にする。
- multi-thread 探索で共有できる。
- 読み取りは lock-free にする。
- 書き込み競合は best effort とし、負けた store は捨てても探索の正しさを壊さない。
- ハッシュ値だけで一致判定せず、完全な `Board { player, opponent }` を保存して誤 hit を避ける。
- 世代管理により、前回探索の古い entry を低コストで置換しやすくする。

## メモリ構造

`TranspositionTable` は 64 byte cluster の配列で構成する。

```text
TranspositionTable
  clusters: Box<[TTCluster]>
  cluster_mask: usize
  generation: AtomicU8

TTCluster (64 byte aligned, 64 byte total)
  entries: [TTEntry; 2]

TTEntry (32 byte)
  player:   AtomicU64
  opponent: AtomicU64
  data:     AtomicU64
  seq:      AtomicU64
```

1 cluster は cache line 1 本に収まる。1 回の probe は同じ cluster 内の 2 entry だけを見る。

`TTEntryData` は 8 byte に pack する。

```text
min            i8
max            i8
lv             u8
selectivity_lv u8
move0          u8
move1          u8
generation     u8
flags          u8
```

`move0` は最優先の TT move、`move1` は補助 move である。合法手なしや未設定の場合は `NO_COORD` を入れる。

## サイズ

デフォルトサイズは `DEFAULT_TT_MB_SIZE = 256` MiB である。

```rust
let tt = TranspositionTable::new();
```

サイズを指定する場合は `with_mb_size` を使う。

```rust
let tt = TranspositionTable::with_mb_size(512);
```

cluster 数は mask index を速くするため 2 の累乗に切り下げる。そのため実際の確保量は指定 MiB 以下になることがある。

```rust
let actual_bytes = tt.actual_byte_size();
let actual_mib = tt.actual_mb_size();
```

## Hash と Index

TT key は `Board::player` と `Board::opponent` から deterministic mixer で生成する。

```rust
let key = TranspositionTable::key(board);
let cluster = key as usize & cluster_mask;
```

乱数表は使わない。TT 内では key は index 用であり、一致判定は保存済みの `player` / `opponent` と現在の board を直接比較して行う。

## 推奨アクセスパターン

探索側では、1 度 key を作って prefetch、probe、store まで使い回す。

```rust
let key = TranspositionTable::key(board);
tt.prefetch_key(key);

let probe = tt.probe_with_key(board, key);
if let Some(data) = probe.data() {
    // data.min(), data.max(), data.moves() を使って枝刈りや move ordering を行う。
}

// 探索後、probe で得た slot に保存する。
tt.store(
    probe.slot(),
    board,
    min,
    max,
    lv,
    selectivity_lv,
    best_move,
);
```

単純な互換 API として `lookup` / `get` / `add` も残している。

```rust
let hit = tt.lookup(board);
tt.add(board, min, max, lv, selectivity_lv, best_move);
```

ただし高速化する探索では `probe_with_key` と `store` を分け、同じ cluster を二度探さない形を優先する。

## 使い方

探索中の基本的な流れは次の通りである。

1. `TranspositionTable::key(&board)` で key を 1 回だけ作る。
2. `prefetch_key(key)` で対象 cluster を先読みする。
3. `probe_with_key(&board, key)` で hit / miss を判定する。
4. `TTProbe::data()` で保存済みの探索結果を読む。
5. 探索後に `TTProbe::slot()` をそのまま `store()` に渡す。

```rust
let key = TranspositionTable::key(&board);
tt.prefetch_key(key);

let probe = tt.probe_with_key(&board, key);
if let Some(data) = probe.data() {
    let min = data.min();
    let max = data.max();
    let moves = data.moves();

    // ここで枝刈りや move ordering に使う
}

let (min, max, lv, selectivity_lv, best_move) = search_result;

tt.store(
    probe.slot(),
    &board,
    min,
    max,
    lv,
    selectivity_lv,
    best_move,
);
```

`probe()` だけでも使えるが、速度を詰める探索では `probe_with_key()` を優先する。`probe()` は board から key を内部で作り直すため、`prefetch_key()` や `store()` と組み合わせるときは key を外で保持した方が無駄が少ない。

`lookup()` と `get()` は保存済みデータだけ欲しいときの簡易 API である。

```rust
if let Some(data) = tt.lookup(&board) {
    // hit
}
```

`add()` は `probe()` と `store()` を 1 回で済ませる互換 helper である。

```rust
tt.add(&board, min, max, lv, selectivity_lv, best_move);
```

ただし探索ループでは、`probe()` で得た `slot` を `store()` に渡す形が基本になる。

## Probe

`probe` は以下の順に処理する。

1. key から cluster index を求める。
2. cluster 内の 2 entry を seqlock snapshot として読む。
3. `player` / `opponent` が一致すれば hit を返す。
4. 空 entry があれば miss slot として返す。
5. 空きがなければ replacement score が最も低い entry を miss slot として返す。

書き込み中の entry を見た場合は数回だけ retry する。retry 中は `std::hint::spin_loop()` を使うが、通常の mutex/spin-lock を reader が取得するわけではない。

## Store

`store` は probe で得た `TTSlot` に対して保存する。

同じ board が既に入っている場合:

- 同じ `lv` / `selectivity_lv` なら bounds を merge する。
- 新しい entry の探索品質が高いなら置き換える。
- 新しい best move がある場合、古い best move を `move1` に残す。

違う board が入っている場合:

- 空 entry なら保存する。
- 古い generation なら保存する。
- 同じ generation なら、より高品質な entry のみ保存する。

書き込み競合で CAS に負けた場合は数回 retry する。最後まで失敗した store は捨てる。TT は cache なので、store loss は性能にだけ影響し、探索結果の正しさには影響しない。

## Replacement Score

置換候補は以下を優先して選ぶ。

- 空 entry
- 現 generation ではない古い entry
- `lv` / `selectivity_lv` が低い entry

同世代では深い探索結果を残しやすくする。世代が違う entry は大きく低く評価し、反復深化や次 root 探索で自然に入れ替わるようにする。

`set_old()` は旧コード向けの互換 API で、内部では `advance_generation()` を呼ぶ。新規コードでは `advance_generation()` を直接使う。

## 世代管理

`generation` は `u8` の atomic counter である。

```rust
tt.advance_generation();
```

探索の root が変わるタイミングや、反復深化の区切りで進める。探索 worker が同じ TT を読んでいる最中に頻繁に進める用途は想定しない。

generation が 0 に wrap した場合は entry を clear し、generation を 1 に戻す。

互換 API として `set_old()` は `advance_generation()` を呼ぶ。

## 並列性

TT は複数 thread から `&TranspositionTable` で共有できる。

読み取り:

- `seq` を acquire load する。
- 奇数なら writer がいるため snapshot 失敗とする。
- `player` / `opponent` / `data` を読む。
- acquire fence 後に `seq` を再読込し、値が同じなら一貫した snapshot とみなす。

書き込み:

- 偶数 `seq` を CAS で奇数にする。
- board と data を書く。
- `seq + 2` を release store して publish する。

この方式は reader 側を lock-free にできる。writer 側は短時間だけ entry を占有するが、失敗時は retry 後に store を諦められる。

## Prefetch

`prefetch_key` は x86_64 では `_mm_prefetch` を使って対象 cluster を L1 に寄せる。非 x86_64 では no-op である。

探索側では、子局面を作る前後や move ordering の直前など、実際に probe する少し前に呼ぶと効果が出やすい。

```rust
let key = TranspositionTable::key(next_board);
tt.prefetch_key(key);
```

## Safety

実装内では速度のために限定的に `unsafe` を使う。

- `get_unchecked`: cluster index は `key & cluster_mask`、entry index は固定範囲から作る。
- `write_bytes` in `clear(&mut self)`: `&mut self` により concurrent access がないことを要求する。zero bit pattern は atomic 整数と TT entry に対して有効である。
- `mem::transmute`: `TTDataFields` は `#[repr(C)]` かつ compile-time assert で 8 byte を確認する。
- `_mm_prefetch`: x86_64 のみで呼び、pointer は cluster 配列内を指す。

探索中に共有 TT を clear したい場合は `clear(&mut self)` ではなく、全 worker を止めてから実行する。古い entry を使い捨てたいだけなら `advance_generation()` を使う。

## 値の範囲

`TTEntryData` は compact にするため score と depth を小さい型に詰める。

- `min`, `max`: `i8`
- `lv`, `selectivity_lv`: `u8`
- `best_move`: `u8`

`store` は debug build で範囲を検査する。release build では呼び出し側が範囲を保証する。

## Test と Benchmark

engine crate の単体テスト:

```sh
cargo test -p deft_reversi_engine
```

TT の micro benchmark:

```sh
cargo test -p deft_reversi_engine --release bench_mixed_probe_store -- --ignored --nocapture
```

benchmark は test harness 上の簡易測定であり、実探索の速度を完全には表さない。変更比較用の目安として使う。
