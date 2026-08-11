# Edax の YBWC 実装詳解

この文書は、Edax のソースを再参照しなくても、同じ並列探索を別のプログラムへ実装できる粒度で YBWC（Young Brothers Wait Concept）を説明する。

対象は `/home/azure/dev/edax-reversi` の commit `713a434f13b3d15fb69c61b9ba3641aed82c496c`（2024-12-18、Edax 4.6）である。対象ワークツリーには未追跡の補助ディレクトリがあるが、`src` 以下の参照ファイルはこの commit と一致する。

## 読む前に：用語はこれだけ

本文では、ソースコード上の名前と説明用の日本語を区別する。まず次の意味だけ押さえれば読み進められる。

| この文書での表現 | 意味 |
|---|---|
| 局面、親、子 | 探索木上の一つの盤面を「局面」と呼ぶ。一手進んだ局面が「子」、一手前が「親」である。 |
| 最初の子 | 手順の並べ替えで最善と予想された子。YBWCでは必ず先に直列探索する。elder brother / eldest child に相当する。 |
| 残りの子 | 最初の子以外の候補手。並列化の対象であり、young brothers に相当する。 |
| 共有分割点 | 複数のスレッドが共同で読む `Node`。現在の α、β、最善手、次に渡す候補手、実行中の子探索を持つ。 |
| 主探索 | 共有分割点を作った側の探索。ソース中の master に相当する。 |
| 子探索 | 主探索から分けられた独立 `Search` による探索。ソース中の slave に相当する。 |
| 作業スレッド | 子探索を実行する常駐スレッド。ソース中の `Task` 一つにつき一つ存在する。 |
| NWS（幅1の探索窓） | 「値が α を上回るか」を判定する探索。Null Window Search の略。 |
| α以下 / α超過 | 子の結果 `g` が `g <= α` なら α以下、`g > α` なら α超過。一般的な fail-low / fail-high と同じ意味である。 |
| 通常幅の探索窓 | αとβの間を探索して値を確かめ直す探索。NegaScout/PVSで必要になる。 |
| 停止理由 | `Search.stop` に入る値。通常実行、並列探索内だけの停止、時間切れ、外部要求による停止などを区別する。 |

Edax の `Node` は盤面そのものではない。盤面は各 `Search` が個別に持ち、`Node` は複数の探索を調整する共有情報である。

## 1. 参照ソース

中心となるファイルは次の通りである。行番号は上記 commit に対するもの。

| ファイル | 主な範囲 | 役割 |
|---|---:|---|
| `src/ybwc.h` | 25-109 | `Task`、`Node`、`TaskStack` の定義 |
| `src/ybwc.c` | 64-97 | 共有分割点の初期化と破棄 |
| `src/ybwc.c` | 117-144 | 待機中の祖先スレッドを手伝い役にする処理 |
| `src/ybwc.c` | 171-207 | 分割条件と子探索の開始 |
| `src/ybwc.c` | 216-254 | 子探索の停止、完了待ち、主探索の再開 |
| `src/ybwc.c` | 265-291 | 最善値と α の共有更新 |
| `src/ybwc.c` | 301-360 | 候補手を一件ずつ安全に配る処理 |
| `src/ybwc.c` | 366-445 | 作業スレッドが行う NWS/PVS と完了処理 |
| `src/ybwc.c` | 461-480 | 常駐作業スレッドの待機ループ |
| `src/ybwc.c` | 488-675 | 作業スレッドと空き処理一覧の生成・回収 |
| `src/midgame.c` | 448-550 | NWS局面からYBWCを呼ぶ処理 |
| `src/midgame.c` | 569-699 | PVS局面からYBWCを呼ぶ処理 |
| `src/root.c` | 350-432 | ルート局面のYBWC処理 |
| `src/search.c` | 522-555 | 子探索用 `Search` の複製 |
| `src/search.c` | 1319-1343 | 子孫を含む停止通知と状態変更 |
| `src/search.h` | 53-127 | `Search` の盤面、親子関係、停止状態、ノード数 |
| `src/settings.h` | 177-184 | 分割深さ、残手数、同一分割点の子探索数 |
| `src/const.h` | 76-83 | 停止理由 `Stop` |

## 2. この実装の要点

Edax は、候補手ごとに一回限りの処理を作る方式ではない。一つの共有分割点を作り、主探索と最大三つの子探索が、そこから次の候補手を順番に受け取る。

