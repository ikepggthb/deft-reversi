# Egaroucid の YBWC 実装詳解

この文書は、Egaroucid のソースを手元で再参照しなくても、同じ探索制御を別実装へ移植できる粒度で YBWC（Young Brothers Wait Concept）を記述する。

対象は `/home/azure/dev/Egaroucid_test/Egaroucid` の commit `112981e381801a38604f54714d46eb9d90322b80`（2026-07-02）である。対象ワークツリーには未追跡の `diff1.txt` があるが、以下で扱うエンジンソースは commit の内容と一致する。

ここでいう「同じ実装」は、探索結果だけでなく、次を含む。

- どの深さで分割を許可するか
- どの子を親スレッドに残すか
- 並列処理の登録に失敗したとき何をするか
- 並列処理する子へ何をコピーし、何を共有するか
- 幅1の探索窓の α超過をどう兄弟へ伝播するか
- NegaScout でどの手を再探索するか
- 非同期結果をどの順番で回収し、探索した局面数をどう合算するか
- 停止フラグの寿命をどう保証するか

## 読む前に：用語はこれだけ

本文では、一般的な探索用語とEgaroucidの変数名を区別する。まず次の意味だけ押さえれば読み進められる。英語名は、ソースコードと照合するときのために括弧内へ残す。

| この文書での表現 | 意味 |
|---|---|
| 局面、親、子 | 探索木上の一つの盤面を「局面」と呼ぶ。ある局面から一手進んだ局面が「子」、一手前が「親」である。 |
| 最初の子 | 手順の並べ替えで最善と予想された子。YBWCでは、この子だけを先に直列探索する。原理名の elder brother / eldest child に相当する。 |
| 残りの子 | 最初の子以外の候補手。YBWCで並列化する対象であり、原理名の young brothers に相当する。 |
| NWS（幅1の探索窓） | 「値が α を上回るか」だけを安く判定する探索。Null Window Search の略。 |
| α以下 / α超過 | 子の結果 `g` が `g <= α` なら α以下、`g > α` なら α超過と表す。一般的な fail-low / fail-high と同じ意味である。 |
| 通常幅の探索窓 | αとβの間を探索し、より正確な値を求める探索。NegaScoutでNWSの結果を確かめ直すときに使う。 |
| 作業スレッド | 親スレッドから渡された子局面を並列に探索するスレッド。一般に worker と呼ばれるもの。 |
| 非同期結果 | 並列処理の終了後に値を受け取る入れ物。原実装の `std::future<Parallel_task>` に相当する。 |
| 停止条件一覧 | 探索全体と各分割の停止フラグをまとめたもの。どれか一つが false なら、その探索を止める。原実装の `searchings` である。 |

略号のうち、TT は「以前の探索結果を再利用する表」、ETC は「子を深く読む前にできる枝刈り」、MPC は「浅い探索から確率的に行う枝刈り」を指す。Lazy SMP はYBWCとは別に補助探索を並列実行する仕組みであり、該当節でだけ扱う。

## 1. 参照ソース

中心となるファイルは次の通りである。行番号は上記 commit に対するもの。

| ファイル | 主な範囲 | 役割 |
|---|---:|---|
| `src/engine/ybwc.hpp` | 23-33 | 分割深さ、末子数、返却特殊値 |
| `src/engine/ybwc.hpp` | 54-67 | 子を探索する並列処理の本体 `ybwc_do_task_nws` |
| `src/engine/ybwc.hpp` | 86-111 | 並列処理の登録 `ybwc_split_nws` |
| `src/engine/ybwc.hpp` | 116-338 | NWS ノードの弟探索（vector/固定配列版） |
| `src/engine/ybwc.hpp` | 343-542 | NegaScout ノードの弟探索（vector/固定配列版） |
| `src/engine/midsearch_nws.hpp` | 315-318 | 複数停止フラグの AND 判定 |
| `src/engine/midsearch_nws.hpp` | 328-477 | YBWC を呼ぶ NWS 本体 |
| `src/engine/midsearch.hpp` | 156-334 | 通常の内部 NegaScout ノード |
| `src/engine/midsearch.hpp` | 529-629 | ルート相当の `first_nega_scout_legal` |
| `src/engine/thread_pool.hpp` | 38-256 | 並列処理受理条件、作業スレッド、探索 ID ごとの上限 |
| `src/engine/parallel.hpp` | 24-29 | 子を探索した処理の返却値 |
| `src/engine/search.hpp` | 235-374 | `Search` の状態、再構築、move/undo |
| `src/engine/search.hpp` | 653-666 | `Flip_value` |
| `src/engine/move_ordering.hpp` | 602-608 | 降順の手順ソート |
| `src/engine/setting.hpp` | 144-167 | 有効な YBWC 機能スイッチ |

実際に有効なのは NWS と NegaScout の固定配列版である。vector 版も同じロジックを持つが、現設定では主に無効化された解析経路用である。

## 2. この実装の全体像

Egaroucid は共有分割点から作業スレッドが手を取り出す方式ではない。親スレッドが各着手を順に見て、空き作業スレッドがいればその着手後の局面を独立した処理として作業スレッドに渡す。作業スレッドは専用に再構築した `Search` で、その子をNWS（幅1の探索窓による探索）を行う。

