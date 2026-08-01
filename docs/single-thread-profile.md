# 単スレッド探索のプロファイル

## 測定条件

- FFO #43 (23空き) を level 60、1 スレッドで solve
- `valgrind --tool=callgrind --collect-atstart=no --toggle-collect='*Solver*solve*'`
  で `Solver::solve` の中だけを収集した
- ビルドは `RUSTFLAGS="-Ctarget-cpu=native"` 相当 (valgrind が AVX-512 を
  実行できないため `-Ctarget-cpu=x86-64-v3`。AVX2 の flip / moves は有効)
- 収集した命令数 17,223,676,865

`--collect-atstart` を指定しないと、評価関数 JSON (50MB) の serde_json による
読み込みが全体の 63% を占めてしまい探索が埋もれる。必ず `--toggle-collect` を使う。

## 結果

| 関数 | 命令数 | 割合 |
|---|---:|---:|
| `nws_final_simple_impl` | 7.41 G | **43.0%** |
| `board::stability::get_stability` | 1.89 G | **11.0%** |
| `negaalpha_final_impl` (再帰) | 1.56 G | 9.1% |
| `eval::feature_indexes::FeatureIndexes::refresh` | 1.55 G | **9.0%** |
| `leaf::solve_score_2_empties` | 1.50 G | 8.7% |
| `negaalpha_final_impl` | 1.11 G | 6.5% |
| `leaf::solve_score_3_empties` | 1.02 G | 5.9% |

## SIMD の有効性

`flip.rs` / `moves.rs` の AVX2 実装は `#[cfg(target_feature = "avx2")]` による
コンパイル時分岐であり、実行時判定は無い。`-Ctarget-cpu=native` を付けない
ビルドではスカラー版が使われる。

FFO40-45、1 スレッドでの比較。ノード数は完全に同一 (295,670,532)。

| ビルド | 時間 | NPS |
|---|---:|---:|
| RUSTFLAGS なし | 18.05秒 | 17.1 M |
| `-Ctarget-cpu=native` | 10.63秒 | 29.8 M |

**SIMD の有無で 1.75 倍**の差がある。ベンチマークでは必ず native ビルドを使う。

逆アセンブルでの確認方法:

```bash
objdump -d --no-show-raw-insn <binary> | grep -cE '\bvpsrlvq\b'
```

native ビルドでは 687、RUSTFLAGS なしでは 0 になる。

同じ方法で edax (`-march=native`) を調べると `vpsrlvq` 1045 / `vpsllvq` 447 で、
deft の 687 / 589 と同程度。SIMD 化の度合いに大きな差は無い。

## 試したが効果が無かった変更

### get_stable_edge の SSE2 化

edax は左右辺の 8 マスを 1 byte に詰める処理を `_mm_movemask_epi8` で行う。
deft は乗算とシフト (`pack_a1a8` / `pack_h1h8`) で行っている。
edax と同じ SSE2 実装へ置き換えたが、FFO40-45 の 1 スレッド 5 回計測の中央値で
10.673 秒から 10.841 秒となり改善しなかった。

### EDGE_STABILITY の LazyLock deref 削減

`get_stable_edge` は `LazyLock` のテーブルを 4 回 deref している。
一度だけ deref して参照を使い回すようにしたが 10.616 秒で、誤差の範囲だった。
LLVM が既に共通部分式として除去していると思われる。

### get_stable_by_contact の AVX2 化

安定石の伝播ループを 4 方向同時に回す Egaroucid と同じ形にしたが、
1 スレッドの時間は変わらなかった。詳細は `docs/egaroucid-comparison.md`。

いずれも採用していない。

## nws_final_simple_impl (43.0%) の内訳

`-Cdebuginfo=2` (`CARGO_PROFILE_RELEASE_DEBUG=2`) を付けて行情報を出し、
`callgrind_annotate --auto=yes` で分解した。43.0% の内訳は次の3つ。

| 由来 | 全体に対する割合 |
|---|---:|
| `nws.rs` のソース行 | 13.6% |
| `core_arch/x86/avx2.rs` (flip / moves の SIMD がインライン展開されたもの) | 10.7% |
| `core/num/uint_macros.rs` (`count_ones` / `trailing_zeros` など) | 2.8% |

`nws.rs` の行別では手順付けが大きい。

| Ir | 全体% | 行 |
|---:|---:|---|
| 338 M | 1.97% | `let region = 1 << (((move_num >= 32) as i32) * 2 + ...)` |
| 263 M | 1.53% | `let score = -i32::from(mobility) * 18 + i32::from(parity & region != 0) * 17;` |
| 167 M | 0.97% | `if move_list[i].score > move_list[best_index].score` (選択ソート) |
| 150 M | 0.87% | `child_moves.count_ones() + (child_moves & CORNER).count_ones()` |
| 113 M | 0.66% | `while moves != 0` (手の列挙) |

**手順付けだけで全体の約 5.3%** を占める。残りは合法手生成 (`child_board.moves()`)
と flip で、これは SIMD 化済みであり削りにくい。

### 採用した改善: region の表引き化

`region` の式は `leaf.rs` の `QUADRANT_ID` テーブルと完全に同じ値を返していた。
テーブル参照に置き換えたところ、FFO40-45 の 1 スレッド 5 回計測の中央値で
10.886 秒から 10.682 秒になった (約 1.9%)。

## 未着手の候補

### FeatureIndexes::refresh (9.0%)

終盤の手順付けは、ノードごとに `FeatureIndexes::from_board(&board.passed())` で
パターン特徴量を全再計算している (11 パターン × 4 回転 × 最大 10 マス)。
子局面へは `child_from_swapped` で差分更新しているので、ノード内では既に
最適化されているが、**ノードをまたいだ差分更新は行っていない**。

edax は `search_update_midgame()` の中で `eval_update()` を呼び、
探索の make/unmake に合わせて評価状態を増分更新している。
deft の探索は `&Board` を値で渡し unmake を持たないため、
累積状態を再帰に通すには広範囲の変更が必要になる。

Egaroucid も同じく増分更新で、さらに終盤手順付け用には 4 グループ中
1 グループしか更新せず、専用の小さい重みで評価する。
これが Egaroucid との残り約 6% の差の主因と見ている。
詳細は `docs/egaroucid-comparison.md`。

### nws_final_simple_impl (43.0%)

終盤 6〜13 空きの主ループ。ここが最大だが、内訳は手順付け・local TT・
MPC・再帰が混ざっており、単一の改善点は見えていない。
callgrind の `--separate-callers` や `--dump-instr=yes` で
さらに分解する必要がある。

### solve_score_2_empties / solve_score_3_empties (合計 14.6%)

2〜3 空きの専用ソルバー。既にパリティ順と `NEIGHBOUR` による早期判定が
入っており、edax の `solve_2` / `solve_3` と構造は同等である。