```text
候補手を良さそうな順に並べる
  |
  +-- 最初の子を主探索が直列探索
  |      |
  |      +-- ここで枝刈りできれば分割しない
  |
  +-- 残りの子について分割を試す
         |
         +-- 空き作業スレッドあり -> 子探索を一つ追加
         |
         +-- 空きなし -> 主探索がその子を直列探索
         |
         +-- 各子探索は一手で終了せず、共有分割点から次の手も受け取る
         |
         +-- αがβ以上 -> 主探索と不要な子探索を止める
         |
         +-- 主探索は全子探索の終了を待つ
```

Egaroucid版との最も大きな違いは、作業スレッドが一つの手だけを担当するのではなく、探索を終えるたびに同じ共有分割点から次の手を取得することである。候補手の割り当ては動的に変わる。

もう一つの特徴は、子探索を待っている主探索スレッドを眠らせたままにしないことである。別の子探索がさらに深い場所で分割したい場合、待機中の祖先スレッドがその深い分割を手伝える。

## 3. 設定値と分割の有効化

設定値は次である。

```c
#define SPLIT_MIN_DEPTH 5
#define SPLIT_MIN_MOVES_TODO 1
#define SPLIT_MAX_SLAVES 3
```

意味は次の通り。

- 現在の共有分割点の `depth` が 5 以上でなければ分割しない。
- `n_moves_todo` が 1 以上でなければ分割しない。
- 同じ共有分割点に同時登録できる子探索は最大 3 個である。

`options.n_task` は主探索を含む総スレッド数である。`TaskStack` は要素 0 を主探索に使い、要素 1 以降を常駐作業スレッドにする。そのため、利用できる追加作業スレッド数は `options.n_task - 1` である。

```c
search->allow_node_splitting = (search->tasks->n > 1);
```

総スレッド数が 1 ならYBWCは完全に無効になる。コマンド設定の `n-tasks` は 1 以上、実行環境の最大スレッド数以下へ制限される。

### 3.1 `SPLIT_MIN_MOVES_TODO = 1` の正確な意味

`n_moves_todo` は、最初に候補手数で初期化される。次の手へ進むたびに一つ減るため、現在渡されている手を含む残数として働く。

名前に反して、複数スレッドが動き始めた後の `n_moves_done` は「探索完了した手の数」ではなく、共有カーソルが通過して各スレッドへ割り当てた手の数に近い。`n_moves_todo` も厳密な未完了処理数ではなく、共有カーソルより後ろに残る手を数えるための値である。最初の子については、主探索がその探索を終えて `node_next_move()` を呼ぶまで `n_moves_done == 0` なので、YBWCの最初の子を直列にする条件として正しく働く。

設定値が 1 なので、残りがちょうど一手でも分割条件を満たす。ソースのコメントは「最後の手を分割しない」と説明しているが、現在値 1 では最後の一手も作業スレッドへ渡せる。最後の数手を主探索へ残したい場合は、この定数を 2 以上にする必要がある。

## 4. 三つの中心データ構造

### 4.1 `Node`：共有分割点

`Node` は複数スレッドが共同更新する調整用構造体である。

```c
typedef struct Node {
    Search *search;                         // 主探索
    Search *slave[SPLIT_MAX_SLAVES];        // 実行中の子探索
    Node *parent;                           // 一つ外側の共有分割点
    Move *move;                             // 次の候補手を指す位置
    Task help;                              // 待機中の主探索が使う一時処理
    mtx_t mutex;
    cnd_t condition;
    int bestmove;
    int bestscore;
    int alpha;
    int beta;
    int n_slave;
    int depth;
    int height;
    int n_moves_done;
    int n_moves_todo;
    bool is_helping;
    bool pv_node;
    bool stop_point;
    bool is_waiting;
} Node;
```

共有するものは α、β、最善値、最善手、候補手を配る位置、実行中の子探索一覧である。盤面、評価状態、再帰の作業領域は共有しない。

`beta` は初期化後に変えない。`alpha` と `bestscore` は結果が届くたびに単調増加する。`move` は候補手一覧を前へ進むだけで、前の手へ戻らない。

### 4.2 `Task`：一つの常駐作業スレッド

`Task` はスレッド、条件変数、再利用する `Search` をひとまとめにする。

```c
typedef struct Task {
    Search *search;
    Node *node;
    Move *move;
    TaskStack *container;
    thrd_t thread;
    mtx_t mutex;
    cnd_t condition;
    bool loop;
    bool run;
} Task;
```

