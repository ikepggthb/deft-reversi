# 終盤探索の Egaroucid との比較

Egaroucid 7.8.0 (Nyanyan) の終盤探索を読み、deft と比較した記録。
edax の比較は `docs/ybwc.md` 系にあるので、ここは Egaroucid 固有の差分に絞る。

## ベンチマーク

FFO #40-49、level 60 (完全読み)、Intel Xeon 4コア 2.10GHz。
同一セッション内で連続して測定した。deft の時間は `DEFT_PHASE_TIME` の
`solve()` 合計 (評価関数 JSON の読み込み約 0.7 秒を含まない)。
edax / Egaroucid は各自が出力する solve 時間。

| エンジン | 1スレッド | 4スレッド | 速度向上 | 1T ノード | 4T ノード | 並列時の増加 |
|---|---:|---:|---:|---:|---:|---:|
| deft (この比較の前) | 27.24秒 | 8.59秒 | 3.17倍 | 796.5 M | 897 M | +12.6% |
| **deft (現在)** | **25.1秒** | **8.27秒** | **3.04倍** | 796.6 M | 900 M | +13.0% |
| edax 4.6 | 27.00秒 | 8.15秒 | 3.31倍 | 934.0 M | 979.7 M | +4.9% |
| Egaroucid 7.8.0 | 23.74秒 | 7.85秒 | 3.02倍 | 676.3 M | 731.3 M | +8.1% |

ノード数はエンジンごとに数え方が違う (edax の `COUNT_NODES=7` は3種類の
ノードを数える) ため、**エンジン間のノード数比較は目安**にしかならない。
時間は同一機・同一問題なので直接比較できる。

読み取れること。

- 1スレッドの素の速度は Egaroucid が最も速い。deft は現在 edax より速く、
  Egaroucid より約 6% 遅い。
- 並列の伸びは 3 エンジンとも 3.0〜3.3 倍で、大きな差は無い。
  **並列化そのものはもう遅れていない**。残る差は素の探索速度側にある。
- 並列時のノード増加は deft が最も大きい (+13%)。edax の +4.9% と比べると
  投機的な探索の無駄が多い。

## 終盤探索の段階構成

空きマス数で使う関数を切り替える構造は 3 エンジンとも同じだが、境界と
各段でやることが違う。

| 空きマス | deft | Egaroucid |
|---|---|---|
| 〜5 (deft) / 〜6 (Ega) | `negaalpha_final`<br>parity 手順のみ、TT 無し | `nega_alpha_end_fast_nws`<br>**stability cut あり**、`empty1_bb` 手順 |
| 6〜13 / 7〜10 | `nws_final_simple`<br>局所TT + parity + mobility | `nega_alpha_end_simple_nws`<br>局所TT + parity + mobility |
| — / 11〜13 | (同上) | `nega_alpha_end_nws`<br>局所TT + mobility + **評価関数手順** |
| 14〜 | `nws_final`<br>共有TT + ETC + MPC + 評価関数手順 + YBWC | `nega_alpha_ordering_nws`<br>共有TT + ETC + MPC + 評価関数手順 + YBWC |

Egaroucid には **11〜13 空きに評価関数で手順付けをする段がある**。
deft はこの範囲を mobility と parity だけで並べている。
Egaroucid のノード数が少ないのは主にここが効いていると思われる。

deft がこれを真似られない理由は次節。

## 最大の構造的な差: 評価特徴量の増分更新

Egaroucid は `Search` に評価特徴量ベクタを持ち、`search->move_endsearch()` /
`undo_endsearch()` で make/unmake に合わせて増分更新する。

```cpp
inline void eval_move_endsearch(Eval_search *eval, const Flip *flip, const Board *board) {
    __m256i f2 = _mm256_sub_epi16(eval->features[eval->feature_idx].f256[2],
                                  coord_to_feature_simd[flip->pos][2]);
    for (int i = 0; i < N_SIMD_EVAL_FEATURE_GROUP; ++i) {
        f2 = _mm256_add_epi16(f2, eval_move_unflipped_16bit[~flipped_group[i] & player_group[i]][i][2]);
        f2 = _mm256_sub_epi16(f2, eval_move_unflipped_16bit[~flipped_group[i] & opponent_group[i]][i][2]);
    }
    ++eval->feature_idx;
    eval->features[eval->feature_idx].f256[2] = f2;
}
```

ポイントが 2 つある。

1. **増分更新**なので、ノードごとの特徴量の作り直しが無い。
2. 終盤手順付け用には **4 つある特徴量グループのうち 1 つ (`f256[2]`) しか
   更新しない**。評価も 2 パターン群 (corner+block cross / edge+2X triangle)
   だけを引く専用関数 `mid_evaluate_move_ordering_end` で、phase も見ない。
   重みも本体の `eval.egev2` (38MB) とは別の `eval_move_ordering_end.egev`
   (472KB) を使う。

deft は終盤手順付けのたびに `FeatureIndexes::from_board(&board.passed())` で
11 パターン × 4 回転を全再計算している。単スレッドプロファイルで
**9.0%** を占める (`docs/single-thread-profile.md`)。ノード内では
`child_from_swapped` で差分更新しているが、ノードをまたいだ差分更新は無い。

これが「Egaroucid は 11〜13 空きでも評価関数で並べられるのに deft は
できない」理由であり、残り約 6% の差の主因と考えている。

対応するには次の 2 つが要る。どちらも小さくない。

- 探索が `&Board` を値渡しで unmake を持たない構造を変え、評価状態を
  再帰に通す
- 終盤手順付け専用の小さい重み (パターン 2〜3 個) を別に学習する

`--ordering-eval` の口は既にあるので、後者だけ先に用意して
「小さい評価関数を毎ノード再計算する」形にすれば、増分更新なしでも
再計算の費用は数分の 1 になる。