```text
親の局面
  |
  +-- 候補手を評価し、良さそうな順に並べる
  |
  +-- 最初の子を親スレッドで直列探索
  |      |
  |      +-- ここで枝刈りできれば、残りの子は探索しない
  |
  +-- 残りの子を良さそうな順に処理
         |
         +-- 作業スレッドが空いている -> 子局面を渡してNWS
         |
         +-- 空いていない -> 親スレッドがその子をNWS
         |
         +-- 最後の子 -> 必ず親スレッドがNWS
         |
         +-- どれかがαを上回る -> 同じ分割の兄弟を停止
                |
                +-- 親もNWS: その値で枝刈り
                |
                +-- 親がNegaScout: 必要なら通常幅で直列再探索
```

YBWC の “wait” は「最善と予想する最初の子の探索が終わるまで弟を走らせない」こととして実現される。最初の子の結果で α が確定・更新された後だけ、弟をその α の幅1の探索窓で並列化する。

重要なのは、親スレッドも待機専用にはならない点である。並列処理を投入しながら兄弟を走査し、作業スレッドを確保できなかった子と意図的に残した末子を自分で探索する。最後にだけすべての非同期処理が終わるのを待つ。

## 3. 探索値と探索窓の規約

探索値は常に手番側視点の negamax 値である。

親ノードの α を `parent_alpha` とすると、ある子を NWS する呼び出しは次になる。

```cpp
g = -nega_alpha_ordering_nws(child, -parent_alpha - 1, child_depth, ...);
```

子側の探索窓は `[-parent_alpha - 1, -parent_alpha]`、親側へ符号を戻すと `[parent_alpha, parent_alpha + 1]` である。整数スコアを前提とした幅 1 の探索窓で、結果の解釈は次の通り。

- `g <= parent_alpha`: α以下。親の α を上げないので、その子は完了扱いにできる。
- `g > parent_alpha`: α超過。NWS 親ならそのまま枝刈り。NegaScout 親なら β 未満の場合に通常幅の探索窓での再探索が必要。

Egaroucid のオセロスコアは通常 `[-64, 64]` の範囲である。制御用の値は探索値の外に置かれている。

```cpp
YBWC_NOT_PUSHED = -124;
YBWC_PUSHED     =  124;
SCORE_UNDEFINED = -126;
```

現行の `ybwc_split_nws` は実スコアを返さず、`YBWC_NOT_PUSHED` または `YBWC_PUSHED` だけを返す。関数内には TT 枝刈り/MPC の結果を直接返す旧案がコメントアウトされているため、呼び出し側には「特殊値以外なら探索値」という分岐が残っている。

## 4. 有効条件と定数

ビルドスイッチは次の設定である。

```cpp
#define USE_YBWC_NWS true
#define USE_YBWC_NEGASCOUT true
#define USE_YBWC_NEGASCOUT_ANALYZE false
#define USE_YBWC_SPLITTED_TASK_TERMINATION false
```

分割深さは次である。

```cpp
YBWC_MID_SPLIT_MIN_DEPTH = 6;
YBWC_END_SPLIT_MIN_DEPTH = 16;
YBWC_N_YOUNGER_CHILD     = 1;
```

呼び出し元は、これから探索する**子の深さ** `depth - 1` で判定する。

```text
中盤探索: depth - 1 >= 6   すなわち現在ノード depth >= 7
終局探索: depth - 1 >= 16  すなわち現在ノード depth >= 17
```

さらに `search->use_multi_thread` が true でなければならない。

子を探索する処理が構築する新しい `Search` の `use_multi_thread` は次で決まる。

```cpp
(!is_end_search && child_depth > 6) ||
( is_end_search && child_depth > 16)
```

ここが `>=` ではなく `>` なのは意図された整合である。子ノード自身がさらに分割するには、その孫の深さ `child_depth - 1` が最小深さ以上でなければならない。つまり `child_depth > min_depth` となる。

最大分割深さ、同時実行並列処理数、空き数の明示チェックはソースに残っているがコメントアウトされている。実際の上限はスレッドプールの空き作業スレッド数と探索 ID の使用数の上限だけで決まる。

## 5. 必要なデータ構造

### 5.1 `Parallel_task`

子を探索する処理は次を返す。

```cpp
struct Parallel_task {
    int value;             // 親視点へ符号反転済みの値、または SCORE_UNDEFINED
    uint64_t n_nodes;      // この子専用 Search が訪問したノード数
    uint_fast8_t cell;     // この子へ至る着手
    int move_idx;          // 親の move_list 内の位置
};
```

現行の回収処理が実際に使うのは `value`、`n_nodes`、`move_idx` である。`cell` は格納されるが参照されない。

`move_idx` は並列処理を登録した後も安定していなければならない。したがって、YBWCの弟ノード探索関数に入った後は `move_list` を並べ替えてはいけない。Egaroucid は弟ノード探索関数の前に全体を一度だけ降順ソートし、以後は `flip.flip = 0` を完了マーカーとして使う。

### 5.2 `Flip_value`

各候補手は次を持つ。