`run == false` の間は条件変数で待機する。`node_split()` が `node`、`move`、複製済み `search` を設定し、`run = true` にして起こす。一つの子探索が終わると同じ `Task` は空き一覧へ戻り、別の分割で再利用される。

### 4.3 `TaskStack`：空いている作業スレッドの一覧

```c
typedef struct TaskStack {
    Task **stack;
    Task *task;
    mtx_t mutex;
    int n;
    int n_idle;
} TaskStack;
```

名前どおり後入れ先出しである。`task_stack_get_idle_task()` は末尾から一件取り出し、空きがなければ `NULL` を返す。待ち行列へ処理を蓄積しないため、空きがない場合は主探索が直列探索へ戻る。

ファイル冒頭のコメントには FIFO とあるが、ヘッダと実装は FILO/LIFO である。探索結果には影響しないが、同じ作業スレッドの再利用順まで合わせる場合は後入れ先出しにする。

## 5. 作業スレッドの生成と待機

`task_stack_init(stack, n)` は `n` 個の `Task` 領域を確保する。

```text
task[0]      主探索が現在のスレッドで使用する。新しいスレッドは作らない。
task[1..n-1] task_init() 後に thrd_create() し、task_loop() で常駐待機する。
```

各常駐スレッドは次のループを実行する。

```text
Taskのmutexを取得
loopがtrueの間:
    runがfalseなら条件変数で待つ
    runがtrueなら:
        task_search(task)
        このTaskを空き一覧へ戻す
```

作業のたびにスレッドを生成・破棄しないことが重要である。分割は探索木の多くの場所で発生するため、スレッド生成費用を毎回払うとYBWCの利点が失われる。

`task_free()` は待機ループを終了させ、スレッドを `thrd_join()` してから条件変数、mutex、専用 `Search` を破棄する。

## 6. 共有分割点の初期化

探索関数は候補手を生成した後、スタック上に `Node node` を作り、次で初期化する。

```c
node_init(&node, search, alpha, beta, depth, movelist.n_moves, parent);
```

主な初期値は次の通り。

```text
alpha / beta       呼び出し元の探索窓
bestscore          -SCORE_INF
bestmove           NOMOVE
n_moves_todo       候補手数
n_moves_done       0
n_slave            0
parent             一つ外側の共有分割点
search             主探索のSearch
is_waiting         false
is_helping         false
stop_point         false
```

`parent` は盤面上の親局面ではなく、「現在の探索を生んだ外側の共有分割点」を指す。この連鎖が、待機中の祖先スレッドを探すために使われる。

`Node` はmutexと条件変数を含むため、探索終了時に必ず `node_free()` する。ただし、その前に全子探索が終了していなければならない。

## 7. 最初の子を必ず直列探索する仕組み

`node_first_move()` は候補手一覧の先頭を返し、同時に次を初期化する。

```c
node->n_moves_todo = movelist->n_moves;
node->n_moves_done = 0;
node->move = movelist_first(movelist);
```

NWS局面では最初の手もループ内で `node_split()` へ渡される。しかし分割条件には次がある。

```c
node->n_moves_done
```

最初の手では 0 なので分割に失敗し、主探索が直列探索する。PVS局面とルート局面は、ソース構造上も最初の手をループ外で通常幅探索してから、二手目以降に `node_split()` を呼ぶ。

このため、どの呼び出し経路でも最初の子が完了する前に残りの子は並列化されない。

## 8. 分割条件 `node_split()`

分割を試す条件は五つすべての論理積である。

```c
search->allow_node_splitting
&& node->depth >= SPLIT_MIN_DEPTH
&& node->n_moves_done
&& node->n_slave < SPLIT_MAX_SLAVES
&& node->n_moves_todo >= SPLIT_MIN_MOVES_TODO
```

日本語にすると次になる。

1. 複数スレッド探索が有効である。
2. 現在の深さが 5 以上である。
3. 少なくとも最初の一手を探索済みである。
4. この共有分割点の子探索が 3 個未満である。
5. 残り候補手数が設定値以上である。

条件を満たしても、実際に手伝えるスレッドがなければ分割は失敗する。失敗時、呼び出し元は同じ手をその場で直列探索する。

成功時は次のどちらかを使う。

1. 待機中の祖先主探索を手伝い役にする。
2. それがなければ、全体の空き作業スレッド一覧から一つ取る。

この順序は重要である。眠っている祖先スレッドを先に再利用し、実際に空いている作業スレッドを他の分割へ残す。