## 移植した改善

### 局所TTで解決してから合法手生成する (採用、1スレッド -7.5%)

Egaroucid の `nega_alpha_end_simple_nws` は、手生成ループでは flip だけを
求め、次のループで **局所TTを引いてから** `get_legal()` を呼ぶ。
局所TTで解決した手には合法手生成の費用を払わない。

deft は手生成ループで全ての手の `child_moves` と mobility を先に求めていた。
Egaroucid と同じ順序に組み替えたところ、FFO40-49 の 1 スレッドが
27.24 秒 → 25.1 秒 (-7.5%)、4 スレッドが 8.59 秒 → 8.27 秒 (-3%) になった。
コミット `82475a0`。

## deft に既に入っていたもの

Egaroucid の終盤簡易 NWS の主要な工夫は、多くが deft にも入っていた。

| 工夫 | deft |
|---|---|
| 全消し (`flip == opponent`) の即時 `SCORE_MAX` | あり |
| 空きマス数ごとに層を分けた thread-local な局所TT | あり (2048 × 8 層) |
| 局所TTの lower/upper による手のスキップ | あり |
| 相手の着手数が 1 以下の手を手順付け中にその場で探索 | あり |
| 隅を 2 回数える mobility (`get_n_moves_cornerX2`) | あり |
| parity 17 / mobility 18 の重み | あり (Egaroucid の `W_END_NWS_SIMPLE_*` と同値) |
| 選択ソートで毎回最良手を前に持ってくる | あり |

## 効果が無かった移植 (不採用)

### 置換表の手を手順付けより先に探索する

Egaroucid の `nega_alpha_ordering_nws` は TT の最善手を先に探索し、
カットできなければ初めて残りの手を評価関数で並べる。deft に同じ構造を
入れて計測したが、1 スレッド 25.1 秒で変化しなかった。

計測すると `nws_final` の手順付けに到達するノード 871,381 件のうち、
TT に手が入っていたのは 16,924 件 (1.9%)、うちカットできたのは
10,706 件 (1.2%) だった。deft は空きマス 14 以上でしか共有TTを引かないため
そもそも母数が小さく、この最適化が効く余地が無い。

### `get_stable_by_contact` の AVX2 化

Egaroucid は安定石の伝播ループを `_mm256_srlv_epi64` / `_mm256_sllv_epi64` で
4 方向同時に回す。deft のスカラー実装を同じ形に置き換えたが、
1 スレッド 25.1〜25.6 秒で変化しなかった (ノード数は完全に同一)。
ループ回数が少なく、SIMD の準備費用と釣り合わないと思われる。

### 4〜5 空きでの stability cut

Egaroucid は `nega_alpha_end_fast_nws` (6 空き以下) の全ノードで
stability cut を行う。deft の `negaalpha_final` (5 空き以下) には無い。

| 適用範囲 | ノード数 | 1スレッド時間 |
|---|---:|---:|
| 無し (現状) | 796.6 M | 25.08〜25.54秒 |
| 4〜5 空き | 726.3 M (-8.8%) | 25.30〜25.70秒 |
| 5 空きのみ | 743.8 M (-6.6%) | 24.95〜25.44秒 |

**ノードは確実に減るが時間は変わらない**。deft の `get_stability` が
プロファイルの 11.0% を占めるほど重く、刈り取った分と相殺してしまう。
`get_stability` を速くできればここは採用できる。
なお SSE2 化 (`_mm_movemask_epi8`) と `LazyLock` の deref 削減は既に
試して効果が無かった (`docs/single-thread-profile.md`)。

## 並列探索の比較

Egaroucid の YBWC は deft / edax より**単純**で、helping も待ち行列も無い。

```cpp
inline bool push_task(thread_id_t id, const F &task) {
    if (n_idle > 0 && ...) {         // 空きスレッドがある時だけ積む
        std::unique_lock<std::mutex> lock(mtx);
        if (n_idle > 0 && ...) {
            tasks.push(...); --n_idle; condition.notify_one();
        }
    }
}
```

- キューは実質使わない。空きワーカーがいる時だけ push し、いなければ
  master が自分で直列に探索する。
- master は `std::future::get()` で**素直にブロックする**。deft の
  `join_slaves` のように待ち時間で他の仕事を実行することはしない。
- 分割点で仕事を共有する構造も無い。1 手 = 1 タスクを投げるだけ。
- 分割の下限は `YBWC_END_SPLIT_MIN_DEPTH = 16` (deft は空きマス 16、
  条件付きで 15)。ほぼ同じ。
- `ybwc_split_nws` の中の「空きスレッド確認」「同時分割数の上限」は
  いずれもコメントアウトされていて、実質 `push_task` の `n_idle > 0` だけが
  効いている。
- `USE_YBWC_SPLITTED_TASK_TERMINATION` (分割済みタスクを打ち切って
  直列にやり直す) は `false` で無効。

つまり Egaroucid は「並列化を凝ることでは勝っていない」。
実際に速度向上比は 3.02 倍で、deft の 3.04 倍、edax の 3.31 倍と同程度か
やや低い。**Egaroucid が速いのは素の探索が速いから**である。

## まとめ

- 並列化の設計はもう追いついている。Egaroucid はむしろ deft より単純。
- 1 スレッドの差 (約 6%) の主因は**終盤手順付けの評価関数の費用**で、
  Egaroucid は評価特徴量を増分更新し、終盤手順付け専用の小さい重みを
  使うことでこれをほぼ無料にしている。
- 小手先の SIMD 化や cutoff の追加では埋まらないことを実測で確認した。
  次に手を付けるなら評価特徴量の増分更新か、終盤手順付け専用の
  小さい評価関数のどちらかになる。