```cpp
struct Flip_value {
    Flip flip;          // pos と反転石 bitboard
    int value;          // move ordering score
    uint64_t n_legal;   // 着手後の相手合法手。再計算を避ける
};
```

`flip.flip != 0` が未処理、`flip.flip == 0` が処理済み/ETC で除外済みという意味を兼ねる。合法手なら少なくとも一石は返るため、この特殊値は成立する。

### 5.3 子を探索する処理専用の `Search`

親は候補手を `search->move()` した**後**に並列処理の登録を試みる。その時点の次の値を処理本体に値渡しする。

- `board.player`, `board.opponent`
- `n_discs`
- `parity`
- `mpc_level`
- `is_presearch`
- `thread_id`
- 親の α、子深さ、事前計算済み合法手、終局探索フラグ
- 着手位置、`move_idx`
- 停止フラグを指すポインタ群

作業スレッドはこれらから新しい `Search` を構築し、評価特徴量をその盤面から再計算する。親の mutable な `Search` 自体は共有しない。特に次は子へコピーされない。

- 親の `n_nodes`（子は 0 から数え、終了後に合算する）
- 親の評価 取り消し用の履歴
- killer/history/counter-move 情報（現設定では機能自体が無効）

これにより、親は処理の登録直後に `undo()` して次の兄弟へ進める。親 `Search` と作業スレッド `Search` が同じ盤面オブジェクトを同時変更することはない。

### 5.4 停止フラグの停止条件一覧

NWS の再帰では、停止条件を `std::vector<bool*> searchings` で表す。

```cpp
is_searching(searchings) = all(*flag == true for flag in searchings)
```

要素は外側から内側の順に並ぶ。

```text
[探索全体の searching,
 上位 split の n_searching,
 ...,
 現在 split の n_searching]
```

vector 自体は並列処理する子へ値コピーされるが、各ポインタの指す bool は共有される。したがって、どの祖先 split が false になっても、その子孫は `is_searching()` で停止する。

通常 NegaScoutの弟ノード探索関数は新たに次を作る。

```cpp
bool n_searching = true;
std::vector<bool*> searchings = {searching, &n_searching};
```

NWSの弟ノード探索関数は既存停止条件一覧を受け取り、ローカル `n_searching` のポインタを末尾へ追加し、終了時に pop する。

## 6. スレッドプールが処理を受け付ける条件

同じ動作を再現するには、一般的な「常に待ち行列へ積めるスレッドプール」に置き換えるだけでは不十分である。

Egaroucid の `push(id, &pushed, task)` は次のときだけ処理を受理する。

1. pool が稼働中である。
2. `n_idle > 0`、つまり予約可能な空いている作業スレッドがいる。
3. `n_using_thread[id] < max_thread_size[id]`、つまり探索 ID ごとの使用数の上限内である。

受理時は排他制御内で条件を再確認し、待ち行列へ一件追加し、`n_idle` を一つ減らし、作業スレッドを一つ起こす。探索 ID が `THREAD_ID_NONE` でなければ使用数も一つ増やす。作業スレッド完了時に使用数を減らす。

受理されなかった処理は待ち行列に残らない。`ybwc_split_nws` は一度非同期結果を vector に追加してから `pushed` を見て、false なら直ちに `pop_back()` する。呼び出し側は同じ子をその場で直列探索する。

この規約には三つの効果がある。

- 待ち処理を大量に作らない。
- 作業スレッドが再帰的に split しても、空きがなければ直列へ戻るため処理待ち同士の相互待ちを作らない。
- 親スレッドが末子や投入失敗した子を探索するので、作業スレッドの仕事と並行して必ず前進する。

親は非同期結果待ち中に別処理を実行して助けることはない。回収も完了順ではなく、`parallel_tasks` の投入順に 処理が終わるまで待つ `get()` する。

## 7. 最初の子を直列探索する処理

YBWCの弟ノード探索関数を呼ぶ前に、NWS/NegaScout の本体は共通して次を行う。

1. TT を 参照 し、TT 最善手があれば最初に直列探索する。
2. ETC で確定した手を `flip.flip = 0` にする。
3. 残る手に move-ordering score と `n_legal` を設定する。
4. YBWC 条件を満たせば全候補を 評価値の降順に並べ替え する。
5. TTに記録された手 をまだ探索していなければ、ソート後の最初の有効手を直列探索する。
6. その手を `flip.flip = 0` にする。
7. 局面がまだ枝刈りしていない場合だけ弟ノード探索関数を呼ぶ。

内部 NegaScout の最初の子は通常幅の探索窓 `[-beta, -alpha]` で探索される。NWSを行う局面の最初の子は幅1の探索窓で探索される。いずれも、弟が使う α は最初の子の結果を反映した後の値である。

TTに記録された手 を先に探索済みなら、それが概念上の最初の子である。ソート後の先頭をもう一度直列探索することはない。`serial_searched` がこの区別を持つ。

## 8. 並列処理を登録する関数 `ybwc_split_nws`

挙動をそのまま写すと次になる。

