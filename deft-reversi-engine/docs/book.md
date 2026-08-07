# Opening Book

`deft-reversi-engine` の定石 book。標準の形式は独自の `.dbk`
([deft book](#dbk-標準形式)) で、[Egaroucid](https://github.com/Nyanyan/Egaroucid) の
`.egbk3` (旧形式の `.egbk2` / `.egbk` を含む) と
[Edax](https://github.com/abulmo/edax-reversi) の `book.dat` は取り込み・書き出しで
対応する。

- 実装: `src/book/`
- 参照した実装: Egaroucid `src/engine/book.hpp`, Edax `src/book.c`

## 何を変えたか

Edax と Egaroucid の book を読んだうえで、3 点を変えてある。

### 1. 値だけでなく「その値がどれだけ信用できるか」を持つ

Edax は評価値と上下界を、Egaroucid は評価値と探索レベルだけを持つ。どちらも
**この値がどこまで詰めた結果なのかをファイルから復元できない**。`level` は
そのエンジンの探索表に紐づく数字なので、エンジンをまたぐと意味が変わってしまう。

`BookValue` (`src/book/value.rs`) は探索の中身そのものを持つ。

| 欄 | 内容 |
|---|---|
| `score` | 手番側から見た評価値 (石差) |
| `lower` / `upper` | 上下界 |
| `depth` | 実際に読んだ深さ |
| `selectivity` | MPC の選択度。6 なら枝刈りなし |
| `flags` | 完全読みか / 終盤か / 人手で固定か / 子から伝播したか |

上下界は**子から積み上げて**計算する (`Book::propagate`)。

- **下界** = 登録済みの子の中の最善。その手は実際に選べるので、少なくともこの値は出る
- **上界** = 合法手をすべて持っていれば同じく子から。持っていなければ
  `Frontier` (まだ登録していない手の中の最善) の見積もりも入れる。
  frontier をまだ調べていなければ上界は不明 (±64)

この幅 (`BookValue::uncertainty`) がそのまま

- 育成でどこを掘るべきかの指標 ([最良優先の育成](#育成--最良優先))
- 対局中に book を信じてよいかの指標 (`Trust`)

になる。Edax の上下界は探索窓をそのまま保存したもので、子から積み上げてはいない。

`Trust` は 4 段階で、対局中は「信用度がこれ未満なら book を使わず探索に回す」
という指定ができる (`PickPolicy::min_trust`)。

| `Trust` | 条件 |
|---|---|
| `Exact` | 終局まで読み切った確定値 |
| `High` | 上下界の幅が 4 以内 |
| `Low` | それより広い |
| `None` | 値が無い |

### 2. 局面を手数 (ply) の層に分けて持つ

book は初期盤面から手を進めた DAG で、**子は必ず 1 手多い**という強い構造を持つ。
ハッシュ表に全局面をばらまくのではなく、この構造を素直に持たせてある
(`src/book/layer.rs`)。

```
LayeredTable
 ├ PlyLayer[0]  boards[] values[] frontiers[] index(HashMap)
 ├ PlyLayer[1]  boards[] values[] frontiers[] index(HashMap)
 ├ …
 └ PlyLayer[60]
```

得られるもの:

- **評価値の伝播が線形走査になる。** ply の大きい方から処理すれば子は必ず確定
  済みなので、木を辿る必要も訪問済み判定 (世代印) も要らない
- **同じ ply の局面は互いに独立。** 層の中はそのまま並列に処理できる
- **子を引く先が 1 層に限定される。** ハッシュ表が小さいほどキャッシュに乗る
- **到達可能性の判定が前向きの 1 パスになる。** キューも訪問済み集合も不要
- **「25 手目まで」といった深さ制限が自然に書ける**
- **ファイルも層で区切れるので、読み込みを層単位で並列化できる**

層の中は列指向 (盤面・値・frontier を別々の配列に持つ) にしてある。走査は
たいてい 1 つの配列を舐めるだけで済み、キャッシュに無駄が乗らない。

パスは石数を変えないので、ある局面とそのパス後の局面は同じ層に入る。

### 3. 定石名を持つ

Edax にも Egaroucid にも無い。局面から「この進行は 虎定石 です」を引ける
(`src/book/names.rs`)。名前は局面ではなく**手順**に付き、局面から引くと

- `names_at` — その局面が終点になる定石
- `names_through` — その局面を通る定石 (まだ途中のもの)

の 2 つが返る。形式はリポジトリにある `opening.txt` と同じ `名前 = 手順`。

## データモデル

局面は正規形の盤面をキーに、次の 2 つを持つ。

```rust
BookValue { score, lower, upper, depth, selectivity, flags }   // 8 B
Frontier  { mv, score, depth }                                  // 3 B
```

`Frontier` は「まだ book に無い手の中の最善」で、Egaroucid の `leaf` に相当する。
探索の深さも持たせてあるので、親の値の**上界**を抑えるのに使える。

着手リスト (link) は保存しない。手を知りたいときは合法手を実際に打ってみて、
その子局面が次の層にあるかを引く (`Book::children`)。Egaroucid と同じ方針で、

- レコードが固定長になりファイルが小さい
- link の張り直しという操作が要らない
- パス局面を保存する必要がない (参照時にパスを挟むだけ)

`n_lines` (その局面を通る変化の本数) は**保持しない**。層構造のおかげで深い層から
1 回舐めれば数え直せるので、育成のたびに張り直す欄を抱える意味がない。
`.egbk3` / `.dat` へ書き出すときだけ計算する。

## 局面の正規化

盤面は 8 対称形のうち `(player, opponent)` の辞書順で最小のもの (正規形) を
キーにする (`src/book/sym.rs`)。Egaroucid の `representative_board` と同じ規則・
同じインデックス (0..8) で、`convert_coord_from_representative` /
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

人間向けの表示名 (A1/H8) だけが 180 度ずれるが、盤面も同じだけずれているので
一貫している。この性質は `src/book/sym.rs` の
`representative_is_invariant_under_the_egaroucid_bit_order` で 2 万局面について
検証している。

### 実装

`representative_board` は比較が `(player, opponent)` の辞書順であることを使い、
まず `player` だけを 8 通り作って最小を求め、`opponent` は同点だった候補について
だけ変換する。大半の局面で同点は 1 つなので、盤面の変換が 16 回から 9 回に減る。

最適化前の素直な実装を `#[cfg(test)]` に残し、20 万局面で盤面も変換インデックスも
完全に一致することを確かめている
(`representative_board_matches_the_reference_implementation`)。

## `.dbk` (標準形式)

すべて little endian。ヘッダ 32 バイト + 層の並び + 末尾の定石名。

### ヘッダ

| offset | size | 内容 |
|--------|------|------|
| 0  | 8 | magic `"DEFTBOOK"` |
| 8  | 2 | `u16` version (= 1) |
| 10 | 2 | `u16` flags (bit 0: frontier 列を含む) |
| 12 | 4 | `u32` 局面数 |
| 16 | 1 | `u8` 育成レベル |
| 17 | 1 | `u8` 育成の最大 ply |
| 18 | 1 | `u8` 自分の手で許した損 |
| 19 | 1 | `u8` 相手の手で許した損 |
| 20 | 4 | `u32` 層の数 |
| 24 | 8 | `u64` 本体のハッシュ (0 なら未計算) |

### 層 (「層の数」だけ並ぶ)

| size | 内容 |
|------|------|
| 1 | `u8` ply |
| 4 | `u32` この層の局面数 `n` |
| 8n | `u64 player` の列 |
| 8n | `u64 opponent` の列 |
| n | `i8 score` の列 |
| n | `i8 lower` の列 |
| n | `i8 upper` の列 |
| n | `u8 depth` の列 |
| n | `u8 selectivity` の列 |
| n | `u8 flags` の列 |
| 3n | frontier の列。flags bit 0 のときだけ |

### 末尾

| size | 内容 |
|------|------|
| 4 | `u32` 定石数 |
| .. | 定石ごとに `u16` 名前長 + 名前 + `u16` 手順長 + 手順 (UTF-8) |
| 2 + .. | `u16` 備考長 + 備考 |

### なぜこの形か

Egaroucid の `.egbk3` は 1 局面 25 バイトのレコードが並ぶ**行指向**。`.dbk` は
**列指向**にしてある。

- 同じ列は値の分布が揃うので、`zstd` などに通したときによく縮む。行指向だと
  u64 の盤面と 1 バイトの値が交互に来て統計が混ざる
- 局面数だけ数えたい、値の分布を見たい、といったときに必要な列だけ読める
- frontier 列は配布用の book では丸ごと省ける (`--slim`、12% 減)

層ごとに区切ってあるので、**読み込みを層単位で並列化できる**。読み込みの大半は
索引 (ハッシュ表) の構築だが、層が違えば衝突しない。

ヘッダのハッシュ (FNV-1a 64) はファイルの破損検出用で、暗号学的な強度は無い。
保存は一時ファイルに書いてから `rename` するので、途中で落ちても元の book が
残る。

### 1 局面あたりのサイズ

| | バイト数 | 持っている情報 |
|---|---|---|
| Egaroucid `.egbk3` | 25 | 値, level, n_lines, leaf |
| **`.dbk`** | **25** | 値, **上下界**, 深さ, 選択度, フラグ, frontier |
| `.dbk --slim` | 22 | 上と同じ (frontier を除く) |
| Edax `book.dat` | 40 + 2×(link+1) | 値, 上下界, level, 勝敗数, n_lines, link, leaf |

同じ 25 バイトで、Egaroucid より復元できる情報が多い。`n_lines` を捨てて
上下界・深さ・選択度に回した分。

## 他形式との相互運用

### Egaroucid `.egbk3` / `.egbk2` / `.egbk`

`src/book/egbk.rs`。形式の詳細はモジュールの doc コメントにある。

Egaroucid の `level` は `BookValue` の深さ・選択度にそのままは移せないので、

- `level >= 空きマス数` なら終局まで読んでいるとみなす
- そうでなければ `level` を探索深さとみなし、選択度は既定値

と近似する。近似なので上下界は広めに付く。取り込んだ book をこのエンジンで
育て直すと、探索し直した局面から順に本来の幅に締まっていく。

`leaf` はこの book の `Frontier` と同じ役割なのでそのまま移る。

### Edax `book.dat`

`src/book/edax.rs`。Edax は 3 つのエンジンの中で唯一、評価値の上下界を
ファイルに持っている。この book も上下界を持つので**そのまま移せる**。
取り込みでは探索の深さから見積もった窓と Edax の窓の狭い方を採る。

- **読み込み**: link は捨て、盤面・評価値・上下界・level を取り込む。leaf は
  「その手を打った先の子局面」として符号を反転して登録し直す。合法手の無い
  局面 (Edax のパス局面) は取り込まない
- **書き出し**: 子局面を引いて link を合成し、Edax が必要とするパス局面を作って
  書き足す。上下界は積み上げた値を書く。勝敗数は 0

### 値がまだ入っていない局面

`.egbk3` と `.dat` は「値が無い」を表せない。棋譜から局面だけ足した直後
(`book add-games`) のような book をこの 2 つで書き出すと、その分は**落ちる**。
書き出し側で数を合わせて落とし、CLI は落とした数を報告する
(`Book::n_undefined`)。`.dbk` は落とさない。

## 評価値の伝播

`Book::propagate()` は深い層から順に 1 回舐めるだけ。層を降りきった時点で
その層の子はすべて確定しているので、木を辿る必要も訪問済み判定も要らない。

各局面について、登録済みの子から

- `lower` = max(子の `-upper`)
- `upper` = 合法手をすべて持っていれば max(子の `-lower`)、持っていなければ
  frontier の見積もりも入れる。frontier 未調査なら ±64
- `score` = max(子の `-score`)
- `depth` = 最善の子の深さ + 1

を積み上げる。`PINNED` フラグの付いた値 (人手で設定したもの) は上書きしない。
積み上げた値と元の値が同じ信用度なら**積み上げた方を採る** — 実際の続きを
見ている分だけ確かなので。

## 育成 — 最良優先

`src/book/grow.rs`。

Edax も Egaroucid も、育成の基本は「book の端 (leaf) を全部そのまま 1 段ずつ
広げる」という幅優先で、どこを先に掘るかという順序を持たない。Edax の
`book_deviate` には「最善からどれだけ離れた変化まで追うか」の制限があるが、
その範囲の中では順序を付けない。

ここは**最良優先 (best-first)** にした。掘る候補に優先度を付け、良いものから
順に予算 (局面数・時間) の限り掘る。優先度は次の順に見る。

1. **根からの損** — 自分の手で許す損と相手の手で許す損を**別々に**数える。
   自分は最善しか選ばないが相手は間違えるかもしれない、という非対称性が実戦の
   book では本質的なので、1 本の数字にまとめない
2. **値の不確かさ** — 上下界が閉じている局面をさらに掘っても book は強く
   ならない。分かっていないところを先に掘る
3. **浅さ** — 同じなら根に近い方を先に。序盤ほど使われる回数が多い

2 が効くのは、この book が上下界を子から積み上げているから。「登録済みの手の
中の最善」が下界、「まだ登録していない手を含めた見積もり」が上界なので、
幅が広い局面はそのまま「まだ調べ足りない局面」になっている。

### 1 局面あたりの探索

未登録の手 `M` を持つ局面 `P` について 2 回探索する。

1. `solve_with_moves(P, level, M)` — `M` の中の最善手 `m` と、その値。
   `P → m` の子局面を book に足す
2. `solve_with_moves(P, level, M \ {m})` — 残りの中の最善手。これが新しい
   `Frontier` になり、`P` の値の**上界**を決める

2 回目は 1 回目で暖まった置換表に乗るので実際には安い。Edax も Egaroucid も
1 回しか探索しないので上界が分からない。

`solve_with_moves` は root の着手を絞った探索で、Egaroucid の
`ai_legal(..., use_legal)` に相当する。

### 到達可能性

根から損の範囲内で辿れる局面を集める処理は、層構造のおかげで**浅い層から
1 回舐めるだけ**で済む。キューも訪問済み集合も要らない。同じ局面に別の経路で
届いたときは損の小さい方を採る。

### 並列化

育成の時間はほぼ探索で、局面ごとの探索は互いに独立している。1 局面の探索は
たいてい浅く、探索の中を YBWC で割るより**局面をばらまく方が効率が良い**。

- 共有カウンタで 1 件ずつ取らせる (work stealing)。局面によって探索時間が
  桁違いに変わるため、均等分割だと最も重い 1 件で律速される
- 探索中は book に触らないので同期が要らない。「候補を集める」→「並列に探索」→
  「結果を反映」の 3 段に分けてある
- `Solver` は置換表がアトミックなので共有できる。共有できることは
  `src/search/solver.rs` のコンパイル時アサーションで保証している

### 中断と再開

`GrowthPolicy` に

- `max_positions` — 追加する局面数の上限
- `time_limit` — 打ち切る時間
- `stop` — 外から立てる `AtomicBool`

を持たせてある。どれで止まっても book は一貫した状態のままで、`GrowthReport`
に止まった理由が入る。`Frontier` がファイルに残るので、次に呼んだときは
続きから掘り進む。

## 実測

初期盤面から 10 手までの全局面 (347 万局面、連結した book)。4 コア、release。

| 操作 | 時間 |
|---|---|
| 登録 347 万局面 | 4.0 s |
| `to_dbk_bytes` (86 MB) | 0.35 s |
| `from_dbk_bytes` | 0.79 s |
| `to_egbk3_bytes` (86 MB) | 1.99 s |
| `propagate` | 1.69 s |
| `prune_unreachable` | 3.61 s |
| `reduce(2)` | 1.88 s |
| `moves` ×10 万 | 0.04 s |
| 表のピーク RSS | 355 MiB |

再現: `cargo run --release --example book_bench -- enumerate 10`

同じベンチの旧設計 (ハッシュ表 + 世代印) では値の伝播 (`negamax`) が 4.9 s、
到達不能局面の削除が 4.8 s だった。層構造にしたことで伝播は約 2.9 倍速い。
表のピーク RSS はほぼ同じ (368 MiB → 355 MiB) だが、1 局面あたりの情報は
上下界・深さ・選択度・frontier の分だけ増えている。

`.egbk3` の書き出しが `.dbk` の 5 倍かかるのは、`n_lines` を子から数え直す
ためで、形式の違いではない。

### 育成の並列度

level 14 で 400 局面を追加:

| スレッド | 時間 | 倍率 |
|---|---|---|
| 1 | 111.9 s | — |
| 2 | 62.6 s | 1.79× |
| 4 | 36.8 s | 3.04× |

再現: `cargo run --release --example book_bench -- grow 400 14 <threads>`

線形に届かないのは、1 巡の中で最も重い 1 局面を待つ分。巡ごとの直列処理
(伝播と候補集め) は book が小さいうちは無視できる。

### ハッシュ

盤面のハッシュは 2 つの u64 を splitmix64 の finalizer で混ぜるだけ。
1000 万局面の insert で:

| | 時間 |
|---|---|
| `BTreeMap` | 8.95 s |
| `HashMap` (SipHash) | 3.31 s |
| `HashMap` (splitmix64) | **0.68 s** |

## 対局での参照

`Book::probe(board)` が対局中に要るものをまとめて返す。

```rust
Probe {
    value,     // この局面の値 (上下界つき)
    moves,     // book にある手。値の良い順
    names,     // この局面が終点になる定石の名前
    upcoming,  // この局面を通る定石の名前
    complete,  // 合法手をすべて持っているか
}
```

手を選ぶのは `Book::pick(board, policy)`。`PickPolicy` は 3 つに分けてある。

| 欄 | 意味 |
|---|---|
| `max_loss` | 最善手からこれ以上悪い手は選ばない (石差) |
| `randomness` | 0.0 で常に最善、1.0 で許容範囲から一様 |
| `min_trust` | これ未満の信用度しか無いときは book を使わない |

Egaroucid の `acc_level` は「指数を変えて重みを歪める」という数字で意味が
読み取りにくいので分解した。互換のため `PickPolicy::from_accuracy_level(0..=10)`
を残してある。

加えて、子局面から得られる最善値がその局面自身の値より
`BOOK_LOSS_IGNORE_THRESHOLD` (= 8) 以上悪いときは `None` を返す。book が自己
矛盾しているとみなして探索に回すための保険。

`min_trust` は Edax にも Egaroucid にも無い。book を無条件に信じると、浅い探索で
入れただけの値をそのまま指してしまう。上下界を持っているからこの判断ができる。

## CLI

```sh
# 新規作成 → 育てる
deft-reversi-cli book new --book book.dbk --note "level 21, 2026-08"
deft-reversi-cli book grow --book book.dbk --eval eval.bin \
    --level 21 --max-ply 30 --player-error 0 --opponent-error 4 \
    --positions 5000 --threads 4

# 時間で区切る (途中で止めても book は一貫した状態)
deft-reversi-cli book grow --book book.dbk --eval eval.bin --seconds 3600

# 中身を見る
deft-reversi-cli book info --book book.dbk --by-ply
deft-reversi-cli book show --book book.dbk --record F5D6C3D3C4

# 棋譜から局面を足す (値は入らないので、続けて grow する)
deft-reversi-cli book add-games --book book.dbk --games games.txt --max-ply 20

# 定石名
deft-reversi-cli book names --book book.dbk --import opening.txt
deft-reversi-cli book names --book book.dbk --export opening-out.txt

# 整える / 削る / 統合する
deft-reversi-cli book fix --book book.dbk --prune-unreachable
deft-reversi-cli book reduce --book book.dbk --max-loss 2
deft-reversi-cli book merge --book a.dbk --with b.dbk --out merged.dbk

# 形式変換 (拡張子で判別)
deft-reversi-cli book convert --input book.egbk3 --out book.dbk
deft-reversi-cli book convert --input edax-book.dat --out book.dbk
deft-reversi-cli book convert --input book.dbk --out book.egbk3
deft-reversi-cli book convert --input book.dbk --out book.dat --level 21
deft-reversi-cli book convert --input book.dbk --out slim.dbk --slim

# 対局で使う
deft-reversi-cli --book book.dbk --book-acc-level 2 --level 20
```

`book grow` は評価器を読めなかったら**失敗する**。既定の評価器に黙って落ちると
使い物にならない book ができるので。

## 互換性の検証

`src/book/egbk.rs` と `src/book/edax.rs` のテストには、Egaroucid の `save_egbk3` /
Edax の `book_save` と同じ順序の書き出しを並べた C プログラムが**実際に出力した
バイト列**を golden data として持たせてある。書き出しがそのバイト列と 1 バイトも
違わないこと、およびそのバイト列を読み戻せることを検証している。

対称形の正規化については、Egaroucid の座標系 (A1 = ビット 63) で表した盤面と
このエンジンの座標系で表した盤面が同じ正規形に落ちることを 2 万局面で確認して
いる (`src/book/sym.rs`)。
