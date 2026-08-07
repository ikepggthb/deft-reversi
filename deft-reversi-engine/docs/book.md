# Opening Book (Egaroucid 互換)

`deft-reversi-engine` の定石 book は [Egaroucid](https://github.com/Nyanyan/Egaroucid) の
`book.egbk3` と相互運用できる。Egaroucid が保存した book をそのまま読み込めるし、
このエンジンが保存した book を Egaroucid で読み込める。旧形式 (`.egbk2` / `.egbk`) と
[Edax](https://github.com/abulmo/edax-reversi) の `book.dat` の取り込み・書き出しにも
対応する (Egaroucid 自身が持っている機能に合わせた)。

- 実装: `src/book/`
- 参照した Egaroucid の実装: `src/engine/book.hpp`, `src/engine/util.hpp`

## データモデル — link を持たない

Egaroucid の book の一番の特徴は、**局面ごとの着手リスト (link) をファイルに
持たない**こと。手を知りたいときは合法手を実際に打ってみて、その子局面が book に
登録されているかを引く (`Book::moves_with_value`)。

| | Edax | Egaroucid (この実装) |
|---|---|---|
| 着手リスト | ファイルに保存 | 保存しない。子局面を引いて導出 |
| 1 局面のサイズ | 40 B + 2×(link 数+1) | **25 B 固定** |
| 評価値 | value + lower + upper | value のみ (上下界なし) |
| 統計 | 勝ち/引き分け/負け/ライン数 | `n_lines` のみ |
| パス局面 | book に登録が必要 | 不要 (参照時にパスを挟む) |
| link の張り直し | 必要 (`book link`) | 概念が存在しない |

局面が持つのは次の 4 つだけ (`BookElem`)。

- `value` — 手番側から見た評価値 (石差)
- `level` — その評価値を出したときの探索レベル
- `leaf` — **まだ book に無い手**の中の最善手 (`value` / `move` / `level`)
- `n_lines` — この局面以下の部分木のノード数

`leaf` は「次にどこを掘るべきか」の記録で、book を広げるときの起点になる。

## 局面の正規化

盤面は 8 対称形のうち `(player, opponent)` の辞書順で最小のもの (representative
board) をキーにする。Egaroucid の `representative_board` と同じ規則・同じ
インデックス (0..8) で、`convert_coord_from_representative` /
`convert_coord_to_representative` で着手座標を行き来する。

| idx | 変換 | 座標への作用 |
|-----|------|--------------|
| 0 | なし | `cell` |
| 1 | vertical | `(7-y)*8 + x` |
| 2 | black line (反対角線) | `(7-x)*8 + (7-y)` |
| 3 | black line + vertical | `(7-x)*8 + y` |
| 4 | black line + horizontal | `x*8 + (7-y)` |
| 5 | white line (主対角線) | `x*8 + y` |
| 6 | horizontal | `y*8 + (7-x)` |
| 7 | 180 度回転 | `(7-y)*8 + (7-x)` |

### 座標系が逆でも正規形が一致する理由

Egaroucid は **A1 をビット 63** に置く。このエンジンと Edax は **A1 をビット 0**
に置く。この差はビット列の 180 度回転にあたる。

180 度回転は対称変換の群 (D4, 位数 8) の元なので、ある物理局面の 8 対称形が作る
`(player, opponent)` の集合は両者で**完全に一致する**。正規形はその集合の辞書順
最小なので、**Egaroucid が保存したビット列はこのエンジンの正規形とそのまま同じ**
になる。着手も「u64 のどのビットか」を指すので変換不要でやり取りできる。

そのため Egaroucid の book をビット操作なしで読み書きできる。人間向けの表示名
(A1/H8) だけが 180 度ずれるが、盤面も同じだけずれているので一貫している。
この性質は `src/book/elem.rs` の
`representative_is_invariant_under_the_egaroucid_bit_order` で 2 万局面について
検証している。

## ファイル形式

### `.egbk3` (現行形式)

すべて little endian。ヘッダ 14 バイト + 25 バイト固定長のレコード。

| offset | size | 内容 |
|--------|------|------|
| 0  | 9 | magic `"DICUORAGE"` (= `EGAROUCID` の逆順) |
| 9  | 1 | book version (= 3) |
| 10 | 4 | `i32 n_boards` |

| offset | size | 内容 |
|--------|------|------|
| 0  | 8 | `u64 player` |
| 8  | 8 | `u64 opponent` |
| 16 | 1 | `i8 value` |
| 17 | 1 | `i8 level` |
| 18 | 4 | `u32 n_lines` |
| 22 | 1 | `i8 leaf.value` |
| 23 | 1 | `i8 leaf.move` |
| 24 | 1 | `i8 leaf.level` |

ヘッダの `n_boards` の個数だけ読む (Edax は EOF まで読む)。`value` が石差の範囲外、
あるいは `player & opponent != 0` のレコードは黙って読み飛ばす。同じ盤面が複数
あるときは **level が高い方**を採る (value と leaf は独立に判定)。

疑似座標と特別な値は Egaroucid と同じ。

| 定数 | 値 |
|------|-----|
| `MOVE_PASS` | 64 |
| `MOVE_NOMOVE` | 65 |
| `MOVE_UNDEFINED` | 125 |
| `SCORE_UNDEFINED` | -126 |
| `LEVEL_UNDEFINED` | -1 |
| `MAX_N_LINES` | 4000000000 |

### `.egbk2` / `.egbk` (旧形式)

`.egbk2` はヘッダが同じ (version = 2) で、局面は可変長
(`player`, `opponent`, `value`, `level`, `n_moves`, `n_moves` 個の着手)。
着手リストは読み飛ばす — Egaroucid 自身も v3 では使っていない。

`.egbk` はマジックが無く `i32 n_boards` から始まり、局面は 17 バイト
(`player`, `opponent`, `u8 value_raw`)。評価値は `-((i8)value_raw - 64)` で復元する。

### Edax の `book.dat`

読み書きの両方に対応する (`src/book/edax.rs`)。ヘッダ 42 バイト、局面は
40 バイト + 着手リスト。offset 17 の 1 バイトは C の構造体
`struct { short year; char month, day, hour, minute, second; }` を丸ごと
`fwrite` するために入るパディングで、`sizeof` が 8 になるため必ず存在する。

- **読み込み**: link は捨て、盤面・評価値・level・`n_lines` を取り込む。leaf は
  「その手を打った先の**子局面**」として符号を反転して登録し直す。合法手の無い
  局面 (Edax のパス局面) は取り込まない
- **書き出し**: 子局面を引いて link を合成し、Edax が必要とするパス局面を作って
  書き足す。上下界は ±64 (不明扱い)、勝敗数は 0 で埋める

## データ構造

数千万局面の book を扱えるようにするため、局面表と走査を次のようにしてある
(実装は `src/book/table.rs`)。

### 局面表

正規形の盤面をキーにしたハッシュ表。ハッシュは 2 つの u64 を splitmix64 の
finalizer で混ぜるだけで、既定の SipHash より一桁速い。

| | 時間 (1000 万局面の insert) |
|---|---|
| `BTreeMap` | 8.95 s |
| `HashMap` (SipHash) | 3.31 s |
| `HashMap` (splitmix64) | **0.68 s** |

読み込みは 1 レコードにつき正規化 1 回・表の探索 1 回で済ませ、局面数が
ヘッダで分かるので表は先に確保する。`.egbk3` はチャンク単位のストリーム読みに
してあり、数百 MB のファイルを丸ごとメモリに載せない。

### 走査

木を辿る操作 (negamax / `recalculate_n_lines` / `remove_unreachable` /
`upgrade_better_leaves`) は、各エントリに持たせた**世代印**で訪問済みを判定する。

- 走査の前処理は世代を 1 つ進めるだけなので O(1)。印を消して回らない
- スタックには**今辿っている経路**しか積まない。リバーシは 1 手ごとに石が増える
  ので経路は 60 手で頭打ちになり、book が何千万局面でもスタックは深さ 60

以前は到達局面すべてを `Vec<Board>` と `BTreeSet<Board>` に貯めていたため、
3000 万局面では走査だけで 1.5 GB 以上の一時領域が要った。現在は数 KB で済む。

### 正規化

`representative_board` は比較が `(player, opponent)` の辞書順であることを使い、
まず `player` だけを 8 通り作って最小を求め、`opponent` は同点だった候補に
ついてだけ変換する。大半の局面で同点は 1 つなので、盤面の変換が 16 回から
9 回に減る。読み込み時間で約 18% の差になる。

最適化前の素直な実装を `#[cfg(test)]` に残し、20 万局面で盤面も変換
インデックスも完全に一致することを確かめている
(`representative_board_matches_the_reference_implementation`)。

### 実測

初期盤面から 10 手までの全局面 (347 万局面、連結した book) での計測。

| 操作 | 変更前 | 変更後 |
|---|---|---|
| 登録 347 万局面 | 3.15 s | 1.7–2.5 s |
| negamax | 34.4 s | **4.9 s** |
| `recalculate_n_lines` | 34.3 s | **4.8 s** |
| `remove_unreachable` | 19.0 s | **4.8 s** |
| `reduce` | 6.9 s | 2.4 s |
| `moves_with_value` ×10 万 | 0.07 s | 0.03 s |
| ピーク RSS | 471 MiB | **368 MiB** |

`.egbk3` の読み込み (1000 万レコード、250 MB) は 11.5 s / 684 MiB から
**2.1–2.8 s / 533 MiB** になった。

negamax の残りの時間はほぼメモリ遅延で、局面ごとに最大 33 個の子をハッシュ表で
引くところが支配的。link を保存しない設計の代償で、演算をこれ以上削っても
縮まらない。

## 評価値の伝播

`Book::negamax()` は初期盤面から子を辿って値を返す。

- 値は **子局面の値だけ**から決まる (`value = max(-child.value)`)
- `edax_compliant` を真にすると leaf の値も候補に入る (Edax と同じ扱い)
- `n_lines = 1 + Σ 子の n_lines` (4000000000 で飽和)。同じ正規形に落ちる手が
  複数あると、その子は手の数だけ数えられる (Egaroucid も同じ)
- 上下界は持たない

Egaroucid との違いが 1 点ある。Egaroucid の `negamax_book_p` は、終局局面が
パスした向きだけで登録されているときに符号を 1 回余分に反転する
(同じ Egaroucid の `get_all_moves_with_value` とは扱いが食い違う)。この実装は
参照側と同じ、符号の正しい方に揃えてある。

また Egaroucid は `book[board]` (C++ の `operator[]`) の副作用で、book に無い
局面を negamax 中に空エントリとして挿入してしまう。この実装は挿入しない。

## book の育て方

```rust
use deft_reversi_engine::{Board, Book, Evaluator, Solver, SolverOptions};
use std::sync::Arc;

let solver = Solver::with_options(Arc::new(Evaluator::default()), SolverOptions::default());
let mut book = Book::new();
book.add_board(&Board::new(), 21, &solver);
book.expand(3, 2, 21, &solver); // 3 回、誤差 2 以内で広げる
book.save("book.egbk3").unwrap();
```

`expand` は次を繰り返す。

1. `refresh_leaves` — leaf が古くなった局面の leaf を探索し直す
   (Egaroucid の `check_add_leaf_all_search`)
2. `expand_leaves` — leaf の手の子局面を book に登録する
3. `negamax` — 評価値を親へ伝播させる

leaf の探索には `Solver::solve_with_moves` を使う。root の着手を「まだ book に
無い手」だけに絞った探索で、Egaroucid の `ai_legal(..., use_legal)` に相当する。

Egaroucid の engine 部には book を広げるループが無く (GUI 側にある)、
`expand_leaves` / `expand` はこの実装で足した API。engine 側にある整理の操作は
そのまま移植してある。

| このエンジン | Egaroucid |
|---|---|
| `search_leaf` | `search_leaf` |
| `refresh_leaves` | `check_add_leaf_all_search` |
| `recalculate_leaves` | `recalculate_leaf_all` |
| `invalidate_stale_leaves` | `check_add_leaf_all_undefined` |
| `upgrade_better_leaves` | `upgrade_better_leaves` |
| `negamax` / `recalculate_n_lines` / `fix` | 同名 |
| `reduce` | `reduce_book` |
| `delete_terminal_midsearch` | `delete_terminal_midsearch` |
| `merge` / `change` / `remove` | `merge` / `change` / `delete_elem` |

## 対局での参照

`Book::random_move(board, acc_level)` が Egaroucid の `get_random` と同じ選び方を
する。`acc_level` は 0 が最強、10 が最も緩い。

```
acceptable_min = best - 2*acc_level - 0.5
weight = ((exp(v - best) + 1.5) / 3) ^ (10 - acc_level)
```

の重みで抽選する。加えて、子局面から得られる最善値がその局面自身の値より
`BOOK_LOSS_IGNORE_THRESHOLD` (= 8) 以上悪いときは `None` を返す。book が自己矛盾
しているとみなして探索に回すための保険で、Edax には無い仕組み。

## CLI

```sh
# 新規作成 → 広げる
deft-reversi-cli book new --book book.egbk3
deft-reversi-cli book expand --book book.egbk3 --eval eval.bin \
    --book-level 21 --max-error 2 --rounds 3 --threads 4

# 中身を見る
deft-reversi-cli book info --book book.egbk3
deft-reversi-cli book show --book book.egbk3 --record F5D6

# 局面や棋譜から作る
deft-reversi-cli book add --book book.egbk3 --eval eval.bin --record F5D6
deft-reversi-cli book add-games --book book.egbk3 --eval eval.bin --games games.txt

# 整える / 削る / 統合する
deft-reversi-cli book fix --book book.egbk3
deft-reversi-cli book reduce --book book.egbk3 --max-error-per-move 2 --max-line-error 4
deft-reversi-cli book merge --book a.egbk3 --with b.egbk3 --out merged.egbk3

# 他形式から変換する / Edax 形式で書き出す
deft-reversi-cli book convert --input book.egbk2 --out book.egbk3
deft-reversi-cli book convert --input edax-book.dat --out book.egbk3
deft-reversi-cli book export-edax --book book.egbk3 --out book.dat

# 対局で使う
deft-reversi-cli --book book.egbk3 --book-acc-level 2 --level 20
```

## 互換性の検証

`src/book/egbk.rs` と `src/book/edax.rs` のテストには、Egaroucid の `save_egbk3` /
`save_bin_edax` と同じ順序の書き出しを並べた C プログラムが**実際に出力した
バイト列**を golden data として持たせてある。書き出しがそのバイト列と 1 バイトも
違わないこと、およびそのバイト列を読み戻せることを検証している。

対称形の正規化については、Egaroucid の座標系 (A1 = ビット 63) で表した盤面と
このエンジンの座標系で表した盤面が同じ正規形に落ちることを 2 万局面で確認して
いる (`src/book/elem.rs`)。