```text
function try_split(child_position, parent_alpha, child_depth,
                   n_remaining_moves, move_idx, stop_chain, local_stop):
    if n_remaining_moves < 1:
        return NOT_PUSHED

    if not all_flags_true(stop_chain):
        return NOT_PUSHED

    future, pushed = pool.try_push(search_id,
        task = do_nws_task(
            copy(child_position), parent_alpha, child_depth,
            copy(stop_chain), pointer(local_stop), move_idx))

    if pushed:
        futures.append(future)
        return PUSHED

    discard future
    return NOT_PUSHED
```

実コードの `n_remaining_moves` 引数には、弟ノード探索関数呼び出し時の残手数そのものではなく、ループ内で次が渡る。

```cpp
n_available_moves - n_moves_seen
```

`n_moves_seen` は `flip.flip != 0` の手に遭遇するたび増える。閾値が 1 なので、最後の一手になった時点で split を拒否する。その手は親スレッドが直列探索する。これが Egaroucid の「末弟を手元に残す」規則である。

なお `running_count` も引数にあるが、現行の投入可否には使われない。コメントアウトされた同時実行数制限の名残である。

## 9. 作業スレッドが実行する関数 `ybwc_do_task_nws`

作業スレッドの処理は次である。

```text
function do_nws_task(copied_child_state, parent_alpha, child_depth,
                     legal, stop_chain, pointer(local_stop), move_idx):
    child_search = Search(copied_child_state)
    child_search.use_multi_thread = child_depth > split_min_depth

    value = -NWS(child_search,
                 alpha = -parent_alpha - 1,
                 depth = child_depth,
                 skipped = false,
                 legal = legal,
                 stop_chain = stop_chain)

    if any flag in stop_chain is false:
        value = SCORE_UNDEFINED
    else if value > parent_alpha:
        *local_stop = false

    return {
        value,
        n_nodes = child_search.n_nodes,
        cell,
        move_idx
    }
```

判定順序が重要である。探索終了時に既に他の兄弟が split を停止していれば、この処理の値は `SCORE_UNDEFINED` に捨てられる。まだ全フラグが true で、自分が最初に α超過を報告する処理なら、自分の値を有効なまま `local_stop = false` にする。

したがって意図上は、ある split で最初に観測された α超過一件が勝者となり、他の並列兄弟は中断値になる。NWS では一件の α超過があれば十分である。

## 10. NWSを行う局面で残りの子を探索する

入口は次である。

```cpp
ybwc_search_young_brothers_nws(
    search, alpha, &v, &best_move,
    n_available_moves, hash_code, depth, is_end_search,
    move_list, canput, searchings);
```

`alpha` は値渡しで固定される。作業スレッドが探索中に別の結果で `v` が変わっても、既に投入した処理の探索窓は変わらない。

現行設定で動く本処理を忠実な擬似コードにすると次の通り。

```text
function search_young_brothers_nws(search, alpha, inout v, inout best_move,
                                   n_available, depth, move_list, stop_chain):
    futures = []
    local_stop = true
    stop_chain.push(&local_stop)
    n_moves_seen = 0

    for move_idx in 0 .. move_list.length:
        if not all_flags_true(stop_chain): break
        if move_list[move_idx].flip_bits == 0: continue

        n_moves_seen += 1
        searched_here = false
        search.move(move)

        split_state = try_split(
            current_child_position,
            parent_alpha = alpha,
            child_depth = depth - 1,
            n_remaining_moves = n_available - n_moves_seen,
            move_idx,
            stop_chain,
            &local_stop)

        if split_state == PUSHED:
            pass
        else:
            if split_state == NOT_PUSHED:
                g = -NWS(search, -alpha - 1, depth - 1, ..., stop_chain)
            else:
                g = split_state       // 現行では到達しない旧 fast path
                search.n_nodes += 1

            if all_flags_true(stop_chain):
                searched_here = true
                if g > v:
                    v = g
                    best_move = move.pos
                    if g > alpha:
                        local_stop = false

        search.undo(move)

        if searched_here:
            move.flip_bits = 0

    for future in futures in submission order:
        result = future.get()
        search.n_nodes += result.n_nodes
        if result.value != SCORE_UNDEFINED and result.value > v:
            v = result.value
            best_move = move_list[result.move_idx].pos

    stop_chain.pop()
```

直列探索が α超過した場合も `local_stop = false` となり、既に走っている作業スレッドは停止する。作業スレッドが α超過した場合は作業スレッド自身が同じ flag を false にするため、親の兄弟投入ループも停止する。

回収時に `alpha < result.value` を再判定して分割専用停止フラグを落とすコードはコメントアウトされている。最初にα超過を返した作業スレッドが既に flag を落としているため、通常は不要である。

弟ノード探索関数はすべての非同期結果を必ず `get()` してからこの分割専用の停止フラグを pop・破棄する。外側の停止で処理が `SCORE_UNDEFINED` になっても、その処理のノード数は必ず親へ足す。

`hash_code` は引数に残るが、現行の通常回収経路では使わない。TT 登録は弟ノード探索関数から戻った呼び出し元の NWS 本体が行う。

## 11. NegaScoutを行う局面で残りの子を探索する

NegaScout では最初の子だけを通常幅の探索窓で探索した後、弟をすべて現在の α に対する NWS で試す。α以下の弟は完了できるが、`alpha < g < beta` の弟は principal variation 候補なので通常幅の探索窓での再探索が必要になる。