## 9. 通常の子探索を開始する手順

空き作業スレッドを得た場合、`node_split()` は次を行う。

```text
1. task.node = 現在の共有分割点
2. task.move = 今回渡す候補手
3. search_clone(task.search, 主探索のSearch)
4. 共有分割点のmutex内で slave[] に task.search を追加
5. task.run = true
6. task.condition をsignalして作業スレッドを起こす
```

`search_clone()` の前には、主探索がまだ候補手を盤面へ適用していない。子探索用 `Search` は親局面の盤面を複製し、`task_search()` が担当手を適用する。

子探索の登録後、主探索は同じ手を探索しない。ループ末尾の `node_next_move()` で次の候補手を取得し、さらに分割または直列探索を試す。

## 10. `Search` の複製範囲

`search_clone(child, master)` は子探索専用の変更可能な状態を作る。

### 10.1 値として複製・再構築するもの

- 盤面と手番
- 空きマス一覧、評価状態、パリティなど `search_setup()` が作る盤面依存状態
- 深さ、選択率、ProbCut再帰レベル、PV延長深さ
- 時間設定、探索オプション、現在の高さと局面種別
- 安定石によるスコア上下限
- `allow_node_splitting`

子の `n_nodes` と `child_nodes` は 0 へ戻す。

### 10.2 共有するもの

- 通常の置換表、PV用置換表、浅い探索用置換表
- `TaskStack`
- 表示用コールバック
- 最終結果 `Result`
- 最上位の主探索を指す `master`

### 10.3 親子関係

主探索の `Search.child[]` へ子 `Search` を追加し、子の `parent` を主探索へ向ける。この更新は主探索のspin lockで守る。

この親子関係は探索値の共有ではなく、停止通知と探索ノード数の回収に使う。`search_stop_all()` はこの木を再帰的にたどり、子孫探索をまとめて停止する。

## 11. 候補手を重複なく配る仕組み

主探索とすべての子探索は、同じ `Node.move`、`n_moves_done`、`n_moves_todo` を使う。

次の手を取得する本体は次である。

```c
if (node->move && node->alpha < node->beta && !node->search->stop) {
    ++node->n_moves_done;
    --node->n_moves_todo;
    move = node->move = move_next(node->move);
} else {
    move = NULL;
}
```

主探索は `node_next_move()` を使い、関数内でmutexを取得する。子探索は既にmutexを持って結果更新をしているため、lockless版をそのまま呼ぶ。

mutex内でポインタを一度だけ前へ進めるため、同じ候補手が二つのスレッドへ渡ることはない。先に探索を終えたスレッドが次の手を取得するので、候補手数がスレッド数より多くても動的に負荷分散できる。

ここで `n_moves_done` が増えるのは、直前の手の探索完了を全体として確認した時ではなく、共有カーソルを次へ進めた時である。別スレッドが担当中の手もあり得るため、この値を「全スレッドで完了済みの手数」として使ってはいけない。

次のどれかが成立すると、新しい手は配られない。

- 候補手をすべて配り終えた。
- `alpha >= beta` となり枝刈りが成立した。
- 主探索に停止理由が設定された。

## 12. 作業スレッドの探索 `task_search()`

作業スレッドは、一つの手を探索して終了するとは限らない。共有分割点から手が返らなくなるまで次を繰り返す。

```text
while 担当手があり、子Searchが停止していない:
    共有alphaを読み取る
    alpha >= beta なら終了

    子Searchへ担当手を適用
    g = 幅1の探索窓で探索

    alpha < g < beta なら:
        通常幅の探索窓で再探索

    子Searchの盤面を元へ戻す
    共有分割点のmutexを取得
        結果が最善なら bestscore / bestmove / alpha を更新
        必要なら主探索へ停止を通知
        次の候補手を取得
    mutexを解放
```

実際の呼び出しは次である。

```c
move->score = -NWS_midgame(search, -alpha - 1, node->depth - 1, node);
if (alpha < move->score && move->score < node->beta) {
    move->score = -PVS_midgame(search, -node->beta, -alpha,
                              node->depth - 1, node);
}
```

NWS局面では `node->beta == node->alpha + 1` なので、整数スコアに `alpha < g < beta` は存在しない。したがって通常幅の再探索は起こらない。

PVS局面とルート局面ではβまで幅があるため、α超過かつβ未満の手を通常幅で再探索する。`node->pv_node == true` のassertionは、この経路がPVS局面だけで起きることを確認している。

