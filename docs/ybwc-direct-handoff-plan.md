# YBWC 分割点ごとの直接ハンドオフ 実装方針

> **状態: 段階1〜3は実装済み**。NWS・PVS・中盤root・終盤rootのすべてが
> 共有 `SplitPoint` と直接ハンドオフを使う。段階4の上限調整も実測済み。
> 実測結果は末尾の
> [実装後の計測](#実装後の計測) を参照。

## 背景

現在の YBWC は、分割で生まれた仕事を `ThreadPool` が持つ単一の
`Mutex<VecDeque<Job>>` に積み、待機中の親が `join_helping` でそこから引く
pull 方式である。

計測すると、この「引く」経路が主要な実行経路になっている。
FFO #49、4スレッド、1局面あたり:

| 項目 | 値 |
|---|---:|
| `jobs_worker` (専用ワーカーが実行) | 1,654 |
| `jobs_master` (`join_helping` 経由で実行) | **5,133 (76%)** |
| `push_ok` | 6,787 |
| `push_fail` (キュー満杯で棄却) | 20,884 (75.5%) |
| `worker_idle` | 1.124秒 |
| `master_idle` | 0.329秒 |
| `master_help` | 5.009秒 |

edax は同じ問題を push 方式で解いている。`node_split()` は
`get_helper(node->parent, ...)` を呼び、**自分の祖先チェーンを遡って**
待機中の master を探し、見つけたらその master 専用のスロットへ仕事を
書き込んで、その master の condvar だけを起こす。
グローバルなキューも共有 mutex も存在しない。

本ドキュメントは、この方式を deft に導入するための設計と作業手順をまとめる。

## 何を解決し、何を解決しないか

先に期待値を明確にしておく。

**解決するもの**

- グローバル mutex をホットパスから外す。ジョブの76%が通る経路なので効果は
  スレッド数に比例して大きくなる。
- `master_idle` (0.329秒)。待機中の親は 50µs ごとの起床ではなく、
  仕事を渡された瞬間に起こされる。
- 局所性。親が手伝うのは必ず自分の部分木の子孫になるため、
  無関係な深い部分木を走らせて自分の join を遅らせることがなくなる。
- 分割ごとの `mpsc::channel` と `Box` の確保 (段階1で除去)。

**解決しないもの**

- `worker_idle` (1.124秒)。専用ワーカーが遊ぶのは「キューが空 = 誰も分割点に
  到達していない」ためであり、渡し方を変えても仕事が増えるわけではない。
  4コアでの遊び時間の大半はこちらなので、**4コアでの改善幅は小さい**と見込む。
- 探索の無駄 (edax と同じ +13%)。
- 中盤探索の直列性 (`DEFT_PHASE_TIME` の `iterative_deepening`)。

4コアでの期待値は数%程度。主眼は20スレッド構成で、グローバル mutex の
convoy と待機中の親の増加が効いてくる領域である。

## 設計

### HelperSlot

分割点ごとに1つ持つ受け口。edax の `Node::help` / `is_waiting` /
`Node::condition` に相当する。

```rust
/// 待機中の master に仕事を直接渡すための受け口。
pub(crate) struct HelperSlot {
    /// `state.waiting` のロック無し複製。try_offer の早期棄却に使う。
    waiting: AtomicBool,
    state: Mutex<HelperState>,
    cv: Condvar,
}

struct HelperState {
    /// master がこのスロットで待機中か。
    waiting: bool,
    /// 子孫から渡された仕事。同時に保持するのは1件だけ。
    job: Option<Job>,
    /// この分割点で完了した slave の数。単調増加。
    completions: u64,
    /// cutoff / 中断で master を起こすためのフラグ。
    closed: bool,
}
```

操作は3つ。

```rust
impl HelperSlot {
    /// 待機中の master に仕事を渡す。渡せなければ job をそのまま返す。
    fn try_offer(&self, job: Job) -> Result<(), Job>;

    /// slave が1件終わったことを master に伝える。
    fn notify_completion(&self);

    /// master が待つ。仕事を渡されたら Some、
    /// 自分の slave が終わった / 中断された場合は None を返す。
    fn wait(&self, seen_completions: &mut u64) -> Option<Job>;
}
```

`completions` は単調増加のカウンタで、master 側は `seen_completions` と
比較して判定する。更新はすべて mutex 下で行い、更新後に notify するため
通知の取りこぼしは起きない。

### 祖先チェーン

`SearchContext` に祖先の受け口を持たせる。既存の `searchings:
Vec<Arc<AtomicBool>>` と同じ伝播経路に乗せる。

```rust
pub struct SearchContext<'a> {
    /// 祖先の分割点の受け口。末尾が最も深い。
    pub(crate) helper_chain: Vec<Arc<HelperSlot>>,
    // 既存フィールド
    pub searchings: Vec<Arc<AtomicBool>>,
    ...
}
```

ワーカーが実行するジョブは、生成元スレッドの `helper_chain` に自分の分割点の
スロットを push したものを引き継ぐ。結果として、あるスレッドの
`helper_chain` は探索木におけるそのノードの祖先分割点の列に一致する。

### ハンドオフ

```rust
/// 分割点の仕事を、待機中の祖先か、居なければワーカープールへ渡す。
fn spawn_split_job(search: &SearchContext, mut job: Job) -> Result<(), Job> {
    // 近い祖先から順に試す (edax の get_helper と同じ順序)。
    for slot in search.helper_chain.iter().rev() {
        match slot.try_offer(job) {
            Ok(()) => return Ok(()),
            Err(returned) => job = returned,
        }
    }
    match &search.thread_pool {
        Some(pool) => pool.try_push(job),
        None => Err(job),
    }
}
```

渡せなかった場合は現状どおり master が自分でその手を直列に探索する。

### master の待機

`collect_ybwc_tasks` + `join_helping` を、自分の分割点のスロットで待つ形に置き換える。

```rust
fn wait_for_slaves(
    slot: &HelperSlot,
    spawned: u64,
    search: &mut SearchContext,
) {
    let mut seen = 0u64;
    while seen < spawned {
        match slot.wait(&mut seen) {
            // 子孫から渡された仕事を実行する
            Some(job) => { let _ = job(); }
            // slave が1件終わった / 中断された
            None => {
                if search.check_abort_now() {
                    break;
                }
            }
        }
    }
}
```

### ワーカープールの役割

グローバルキューは残すが、**引くのは専用ワーカーだけ**にする。
master はグローバルキューを見ない。これにより

- グローバル mutex に触るのは「祖先に空きが無かった分割」と専用ワーカーだけになる
- master が無関係な部分木を走らせることがなくなる

`ThreadPool::try_push` は完了通知が不要になるため fire-and-forget にでき、
`TaskHandle` と分割ごとの `mpsc::channel` を削除できる。

## 停止性

待ち合わせグラフが循環しないことを確認しておく。

- master は自分の分割点 (深さ d) の slave (深さ > d) の完了を待つ。
- 渡される仕事は必ず「渡した側の祖先チェーンに自分が含まれる」ノード、
  つまり深さ > d の子孫である。
- したがって「待つ」辺も「実行する」辺も深さが増える方向にしか向かず、
  待ち合わせグラフは深さで順序づけられた DAG になる。循環しない。

master が仕事を実行している間は `waiting = false` なので、
自分自身のスロットに再度仕事が入ることはない。
その間に自分の slave が終わっても `completions` が進むだけで、
`wait()` に戻った時点で検出される。

**安全網**: 外部 `stop` フラグは受け口の登録簿を持たないため、
`cv.wait_timeout` を 5ms 程度で使い、liveness の保険とする。
現状の 50µs ポーリング (毎秒2万回) に比べれば毎秒200回であり、無視できる。

## 作業手順

一度に全部やらず、各段階でスコアとベンチを確認しながら進める。

### 段階1: 完了通知を分割点へ移す (ハンドオフ抜き)

- `HelperSlot` を `completions` / `cv` / `closed` だけで導入する。
- `NwsSplitPoint` に `HelperSlot` を持たせる。
- slave はループ終了時に `notify_completion()` を呼び、
  自分の `SearchStats` を分割点の `Mutex<SearchStats>` へ加算する。
- `collect_ybwc_tasks` を `wait_for_slaves` (仕事の受け取り無し版) に置き換える。
- `ThreadPool::try_push` を fire-and-forget にし、`TaskHandle` と
  `mpsc::channel` を削除する。

この段階だけでも分割ごとのアロケーションが減り、ポーリングが消える。
期待値は中立〜わずかな改善。**ここで性能が落ちるなら設計を見直す。**

### 段階2: 直接ハンドオフ (本命)

- `HelperSlot` に `waiting` / `job` と `try_offer` を追加する。
- `SearchContext` に `helper_chain` を追加し、`make_nws_split_worker` で伝播する。
- `nws_final_ybwc` の投入を `spawn_split_job` に差し替える。
- `wait_for_slaves` で渡された仕事を実行する。
- 診断カウンタ `handoff_ok` / `handoff_fail` / `pool_push_ok` / `pool_push_fail`
  を `SearchStats` に追加する。

### 段階3: PVS とルートへ展開

`pvs_final` と `search_root_final_siblings_ybwc` は現在
`make_ybwc_job` + `TaskResult` で1手ずつ結果を受け取っている。
`NwsSplitPoint` と同様に、手ごとのスコアを共有オブジェクトの
`AtomicI32` に書く `PvsSplitPoint` を用意して同じ形に揃える。

段階1・2で NWS 側が安定してから着手する。

### 段階4: 調整

- `queue_cap` の再調整。master がキューを引かなくなるため最適値が変わる。
- `YBWC_MAX_SLAVES` (現在3、edax も3)。
- master がグローバルキューも引くべきかどうかの再評価。

## 検証

**正しさ**

- `cargo test -p deft_reversi_engine --lib`
- FFO 1-19 と 40-49 のスコアを 1 / 2 / 4 スレッドで確認する。
- デッドロック検出のため FFO40-49 を4スレッドで20回連続実行する。
- 段階2以降は `helper_chain` の伝播ミスが静かな性能劣化になるため、
  `handoff_ok` が 0 でないことを必ず確認する。

**性能**

- FFO40-49、1 / 2 / 4 スレッド、5回計測の中央値。
  現状は 29.5 / 16.5 / 10.1 秒。
- `DEFT_PHASE_TIME` で `exact_final` の速度向上を見る。現状 3.02倍。
- `DEFT_YBWC_STATS` と新カウンタで、ハンドオフ経由の割合を確認する。
  現状の `jobs_master` 76% に相当する分がハンドオフに移っていれば設計どおり。

**期待値**

4コアでは `master_idle` 0.329秒 と mutex 競合分のみなので数%。
20スレッドでの再測定が本番。効果が出なければ段階4の調整か、
`worker_idle` 側 (分割機会の生成) に軸足を移す。

## 見送るもの

計測済みで効果が無いことが分かっているため、この作業には含めない。

- 置換表の連想度・ロック方式の変更 (270万 probe で競合0件)
- 分割下限の引き下げ (15 / 14空きは中立〜悪化)
- 分割ごとのアロケーション削減を目的とした最適化
  (段階1の副産物として得られるが、それ自体を目的にはしない)

## 実装後の計測

### ハンドオフの成功率 (4コア)

`DEFT_YBWC_STATS`、FFO40-49、level 60、4スレッド。問題ごとの内訳。

| 空きマス | 投入試行 | ハンドオフ | プール投入 | 失敗 | ハンドオフ率 |
|---:|---:|---:|---:|---:|---:|
| 20 | 159 | 3 | 155 | 1 | 1.9% |
| 22 | 766 | 20 | 735 | 11 | 2.6% |
| 22 | 1,030 | 39 | 974 | 17 | 3.8% |
| 23 | 1,815 | 50 | 1,734 | 31 | 2.8% |
| 23 | 1,279 | 51 | 1,210 | 18 | 4.0% |
| 24 | 2,998 | 156 | 2,778 | 64 | 5.2% |
| 24 | 3,246 | 140 | 3,014 | 92 | 4.3% |
| 25 | 2,540 | 104 | 2,365 | 71 | 4.1% |
| 25 | 3,619 | 125 | 3,424 | 70 | 3.5% |
| 26 | 8,349 | 377 | 7,802 | 170 | 4.5% |
| **合計** | **25,801** | **1,065** | **24,191** | **545** | **4.1%** |

`handoff_ok` が 0 でないので `helper_chain` の伝播自体は動いている
(検証項目のとおり)。ただし **4コアでは 96% がプール経由**で、
段階2の狙いだった「pull を push に置き換える」効果はほとんど出ていない。

この測定当時は、4コアでは分割点の入れ子が浅く、master が
`join_slaves` の50µs待機窓に入っている確率も低かった。
`try_offer` は master が窓の中にいるときしか成立しない。

**この数字だけを見て段階2を戻すべきではない。** 期待値の節に書いたとおり
4コアでの想定効果はもともと数%であり、本番は 20 スレッドである。
入れ子が深くなるほど `helper_chain` は長くなり、待機窓に入っている
祖先が増えるため、ハンドオフ率は上がる側に動く。
後述の2026-08-04変更ではキューのポーリングを廃止して待機を5msに延ばし、
`HelperSlot`経路を全PVS/rootへ展開した。この節の50µs設定と結論は履歴として残す。

### 中盤 YBWC に空きスレッドの門番を足す案 (不採用)

`should_split_eval_ybwc` に `can_spawn_split_job()` を足し、
投入先が無いときは `nws_eval_ybwc` に入らず作業リストの確保を省く案。

初期局面 level 24、4スレッド、各14回の中央値。

| | 中央値 | 備考 |
|---|---:|---|
| 門番なし (現状) | 2.951秒 | 外れ値 6.462 秒を1件含む |
| 門番あり | 2.929秒 | |

**差は 0.7% で、この機械のノイズ幅の中**。採用していない。

`nws_eval_ybwc` は最初の手を直列に探索し終えてから作業リストを確保するので、
確保の費用は depth 8 以上の部分木全体に償却され、元から効くほどの
大きさではなかった。逆に門番を入れると、直列探索の間に空いたスレッドを
ループ内の `try_add_slaves` で拾う機会を失う。

## 2026-08-04: PVS/root統合とmasterによるキュー横取りの廃止

旧実装ではNWSだけが共有 `SplitPoint` を使い、PVSとrootは1手ごとに
`mpsc::channel` と `TaskHandle` を作っていた。全経路を共有分割点へ統一し、
旧 `Job` / `TaskResult` / `TaskHandle` / `join_helping` を削除した。

また `SplitPoint::join_slaves` で待つmasterがグローバルキューをpopする処理を
廃止した。プールへの投入はlock下でidle worker数以内に制限され、投入時点で
専用workerが予約されている。masterが横取りする必要はなく、横取りすると
無関係な部分木を実行して自分のjoinを遅らせ、共有mutexの競合も増やしていた。
完了と直接ハンドオフはcondvarで即時通知し、外部stopの安全確認だけ5ms周期で行う。

内部ノードのslave上限はEdaxと同じ3が最良だった。2は並列性不足、4は投機探索が
増えて悪化した。一方rootは候補手数までslaveを許し、プール側のidle worker制限に
任せる。rootまで3に制限すると8スレッドで遅くなったためである。

Core Ultra 7 265K、FFO40-49、level 60、各3回を旧/新で交互に実行した中央値。
1スレッドは同じ探索経路で12.230秒。

| thread | 旧時間 | 新時間 | 時間差 | 旧nodes | 新nodes | 1T比 旧→新 |
|---:|---:|---:|---:|---:|---:|---:|
| 4 | 3.825秒 | 3.754秒 | -1.9% | 839.2M | 840.4M | 3.20x → 3.26x |
| 8 | 2.333秒 | 2.278秒 | -2.4% | 880.9M | 883.4M | 5.24x → 5.37x |
| 20 | 1.738秒 | 1.656秒 | -4.7% | 1,015.2M | 982.5M | 7.04x → 7.39x |

コア数が増えるほど改善幅が大きく、今回の目的である単コア比の向上を確認した。
20スレッドでは探索量自体も3.2%減り、待機処理のNPS低下だけでなく投機探索の
抑制も効いている。

## DeftとEdaxのnodeカウンタの違い

Edax 4.6は`src/stats.h`の`COUNT_NODES=7`により、次の3種類をすべて
`Search::n_nodes`へ加算する。

- bit 0 (`SEARCH_UPDATE_INTERNAL_NODES`): PVS/NWS、終盤専用solverなどの内部探索。
- bit 1 (`SEARCH_UPDATE_EVAL_NODES`): 静的評価・パターン評価を実行した回数。
- bit 2 (`SEARCH_UPDATE_ALL_NODES`): 手順付け候補の評価など、探索関数の再帰入口とは
  一致しない汎用作業単位。

例えば`search_eval_1`は関数入口をinternal nodeとして1回数え、各合法手の静的評価を
eval nodeとしてさらに数える。`movelist_evaluate_fast`もTT手以外の候補ごとに
all nodeを加算する。2空き専用solverは、実際に調べた1空き局面までまとめて1〜3を足す。

DeftのCLIに表示する`SolverResult::nodes`は
`eval_search_leaf_nodes + final_search_nodes`である。中盤PVS/NWSの内部ノードは表示値に
含めず、静的評価リーフだけを含める。終盤はPVS/NWSと専用1〜4空きsolverが調べた
局面を`final_search_nodes`へ足す。一方、手順付け候補をEdaxのbit 2と同じ独立nodeとして
常に足すわけではない。

したがって両エンジンの絶対node数や絶対NPSは同じ仕事単位ではない。比較に使えるのは、
同じエンジン内での1 thread→N threadのnode増加率と時間倍率である。

## 2026-08-04: Edax再比較と追加調整

Core Ultra 7 265K、FFO40-49、level 60。Edax 4.6
`lEdax-x86-64-v3`との同一セッション比較では、4 threadはDeft 3.824秒 / Edax
3.914秒、8 threadは2.306秒 / 2.308秒、20 threadは1.660秒 / 1.590秒だった。
少数コアでは同等以上だが、20 threadではEdaxが4.4%速い。

14空きで分割を許すidle worker閾値を4から2へ変更した。変更前後を交互に7回測った
20 thread中央値は1.676→1.667秒 (-0.5%)、nodeは983.7M→969.6M (-1.4%)。
8 threadも約0.8%改善し、4 threadの時間差は0.2%未満だった。高コア時に14空きの
仕事を少し早く供給することで、深い部分木へ偏る前にTTを共有できたと考えられる。

以下は実装したが悪化したため採用しなかった。

- PVS分割点でalphaを共有し、slaveがfull-window再探索まで行う方式:
  20 thread中央値1.657→1.683秒。重い再探索の同時実行で投機量が増えた。
- PVS兄弟を4手ずつのwaveに制限する方式: 1.653→1.792秒前後。待ち合わせが増えた。
- EdaxのTaskStack同様のworker専用スロット: 1.657→1.701秒。スロットmutexの追加が
  現在の短い共有キューより重かった。