固定配列版の挙動は次である。

```text
function search_young_brothers_pvs(search,
                                   inout alpha, beta,
                                   inout v, inout best_move,
                                   n_available, depth, move_list,
                                   global_stop):
    futures = []
    local_stop = true
    stop_chain = [global_stop, &local_stop]
    research_indices = []
    next_alpha = alpha
    n_searched = 0
    n_moves_seen = 0

    for move_idx in 0 .. move_list.length:
        if not *global_stop or not local_stop: break
        if move_list[move_idx].flip_bits == 0: continue

        n_moves_seen += 1
        move_done = false
        search.move(move)

        state = try_split(...,
                          parent_alpha = alpha,
                          child_depth = depth - 1,
                          n_remaining_moves = n_available - n_moves_seen)

        if state != PUSHED:
            if state == NOT_PUSHED:
                g = -NWS(search, -alpha - 1, depth - 1, ..., stop_chain)
            else:
                g = state
                search.n_nodes += 1

            if *global_stop and local_stop:
                if g > v:
                    v = g
                    best_move = move.pos

                if g > alpha:
                    next_alpha = max(next_alpha, g)
                    local_stop = false
                    research_indices.push(move_idx)
                else:
                    move_done = true

        search.undo(move)

        if move_done:
            move.flip_bits = 0
            n_searched += 1

    for future in futures in submission order:
        result = future.get()
        search.n_nodes += result.n_nodes

        if result.value == SCORE_UNDEFINED:
            continue

        if result.value > v:
            v = result.value
            best_move = move_list[result.move_idx].pos

        if result.value > alpha:
            next_alpha = max(next_alpha, result.value)
            research_indices.push(result.move_idx)
        else:
            move_list[result.move_idx].flip_bits = 0
            n_searched += 1

    if research_indices is not empty and next_alpha < beta and *global_stop:
        alpha = next_alpha

        for idx in research_indices:
            search.move(move_list[idx])
            g = -NegaScout(search,
                           child_alpha = -beta,
                           child_beta  = -alpha,
                           depth - 1, ...)
            search.undo(move_list[idx])

            move_list[idx].flip_bits = 0
            n_searched += 1

            if *global_stop:
                if g > v:
                    v = g
                    best_move = move_list[idx].pos
                if g > alpha:
                    alpha = g
                    if alpha >= beta:
                        break

        if alpha < beta and *global_stop:
            recurse on remaining nonzero moves, with
                n_available = n_moves_seen - n_searched
```

### 11.1 `next_alpha` を先に採用する理由

NWS で `g > alpha` が分かった時点で、その手は親値の新しい下限を与える。そのため通常幅の探索窓での再探索に入る前に `alpha = max(g...)` とし、再探索探索窓を狭める。

実際には `local_stop` により最初の α超過で兄弟を止めるため、有効な `research_indices` は通常一件である。ただし、停止観測の競合や将来の停止方式変更に耐える形として vector と `max` が用いられている。

### 11.2 β 以上なら再探索しない

`next_alpha >= beta` なら再探索 block 自体へ入らない。幅1の探索窓の結果だけで β 枝刈りに十分だからである。この場合弟ノード探索関数内の `alpha` は更新されないが、`v` は β 以上の値へ更新され、呼び出し元はその `v` を返す。

### 11.3 再帰の目的

α超過で一度兄弟処理を止めるため、まだ一度も見ていない手、または中断され `SCORE_UNDEFINED` になった手が `flip.flip != 0` で残る。通常幅の探索窓での再探索後も 局面が開いていれば弟ノード探索関数自身を再帰呼び出しし、残りを新しい α で処理する。

再帰へ渡す `n_moves_seen - n_searched` は厳密な全残手数ではなく、現在回で「遭遇したが完了していない手」の数である。α超過が早く、後続手をまだ走査していない場合は過小になり得る。その場合、次回は split 条件が保守的になって直列探索が増えるだけで、全 `move_list` を再走査するため手を失うことはない。

`hash_code`、`need_best_move`、ローカルの `prev_alpha` は現行関数に残る未使用値である。

## 12. NWS と NegaScout の差分

| 項目 | NWS 親 | NegaScout 親 |
|---|---|---|
| 最初の子 | 幅1の探索窓 | 通常幅の探索窓 |
| 弟処理 | 固定 α の NWS | 固定 α の NWS |
| 弟の `g <= alpha` | 完了 | 完了 |
| 弟の `g > alpha` | node 枝刈りとして十分 | β 未満なら通常幅の探索窓での再探索 |
| α超過後の残手 | 探索不要 | 再探索後 局面が開けば再帰処理 |
| α の引数 | 値渡し | ポインタで更新 |
| 停止停止条件一覧 | 呼び出し元停止条件一覧に push | `{global_stop, local_stop}` を新規作成 |

## 13. TT、MPC、手順制御との関係

### 13.1 Transposition table

子処理は通常の `nega_alpha_ordering_nws` を呼ぶため、作業スレッド内でも TT 参照、TT 枝刈り、TT 登録が行われる。TT は全 `Search` で共有される。したがって兄弟処理同士や親探索との間で TT の副次的な情報共有は起こるが、YBWC の α や最善手を TT 経由で同期しているわけではない。