## 13. NWS局面からの呼び出し

`NWS_midgame()` は置換表、ProbCut、手順の並べ替え、ETCを処理した後に共有分割点を作る。

```text
for 最初の手から順に:
    node_split() を試す
    失敗した場合:
        主探索が幅1の探索窓でその手を探索
        node_update() で共有結果を更新

全候補手を配り終えたら node_wait_slaves()
```

最初の手でも `node_split()` は呼ばれるが、`n_moves_done == 0` によって必ず直列になる。

NWSの探索窓は `[alpha, alpha + 1]` である。どれか一手がαを上回ればβ以上になるため、残りの兄弟を停止してよい。

## 14. PVS局面とルート局面からの呼び出し

### 14.1 内部PVS局面

`PVS_midgame()` は最初の手をループ外で通常幅探索する。共有 `alpha` を更新した後、二手目以降で分割を試す。

分割に失敗した手は、主探索がまずNWSする。結果が `alpha < g < beta` なら同じ手を通常幅で再探索する。これは作業スレッド側と同じ処理である。

### 14.2 ルート局面

ルートの処理も同じだが、次の追加条件がある。

```c
depth > search->options.multipv_depth
```

MultiPVを求める深さでは各手の値が必要になるため、YBWC分割を行わない。また、その範囲では現在αではなく `SCORE_MIN` をNWS基準に使う。

ルートでは各手の探索費用、進捗表示、時間延長、安定石による値の制限、候補手一覧の並べ直しも行う。これらはYBWCの分割規則ではないが、同じルートループ内にある。

## 15. 最善値とαの共有更新

主探索は `node_update()`、子探索は `task_search()` 内の同等処理で共有結果を更新する。どちらも `node->mutex` 内で次を行う。

```text
探索が停止しておらず、今回のscoreがbestscoreより大きい場合:
    bestscore = score
    bestmove = move
    必要ならルートの表示情報を更新
    score > alpha なら alpha = score
```

同点では更新しないため、先に登録された手が最善手として残る。

主探索側の `node_update()` は、更新後に `alpha >= beta` なら実行中の全子探索へ停止通知を送る。停止通知は各子 `Search` の子孫にも再帰的に伝わる。

共有αは単調に上がる。各スレッドは一手の探索開始時にαを読み取り、その手の探索中はその値を使う。探索中に他スレッドがαを上げても、既に始まった探索窓は途中変更しない。結果を共有へ戻す時点で、新しい `bestscore` と比較する。

## 16. 子探索が主探索を止める仕組み

子探索が `node->alpha >= node->beta` にした場合、同じ共有分割点を作った主探索は別の手を深く探索中かもしれない。不要な探索を早く止めるため、子探索はmutex内で次を行う。

```c
if (node->alpha >= node->beta && node->search->stop == RUNNING) {
    node->stop_point = true;
    node->search->stop = STOP_PARALLEL_SEARCH;
}
```

`STOP_PARALLEL_SEARCH` は時間切れや外部停止とは異なる。この共有分割点内で枝刈りが成立したため、主探索を一時中断するという意味である。

主探索の再帰関数は `search->stop` を定期確認し、値を早く返す。主探索が共有分割点へ戻ると、全子探索の終了を待つ。

## 17. 子探索の停止と完了待ち

`node_wait_slaves()` は三段階で動く。

### 17.1 不要な子探索を止める

次のどちらかなら、登録済みの全子探索に `STOP_PARALLEL_SEARCH` を再帰通知する。

```c
node->alpha >= node->beta || node->search->stop
```

### 17.2 全子探索の終了を待つ

`n_slave > 0` の間、共有分割点の条件変数で待つ。子探索は終了時に自身を `slave[]` から外し、`n_slave` を減らし、同じ条件変数をbroadcastする。

条件変数待ちはmutexを一時解放し、起床時に再取得する。したがって子探索は待機中でも共有分割点を更新できる。

### 17.3 主探索を再開する

全子探索が終了し、次を両方満たす場合だけ主探索の停止理由を `RUNNING` へ戻す。

```c
node->search->stop == STOP_PARALLEL_SEARCH
&& node->stop_point
```

`stop_point` は、この共有分割点の子探索が主探索を止めた時だけtrueになる。時間切れ、思考中止、外側の分割による停止はここで解除しない。この区別が、局所的な枝刈り通知を安全に元へ戻す鍵である。

## 18. 待機中の祖先スレッドを手伝い役にする

Edax固有の重要な最適化が `get_helper()` である。

ある子探索が深い局面で分割しようとした時、まず `node->parent` から外側の共有分割点をたどる。次を満たす祖先を探す。

```text
祖先が子探索の終了待ちである
祖先がまだ別の手伝いをしていない
祖先自身に実行中の子探索がある
```

見つけた場合、その祖先 `Node` に埋め込まれた一時 `Task help` を初期化する。現在の深い局面から `Search` を複製し、深い共有分割点の `slave[]` へ登録してから、祖先の条件変数をbroadcastする。

待機中の祖先スレッドは起きると、通常の待機を続ける代わりに自分で `task_search(&node->help)` を実行する。終了後に一時 `Task` を破棄し、元の子探索待ちへ戻る。

```text
祖先の主探索 ── 子探索Xの終了待ち
                       |
                       +-- Xの深部で新しい分割が必要
                              |
                              +-- 待機中の祖先を起こす
                                      |
                                      +-- 祖先が深い分割を手伝う
                                      +-- 完了後、Xの終了待ちへ戻る
```

この処理により、主探索と子探索の依存関係で眠っているCPUを、さらに深い分割へ再投入できる。通常の空き一覧より先に祖先を探すのは、この待機CPUを優先的に活用するためである。

一時 `Task help` は新しいスレッドを作らない。待機していたスレッド上で同期実行するため、`task_init()` はするが `thrd_create()` はしない。`task_free()` でも `loop == false` なのでスレッド完了待ちは発生しない。

## 19. 子探索の完了処理と探索ノード数

`task_search()` が担当手をすべて処理すると次を行う。

```text
1. 子Searchの状態を STOP_END にする
2. 親Searchのspin lockを取得
3. 親Search.child[] から自分を外す
4. 自分と子孫が探索したノード数を親の child_nodes へ加算
5. 共有分割点のmutexを取得
6. task.run = false
7. Node.slave[] から自分を外し n_slave を減らす
8. Node.condition をbroadcast
```

探索ノード数は `search->n_nodes + search->child_nodes` で数える。子探索がさらに分割した場合、その孫の探索量は子の `child_nodes` に集約され、最後に主探索へ足される。

`Search.child[]` と `Node.slave[]` は似ているが役割が異なる。

| 一覧 | 役割 | 保護するlock |
|---|---|---|
| `Search.child[]` | 停止通知の伝播と探索ノード数の親子集計 | 親 `Search.spin` |
| `Node.slave[]` | 一つの共有分割点で動く子探索の停止と完了待ち | `Node.mutex` |

## 20. 同期規則

忠実な再実装では、どの共有値をどのlockで守るかを固定する。

| 共有対象 | 主な操作 | 保護 |
|---|---|---|
| `Node.alpha`, `bestscore`, `bestmove` | 結果の反映 | `Node.mutex` |
| `Node.move`, `n_moves_done`, `n_moves_todo` | 次の候補手の取得 | `Node.mutex` |
| `Node.slave[]`, `n_slave` | 子探索の登録・解除 | `Node.mutex` |
| `Node.is_waiting`, `is_helping`, `stop_point` | 待機と手伝い、局所停止 | `Node.mutex` を中心に使用 |
| `TaskStack.stack`, `n_idle` | 空き作業スレッドの取得・返却 | `TaskStack.mutex` |
| `Task.run` と起床通知 | 作業開始と待機 | `Task.mutex` と `Task.condition` |
| `Search.child[]`, `n_child` | 子探索の登録・解除 | `Search.spin` |
| `Search.stop` と子孫停止 | 停止状態 | `Search.spin` を使う関数がある |

lockを同時に取る主な順序は次である。

- 通常分割: 空き一覧のmutexを解放してから `Node.mutex`、その後 `Task.mutex`。
- 子探索完了: 親 `Search.spin` を解放してから `Node.mutex`。
- 祖先の手伝い: 祖先 `Node.mutex` を持ち、深い `Node.mutex` を一時取得する。

別実装ではこの順序を文書化し、逆順取得を作らない。特に祖先手伝い機能は複数の共有分割点に触れるため、通常分割より慎重な設計が必要である。

## 21. 実装上の注意点

### 21.1 一部の共有読み取りはlock外で行われる

原実装は多くの更新をmutexやspin lockで守るが、すべての読み取りが同じlock内にあるわけではない。

- `node_split()` は `n_slave` などをlock外で条件判定する。
- `task_search()` は `node->alpha` や停止状態をlock外で読む箇所がある。
- `Search.stop` はlock付き関数で書く経路と直接代入する経路が混在する。