`ybwc_split_nws` 自身で enqueue 前に TT 枝刈りするコードはコメントアウトされている。移植時にここを有効化すると処理数や探索ノード数が変わり、現行 Egaroucid と同じ実装ではなくなる。

### 13.2 MPC

子処理の NWS 本体は通常どおり MPC を行う。これも enqueue 前の `ybwc_split_nws` で先に行う案はコメントアウトされている。`mpc_level` と `is_presearch` は子 `Search` へ引き継がれる。

### 13.3 手順の並べ替えと ETC

YBWC に入る前に手順の並べ替えは完了しており、降順 sort 済みである。`n_legal` も ordering 評価中に計算されている。ETC や既探索手は `flip.flip = 0` なので弟ノード探索関数が skip する。

弟ノード探索関数内にある `swap_next_best_move` はコメントアウトされている。処理が `move_idx` を保持するため、投入後に swap を再導入してはいけない。

### 13.4 終局探索

`is_end_search` が true でも YBWC 処理の入口は同じ `nega_alpha_ordering_nws` である。深さが終局側の切替条件以下になると、その関数が `nega_alpha_end_nws` へ委譲する。YBWC 層が終局専用処理型を持つわけではない。

## 14. 停止通知、完了待ち、変数の寿命

この実装で最も重要な条件は、ポインタを共有したローカル bool が生存中に全処理の完了を待つことである。

```text
local n_searching を作る
  -> pointer を stop_chain と全 task closure へ渡す
  -> task 投入・親の直列探索
  -> 全 future.get()
  -> stop_chain から pointer を外す
  -> n_searching の scope を抜ける
```

この順番を崩し、α超過時に非同期結果を完了を待たず切り離すしたまま弟ノード探索関数を return すると、作業スレッドが破棄済み stack 変数を読む use-after-free になる。Egaroucid は停止を要求しても必ず全非同期結果の終了を待つ。

`SCORE_UNDEFINED` は「その探索値を親の上限・下限情報更新に使ってはいけない」という意味であり、「処理が存在しなかった」意味ではない。中断処理の `n_nodes` は統計へ加算する。

グローバルな探索停止は `global_searching` と外部 `searching` の両方で判定される。YBWC の停止条件一覧は後者と各 split の分割専用停止フラグを伝播する。NWS 本体は入口で `global_searching` も別途見る。

分割専用停止フラグの false は、その split 内の「枝刈りが成立した」という内部通知でもある。NWSの弟ノード探索関数は全処理を完了を待つした後に分割専用停止フラグを停止条件一覧から pop する。そのため、外部停止が起きていなければ呼び出し元へ戻った時点の `is_searching(searchings)` は再び true になり、呼び出し元は得られた枝刈り値を通常どおり TT へ登録できる。この分割内の枝刈りと探索全体のキャンセルを同じ永久停止として扱ってはいけない。

### 14.1 Lazy SMP との共存

現設定では `USE_LAZY_SMP` も true である。iterative deepening の浅い反復（概ね 主探索の深さ 10 以下。最終終局探索は除外）では、主探索 より先に同じスレッドプールへ補助探索が投入される。

補助探索用 `Search` の `use_multi_thread` は false なので、その補助探索自身は YBWC 分割しない。一方、主探索 は true のままであり、補助探索が予約しなかった空いている作業スレッドに対して YBWC 処理を投入できる。したがって浅い反復では Lazy SMP が pool を先に消費し、YBWC の `try_push` 成功数を減らすことがある。これは値の規約ではなくスケジューリングと探索量に影響する。

主探索が終了すると Lazy SMP の `sub_searching` を false にし、補助非同期結果もすべての完了を待つ。YBWC を単体で移植する場合に Lazy SMP まで必須ではないが、Egaroucid と同じ処理数や探索ノード数を再現する性能比較ではこの相互作用を含める必要がある。

## 15. 現行では無効な「分割済み処理の早期停止」

`USE_YBWC_SPLITTED_TASK_TERMINATION` は false である。これを true にすると NWSの弟ノード探索関数にだけ次の追加動作が入る。

1. まだ α以下、running 処理が 2 件以上、かつ中盤 depth 24 以上または終局 depth 28 以上なら、完了済みの非同期結果だけ 完了を待たない 回収する。
2. それでも同条件なら分割専用停止フラグを false にして残処理を停止する。
3. 全処理の完了を待つ。
4. 分割専用停止フラグを pop し、外側探索が生きていれば未完了手を同弟ノード探索関数で再探索する。

コメントは「空きを減らす代わりに 局面が増える」ことを意図している。これは現行挙動ではないので、忠実な再実装では無効のままにする。

## 16. 並行実行の時系列例

最初の子の後に A, B, C, D の四手が残り、作業スレッドが二つ空いている例を考える。`YBWC_N_YOUNGER_CHILD = 1` である。

```text
時刻   親スレッド                       作業スレッド1       作業スレッド2
----   ------------------------------  ------------------  ------------------
t0     A の局面を作り、A の処理を渡す
t1     親局面へ戻し、B の局面を作る     A を NWS
t2     B の処理を渡す
t3     親局面へ戻し、C の局面を作る     A を NWS           B を NWS
t4     空きがないため C を直列 NWS      A を NWS           B を NWS
t5     C が完了し、D の局面を作る
t6     D は末子なので直列 NWS           A を NWS           B を NWS
t7     A の完了を待って結果を受け取る    A 完了              B は完了済みでもよい
t8     B の結果を受け取る
```