C11のメモリモデルでは、別スレッドが同時更新する通常の整数やenumを同期なしで読むとデータ競合になり得る。別実装では、同じmutexで読み書きするか、適切なatomic型を使用する。探索アルゴリズムの意味は保ちつつ、原実装の未保証動作まで模倣しない。

### 21.2 `Node` の寿命は全子探索より長くする

各 `Task` は `task->node` をポインタで参照する。`node_wait_slaves()` が戻る前に `Node` を破棄してはいけない。正しい順序は必ず次である。

```text
候補手の配布を終了
-> 不要な子探索を停止
-> n_slave == 0 まで待つ
-> 共有結果を保存
-> node_free()
-> Nodeの有効範囲を抜ける
```

### 21.3 候補手一覧の寿命も維持する

`Task.move` と `Node.move` は呼び出し元スタック上の `MoveList` 内を指す。全子探索が終わる前に候補手一覧を破棄または並べ替えてはいけない。

### 21.4 古い宣言とコメントをそのまま実装要件にしない

`ybwc.h` にはこの `ybwc.c` で実装されていない `node_stop_slaves`、`task_help`、`task_update`、`task_stack_stop`、`task_stack_clear`、`task_stack_count_nodes` の宣言が残る。現行YBWCの通常経路はこれらを呼ばない。

また、ファイル冒頭の FIFO という記述と実際のLIFO、最後の手を分割しないというコメントと設定値1の挙動に差がある。再実装では実行コードを基準にする。

### 21.5 αは探索中に変わり得る

各手は開始時点のαでNWSする。別スレッドが後からαを上げても、その手を新しいαで自動再探索しない。返ってきた値が共有 `bestscore` 以下なら更新されず、正しさはαβ探索の上下限として維持される。この「開始時にαを写す」動作も再現する。

## 22. 忠実な擬似コード

### 22.1 主探索側

```text
function search_node(search, alpha, beta, depth, moves, parent_split):
    shared = Node(search, alpha, beta, depth, len(moves), parent_split)

    move = first_move(shared, moves)
    while move exists:
        if try_split(shared, move):
            // 子探索がこの手を担当する
        else:
            score = search_one_move_on_master(search, move, shared.alpha, beta)
            update_shared_result(shared, move, score)

        move = next_move(shared)

    stop_and_wait_all_children(shared)
    result = shared.bestscore
    destroy(shared)
    return result
```

PVS局面では最初の手を通常幅で直列探索してから、二手目以降をこのループへ入れる。

### 22.2 分割処理

```text
function try_split(shared, move):
    if parallel_disabled: return false
    if shared.depth < 5: return false
    if shared.n_moves_done == 0: return false
    if shared.n_slave >= 3: return false
    if shared.n_moves_todo < 1: return false

    if wake_waiting_ancestor_as_helper(shared.parent, shared, move):
        return true

    task = take_idle_task()
    if task does not exist:
        return false

    task.node = shared
    task.move = move
    clone_search(task.search, shared.search)

    lock(shared)
        shared.slave.append(task.search)
    unlock(shared)

    wake(task)
    return true
```

### 22.3 子探索側

```text
function run_child(task):
    shared = task.node
    child = task.search
    move = task.move
    child.stop = shared.search.stop

    while move exists and child is running:
        alpha_snapshot = shared.alpha
        if alpha_snapshot >= shared.beta: break

        score = NWS(child, move, alpha_snapshot, shared.depth - 1)
        if alpha_snapshot < score < shared.beta:
            score = PVS(child, move, alpha_snapshot, shared.beta,
                        shared.depth - 1)

        lock(shared)
            if child is running and score > shared.bestscore:
                shared.bestscore = score
                shared.bestmove = move
                shared.alpha = max(shared.alpha, score)

                if shared.alpha >= shared.beta and master is running:
                    shared.stop_point = true
                    master.stop = STOP_PARALLEL_SEARCH

            move = take_next_move_without_relocking(shared)
        unlock(shared)

    detach_child_search_from_parent(child)

    lock(shared)
        task.run = false
        shared.slave.remove(child)
        signal_all(shared.condition)
    unlock(shared)
```

## 23. 検証計画

### 23.1 探索値

- 1スレッドと複数スレッドで値が一致する。
- PVS、NWS、ルート探索の各入口で一致する。
- α以下、`alpha < g < beta`、β以上の三種類を作る。
- MultiPVの対象深さでは分割しない。