もし作業スレッド B が t4 で α超過すれば `local_stop = false` となり、親の C、作業スレッド A、以降の D は停止へ向かう。B の値だけは有効で、B より後に flag を確認した A の値は `SCORE_UNDEFINED` になる。

pool が一般的な待ち行列型で A/B/C をすべて受理してしまうと、D 以外が待ち行列に溜まり、この時系列も探索量も Egaroucid と異なる。`try_push only if idle` はアルゴリズムの一部として扱う必要がある。

## 17. 同じ処理を再実装する手順

実装順は次が安全である。

1. 直列の negamax NWS と NegaScout/PVS を完成させ、同一局面で値が一致するようにする。
2. 候補手一覧に安定した 添字、着手後合法手、完了 印 を持たせる。
3. 作業スレッドごとに子局面から独立 `Search` を再構築できるようにする。
4. `Parallel_task { value, nodes, move_idx }` を実装する。
5. ancestor を含む停止 token 停止条件一覧を実装する。
6. 空いている作業スレッドがいる場合だけ成功する 完了を待たない `try_push` を実装する。
7. NWSの弟ノード探索関数を実装する。最初は nested split を無効にすると検証しやすい。
8. 作業スレッドの `use_multi_thread = child_depth > min_depth` を有効にし、nested split を許す。
9. NegaScoutの弟ノード探索関数に α以下完了、α超過候補、通常幅の探索窓での再探索、残手再帰を追加する。
10. すべての終了経路で非同期処理の完了待ちと 停止通知 の寿命を検査する。
11. ノード数を処理ごとに集計する。

最小限必要な関数の境界 は次である。

```text
try_push(search_id, closure) -> Optional<Future<ParallelTask>>
all_flags_true(stop_chain) -> bool
search_child_nws(copied_state, alpha, depth, legal, stop_chain) -> ParallelTask
search_young_brothers_nws(...)
search_young_brothers_pvs(...)
```

## 18. 正しさを保つための条件

再実装では次を検査条件または設計規約にする。

1. **最初の子を先に探索する**: 弟処理の投入前に少なくとも一つの有効な子を直列探索済みである。
2. **探索窓を固定する**: 一つの同時投入単位の全弟処理は、同時投入単位開始時の同じ α を使う。
3. **手の位置を固定する**: 非同期結果が存在する間、`move_idx` が別の手を指すような並べ替えをしない。
4. **変更可能な探索状態を共有しない**: 作業スレッドは親の盤面、評価値を戻すための履歴、探索ノード数のカウンタ を変更しない。
5. **末子を親スレッドに残す**: `remaining_after_current < 1` の子は投入せず親が探索する。
6. **登録失敗時も探索を進める**: 処理を登録できなかった場合は同じ子を即座に直列探索する。
7. **最初のα超過を採用する**: α超過処理は自分の値を保持してから兄弟停止を通知する。
8. **中断値を上下限として使わない**: 中断値を `v`、α、最善手の更新に使わない。
9. **中断した探索も数える**: 中断処理の探索ノード数も合算する。
10. **破棄する前に完了を待つ**: 停止通知、候補手一覧、親側の局所変数 を破棄する前に全処理の完了を待つ。
11. **必要な手を通常幅で再探索する**: `alpha < g < beta` の NWS 結果を確定値とみなさず通常幅の探索窓で再探索する。
12. **未完了手を再処理する**: 再探索後も `alpha < beta` なら、未完了 印 の残る手を処理する。

## 19. 実装時の注意点

ここは「原実装の忠実な説明」と「別言語で安全に再現する推奨」を分けて読む必要がある。

### 19.1 `bool*` の共有にはデータ競合がある

原実装の `searching` と `n_searching` は通常の `bool` であり、作業スレッドと親が同期なしで読み書きする。実用上は停止ヒントとして動いていても、C++標準ではデータ競合となり、動作が保証されない。同じ問題はスレッドプールの lock 外 `n_idle` 読み取りにもある。

別実装では `AtomicBool` / `std::atomic<bool>` を 緩いメモリ順序での読み書き で使えば、探索制御の意味を変えずに データ競合を除ける。停止フラグのポインタ一覧の代わりに参照カウントされた階層型の停止通知オブジェクトを使ってもよい。ただし「祖先のどれかが false なら停止」という AND 意味論は維持する。

原 C++ のバイナリ挙動まで文字通り模倣する目的で 通常の bool に戻すべきではない。安全なアトミック変数への変更は探索アルゴリズムとして同等である。

### 19.2 停止は即時ではない

探索関数は各所の入口や move loop で flag を定期確認する協調停止である。作業スレッドを強制終了しない。したがって α超過後も、既に深い関数にいる兄弟が次の定期確認点までノードを読む。

### 19.3 非同期結果は完了順には回収しない

最初の非同期結果が遅い場合、後続が完了済みでも先に処理されない。結果値自体の正しさには影響しないが、最善手の tie、停止のタイミング、ログ順、性能には影響し得る。忠実な再現では投入順に `get()` する。

### 19.4 特殊値と通常スコアを分ける

`-124`, `124`, `-126` は現スコア範囲外だから成立する。評価値の範囲を将来拡張するなら enum/variant に分離すべきである。移植時に特殊値を通常のスコア と比較しない。

### 19.5 同点時の最善手は先着順に依存する

最善手は `v < g` のときだけ更新し、`v == g` では更新しない。直列結果が先に反映され、処理結果は投入順に回収されるので、同点時の選択にもこの順序が現れる。

### 19.6 未使用値を機能だと解釈しない

`cell`、`hash_code`、`need_best_move`、`running_count`、`prev_alpha` は現行通常経路の判断に使われない。移植時にそれらから新しい枝刈りや制限を推測して追加しない。

### 19.7 include の循環は設計要件ではない

原ソースは `midsearch_nws.hpp` と `ybwc.hpp` などが相互 include し、forward declaration と `#pragma once` に依存する。これはヘッダ構成上の事情であり、再実装では `NWS/PVS core`、`YBWC scheduler`、`task types` を分離してよい。

## 20. 検証計画

### 20.1 値の一致

同じ TT 初期状態と同じ手順の並べ替えで次を比較する。

- 1 thread（実質すべて直列探索）
- 2 thread
- 最大 thread 数
- nested split 無効/有効

各局面で `value` が一致しなければならない。最善手は同値手がある場合だけ異なり得るが、忠実な回収順まで再現した場合は一致を目標にする。

### 20.2 境界深さ

必ず次を個別に記録する。

```text
中盤 child depth 5: split しない
中盤 child depth 6: その子は task 化できるが、その task 内では再 split しない
中盤 child depth 7: task 内の孫も条件次第で split できる

終局 child depth 15: split しない
終局 child depth 16: その子は task 化できるが、その task 内では再 split しない
終局 child depth 17: task 内の孫も条件次第で split できる
```

### 20.3 処理登録のケース

- 空き 0: 全手を直列探索する。
- 空き 1: 一件だけ投入され、親が残りを探索する。
- 使用数の上限到達: 空きがいても当該 search ID は投入失敗する。
- 残り一手: 空きがいても投入せず親が探索する。
- nested 作業スレッドで空き 0: 相互待ちせず直列探索する。

### 20.4 枝刈りのケース

- 親の直列探索が α超過する。
- 最初に投入した作業スレッドが α超過する。
- 後から投入した作業スレッドが先に α超過する。
- 外部停止と α超過がほぼ同時に起こる。
- NegaScout で `g <= alpha`、`alpha < g < beta`、`g >= beta` の三ケースを作る。
- 通常幅の探索窓での再探索後に 局面が開き、未処理兄弟を再帰処理するケースを作る。

### 20.5 変数の寿命とデータ競合の検査

- ThreadSanitizer で 通常の bool を使わない実装に race がないこと。
- 弟ノード探索関数の全終了経路で outstanding 処理が 0 であること。
- 停止通知 と候補手一覧の参照が処理完了まで有効であること。
- 処理の探索ノード数合計と全体探索ノード数の差分が一致すること。

### 20.6 実行記録で比較すべき項目

デバッグ時は各 split に ID を振り、次を記録すると差を特定しやすい。

```text
split_id, parent_depth, child_depth, alpha, beta,
move_idx, move, push_success, worker_id,
result_value, undefined, nodes,
local_stop_writer, research, recursion
```

## 21. 実装チェックリスト

最後に、Egaroucid と同じ制御になっているかを短く確認する。

- [ ] 中盤は子 depth 6 以上、終局は子 depth 16 以上でのみ分割する。
- [ ] 最初の子を直列探索してから弟を投入する。
- [ ] 手順は弟ノード探索関数前に降順 sort し、処理を登録した後は 添字 を動かさない。
- [ ] 空いている作業スレッドがないと `try_push` は失敗する。
- [ ] 投入失敗した子は親が直列 NWS する。
- [ ] 最後の一手は処理にせず親が探索する。
- [ ] 処理は着手後局面から独立 `Search` を作る。
- [ ] 処理の探索窓は親側 `[alpha, alpha + 1]` で固定する。
- [ ] α超過処理は値を有効にしたまま兄弟停止を通知する。
- [ ] 停止後に終了した他処理は `SCORE_UNDEFINED` 扱いにする。
- [ ] 非同期結果は投入順にすべて `get()` する。
- [ ] 中断処理を含む全探索ノード数を親へ足す。
- [ ] NegaScout の `alpha < g < beta` は通常幅の探索窓で直列再探索する。
- [ ] 再探索後も 局面が開いていれば未完了手を再帰処理する。
- [ ] この分割専用の停止通知 は全処理完了を待つ後まで生存させる。
- [ ] nested split の許可は `child_depth > min_depth` で決める。
- [ ] 通常ビルドでは 分割済み処理の早期停止 を無効にする。

このチェックリストをすべて満たせば、言語やスレッドプールの API が異なっても、Egaroucid の現行 YBWC が持つ探索順、分割判断、α超過処理、再探索、停止伝播を再現できる。