### 23.2 分割境界

- depth 4では分割しない。
- depth 5では最初の子の完了後に分割できる。
- 最初の子は空きスレッドがあっても直列になる。
- 同じ共有分割点の子探索は3個を超えない。
- `n_moves_todo == 1` でも現在設定では分割できる。
- 空き作業スレッドがなければ主探索が直列探索する。

### 23.3 動的な候補手配布

- 一つの子探索が複数の候補手を順に取得できる。
- 同じ候補手が二つのスレッドへ渡らない。
- αがβ以上になった後、新しい候補手を配らない。
- 主探索停止後、新しい候補手を配らない。

### 23.4 停止と再開

- 主探索がβ以上を見つけた場合、全子孫へ停止が届く。
- 子探索がβ以上を見つけた場合、主探索が `STOP_PARALLEL_SEARCH` で中断する。
- その停止を発生させた共有分割点だけが主探索を `RUNNING` に戻す。
- 時間切れや外部停止を誤って解除しない。
- 全終了経路で `n_slave == 0` になってから `Node` を破棄する。

### 23.5 祖先の手伝い

- 祖先が待機中なら、全体の空き一覧より祖先を優先する。
- 同じ祖先が同時に二件を手伝わない。
- 手伝い終了後、祖先は元の子探索待ちへ戻る。
- 手伝い先の `n_slave` と `Search.child[]` が正しく減る。

### 23.6 データ競合と探索ノード数

- ThreadSanitizer等で共有α、停止状態、子探索数のデータ競合を検査する。
- 子探索が再分割した場合も探索ノード数を一度だけ親へ足す。
- `Search.child[]` と `Node.slave[]` の登録数が終了時に0へ戻る。

## 24. 再実装チェックリスト

- [ ] 最初の子を必ず直列探索する。
- [ ] depth 5以上でだけ分割する。
- [ ] 同じ共有分割点の子探索を最大3個にする。
- [ ] 空き作業スレッドがない場合は直列探索へ戻る。
- [ ] 作業スレッドを探索ごとに作らず、常駐・再利用する。
- [ ] 盤面と評価状態は子 `Search` ごとに分離する。
- [ ] α、β、最善値、候補手を配る位置は共有分割点に置く。
- [ ] 候補手の取得と共有結果の更新を同じmutexで守る。
- [ ] 子探索は一手の終了後、同じ共有分割点から次の手を取得する。
- [ ] PVS局面で `alpha < g < beta` なら通常幅で再探索する。
- [ ] β以上になったら不要な子探索と主探索を停止する。
- [ ] 並列探索内だけの停止と、時間切れ・外部停止を区別する。
- [ ] 停止を発生させた共有分割点だけが主探索を再開する。
- [ ] 全子探索の終了前に `Node` と候補手一覧を破棄しない。
- [ ] 待機中の祖先を深い分割の手伝いへ再利用する。
- [ ] 子探索の親子関係を使って停止を子孫へ伝える。
- [ ] 子孫の探索ノード数を重複なく親へ集計する。
- [ ] 原実装の同期されていない共有読み取りは安全な同期へ直す。

## 25. Egaroucid版との違い

| 項目 | Edax | Egaroucid |
|---|---|---|
| 分割の単位 | 共有 `Node` に複数スレッドが参加 | 候補手ごとに独立した非同期処理 |
| 候補手の配布 | 終了したスレッドが次の手を動的取得 | 親が候補手ごとに処理を登録 |
| 同一分割点の子探索数 | 最大3 | 空きスレッド数に依存 |
| 末子の扱い | 設定値1では分割可能 | 親スレッドへ残す |
| αの共有 | `Node.alpha` を更新 | 一回の処理へ値コピー |
| 最善値の共有 | mutex内で即時更新 | 親が非同期結果を回収して更新 |
| 主探索の停止 | 子探索が `STOP_PARALLEL_SEARCH` を設定 | 分割専用停止フラグをfalseにする |
| 候補手一件後 | 同じ子探索が次の手を取得できる | 処理は一件で終了 |
| 待機中スレッド | 祖先を深い分割の手伝いへ再利用 | 非同期結果を待つだけ |
| 完了待ち | 条件変数と `n_slave` | `std::future::get()` |

両者とも「最初の子を先に直列探索し、その結果で更新したαを使って残りの子を並列探索する」というYBWCの原則は共通である。異なるのは、残りの子をどう配り、どこでαと停止状態を共有するかである。
