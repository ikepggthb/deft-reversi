# 終盤探索高速化 作業記録

## 対象範囲

- 基準コミット: `e9618735dde54f3e7335493f93401cd19b0dcc3d`
- 基準コミット名: `feat(workspace): ✨ エンジン・評価・探索・学習基盤を全面刷新`
- 記録日: 2026-07-18
- 対象: 基準コミットから現在の作業ツリーまでの終盤探索高速化
- 差分規模（本ドキュメント追加前）: 12ファイル、812行追加、104行削除

現在の変更は、パターン評価を使用した終盤完全読みの高速化を主目的としている。NNUEへの置き換えは行っていない。

## 実施内容

### 1. 終盤リーフ探索の特殊化

- 2、3、4空きでパスが発生した場合、汎用の合法手生成と再帰を行わず、残りの空きマスを直接調べるようにした。
- 1～4空きの専用ソルバーで、null-window探索時の早期カットを維持した。
- `negaalpha_final` で合法手が1手だけの場合、汎用の手リスト反復を省略して直接子局面を探索するようにした。
- 3、4空きのパリティ順と空きマス情報をパス後も再利用するようにした。
- 完全窓のPVSで必要な正確性を保つため、専用リーフへの切り替え条件を限定した。

対象ファイル:

- `deft-reversi-engine/src/search/final_search/leaf.rs`
- `deft-reversi-engine/src/search/final_search/negaalpha.rs`

### 2. 終盤NWSの高速化

- 汎用NWSから単純終盤NWSへ切り替える閾値を10空きから13空きへ変更した。
- 5空き以下では専用nega-alphaへ切り替える構成にした。
- 単純終盤NWSにスレッドローカルTTを追加した。
  - 1層1024エントリ
  - 空き数別のレイヤー
  - 盤面のplayer/opponentをキーとして照合
  - `i8` のlower/upper boundを保持
  - Egaroucidと同様に子局面をキー、親視点のスコアをboundとして保持
  - 選択率が異なる探索結果は再利用しない
- 子局面の合法手を生成時に保持し、再帰先での再計算を削減した。
- 相手のmobilityが1以下の候補を先に探索し、早いカットを狙うようにした。
- 終盤パリティ、mobility、コーナーmobilityを組み合わせた軽量な手順付けを導入した。
- 1合法手の局面を直接探索する高速経路を追加した。
- 通常の終盤NWSではTT、ETC、MPC、stability cutを維持しつつ、Egaroucidを参考に評価探索の深さ、窓、重みを調整した。
- YBWCのタスク生成、結果回収、fail-high時の兄弟停止を整理した。

対象ファイル:

- `deft-reversi-engine/src/search/final_search/nws.rs`
- `deft-reversi-engine/src/search/move_list.rs`

### 3. 終盤PVSとaspiration探索の改善

- 終盤PVSでTT、ETC、MPC、stability cutを使用する経路を整理した。
- 選択率付き終盤探索で得た完全値を、より深い完全読みのaspiration予測値として利用するようにした。
- 予測値が窓を外した場合に必要な側だけ再探索するようにした。
- 手順評価の窓と評価値・mobilityの重みを調整した。
- 完全読みのルートでは最初の候補をPVSで探索した後、残りの兄弟候補をYBWCで並列化する。
- 内部NWSでも最初の子を先に探索してから、残りの子をYBWCで並列化する。

対象ファイル:

- `deft-reversi-engine/src/search/final_search/pvs.rs`
- `deft-reversi-engine/src/search/solver.rs`

### 4. パターン評価の差分更新

- `FeatureIndexes` に着手・反転石だけを反映する差分更新経路を追加した。
- 現局面、手番反転済み局面、着手後にさらにパスした視点のfeature indexを、盤面全体の`refresh`なしで生成できるようにした。
- ordering level 0、1では、全候補に対して親局面のindexを共有し、各候補の差分だけを適用するようにした。
- ordering level 2では、深さ1専用の評価探索を追加し、末端まで差分更新したfeature indexを渡すようにした。
- NNUE使用時は既存の汎用経路を維持し、パターン評価の場合だけ高速経路を使用する。
- 通常経路との評価値・alpha-beta境界の一致を複数局面と複数窓でテストした。

対象ファイル:

- `deft-reversi-engine/src/eval/feature_indexes.rs`
- `deft-reversi-engine/src/search/eval_search/leaf.rs`
- `deft-reversi-engine/src/search/eval_search/mod.rs`
- `deft-reversi-engine/src/search/move_list.rs`

### 5. flip SIMDの命令列短縮

Intel向けAVX2の`mm_flip`で、opponent bitboardの上位64bitを複製する処理を、unpackとbroadcastの組み合わせから`_mm256_permute4x64_epi64`へ変更した。Egaroucidの実装を参考にしている。AMD向け分岐は維持した。

対象ファイル:

- `deft-reversi-engine/src/board/flip.rs`

### 6. TT世代管理

CLIは同じ`Solver`でFFOの複数局面を連続して解くため、各`solve`開始時にTT generationを進めるようにした。以前の問題のエントリが現局面と同じ世代として置換競合することを防ぐ。

`solve_advances_tt_generation`テストを追加した。

対象ファイル:

- `deft-reversi-engine/src/search/solver.rs`

### 7. YBWCとスレッドプール

- 子手ごとにjobを生成する旧YBWC方式へ戻した。
- 親はTaskHandleを回収し、join待ちの間はキュー上の仕事を実行する。
- ワーカーの空き状態とidle worker数を管理し、空きworker数を超える投機タスクは投入しない。
- fail-high時は同じsplit pointの兄弟タスクを停止する。
- CLIの`--threads N`はmainを含む総探索スレッド数を表す。`--threads 1`はmainのみ、`--threads 20`はmainと19 workerで探索する。

masterと最大1 helperがsplit pointを共有する再設計は、NPSと総実行時間が悪化したため採用せず、`backup/ybwc-redesign-20260718`へ退避した。

対象ファイル:

- `deft-reversi-engine/src/search/thread_pool.rs`
- `deft-reversi-engine/src/search/final_search/nws.rs`
- `deft-reversi-engine/src/search/solver.rs`

### 8. abort応答性

停止確認のノード間隔を4096ノードごとから512ノードごとへ短縮した。探索停止時にバックグラウンドの探索が長く残ることを防ぐ。

対象ファイル:

- `deft-reversi-engine/src/search/search.rs`

### 9. 診断機能

以下の環境変数で探索統計を出力できるようにした。

| 環境変数 | 内容 |
|---|---|
| `DEFT_SEARCH_TRACE` | 探索段階、ルート各手の値・ノード数、TT情報 |
| `DEFT_MPC_STATS` | MPCの試行数とカット数 |
| `DEFT_STABILITY_STATS` | stability cutの試行数とカット数 |
| `DEFT_YBWC_STATS` | YBWC split数とabort数 |

## 正確性の修正・確認

変更後のFFO40～59のスコアは以下となり、比較対象のEdaxの結果と一致した。

```text
+38, +0, +6, -12, -14, +6, -8, +4, +28, +16, +10, +6, +0, -2, -2, +0, +2, -10, +4, +64
```

変更前の提示結果では、少なくともFFO41、46、50、51の値に差があった。

追加・拡充したテストには以下を含む。

- 1～4空き専用ソルバーとbrute forceの一致
- 終盤NWS/PVSとbrute forceの一致
- alpha-beta窓に対するboundの正当性
- feature index差分更新と盤面からの全再計算の一致
- 深さ1の差分評価探索と通常経路の一致
- `solve`ごとのTT generation更新

## ベンチマーク

### ビルド

```bash
env CARGO_TARGET_DIR=/tmp/deft-native-target \
  RUSTFLAGS=-Ctarget-cpu=native \
  cargo build -p deft-reversi-cli --release
```

### 実行

```bash
/tmp/deft-native-target/release/deft-reversi-cli \
  -s deft-reversi-cli/problem/fforum-40-59.obf \
  -e data/eval/eval-v4.bin \
  --threads 20 \
  -l 60
```

### 結果

| 条件 | ノード数 | 時間 | NPS |
|---|---:|---:|---:|
| 提示されたYBWC再設計前、FFO40～59（当時のmain + 20 workers） | 43,932,950,623 | 65.792秒 | 667.749M |
| YBWC再設計の修正前、FFO40～59 | 54,280,325,413 | 94.096秒 | 576.857M |
| YBWC再設計（退避済み）、FFO40～59 | 45,711,879,098 | 85.090秒 | 537.214M |
| 旧内部YBWC復元後（ルート復元漏れあり）、FFO40～59 | 48,501,003,893 | 73.460秒 | 660.229M |
| ルートYBWCを含む復元完了後、FFO40～59（当時のmain + 20 workers） | 40,254,491,756 | 62.900秒 | 639.971M |
| 現在のDeft、FFO40～53 | 5,334,696,337 | 10.865秒 | 490.989M |
| 同一マシンのEgaroucid、FFO40～53 | 約2.94B | 4.284秒 | 約686M |

FFO53の現在のDeftは3回計測で4.395、4.538、4.451秒、約591～597M NPS、約2.61～2.71Bノードだった。

復元確認時に一度`target-cpu=native`を付けずにreleaseバイナリを作成し、FFO55とFFO57のNPSが大きく低下した。これはコードの退行ではなく汎用CPU向けビルドによるものだったため、上表の復元完了後の値には含めていない。ベンチマークでは必ず上記のnativeビルド条件を使用する。

変更前に提示されたDeftのFFO53は10,307,749,808ノード、30.469秒、338.3M NPSだった。ただし、提示値は別時点のバイナリ・環境による値であり、厳密な同一条件比較ではない。

同一マシンでの1 worker相当の参考値は以下の通り。

| エンジン | ノード数 | 時間 | NPS |
|---|---:|---:|---:|
| Deft `--threads 1`（メイン+worker 1） | 2,148,209,457 | 17.141秒 | 125.3M |
| Egaroucid 1 thread | 1,107,747,819 | 16.327秒 | 67.85M |

この比較ではDeftの1ノード当たりの処理速度は高い一方、探索ノード数が約1.9倍ある。残る主要課題はSIMDの命令速度よりも、手順、TT再利用、枝刈りによる探索木の縮小である。

### Edaxとのスレッド数別比較

`--threads`をmain込みの総探索スレッド数へ統一した後、FFO40、45、50、53、57を同じマシンで比較した。

| エンジン | 総スレッド数 | ノード数 | 時間 | NPS |
|---|---:|---:|---:|---:|
| Deft | 1 | 7,714,167,616 | 120.228秒 | 64.163M |
| Edax | 1 | 5,271,016,699 | 73.925秒 | 71.302M |
| Deft | 20 | 11,185,324,978 | 21.154秒 | 528.751M |
| Edax | 20 | 5,988,759,323 | 7.402秒 | 809.073M |

空き28のFFO53まではDeftのノード数がEdaxより少ないが、空き30のFFO57では1スレッド時にDeft 4.565B、Edax 1.350Bへ逆転する。20スレッド時はDeft 7.457B、Edax 1.759Bであり、1スレッドからのノード増加率はDeft 1.63倍、Edax 1.30倍だった。

FFO57の1スレッドtraceでは、事前の中盤探索と選択的完全読みが約50Mノード、正確読みが約4.65Bノードだった。最初の正確読みがfail-lowした後、次のaspiration反復開始時に最初のルート子のTT entryが失われており、その子だけで約993Mノードを再探索した。Edaxはmain TTとは別にPV tableとshallow tableを持ち、ルート各手のboundも保持している。空き数の多い完全読みでは、Deftの単一TTとroot bound未保持が再探索増加の主要候補である。

## 試したが採用しなかった変更

以下はベンチマーク悪化、ノード増加、または効果が確認できなかったため元に戻した。現在の作業ツリーには含まれない。

- 単純終盤NWSの切り替えを6空きにする案
- 単純/通常NWSの切り替えを12または14空きにする案
- 11～13空き、または13空きだけに静的パターン手順評価を入れる案
- 空き領域だけを使った手順付け、残り3空き専用の追加手順付け
- Egaroucid式のworker予約と、join中に親が仕事を手伝わない方式
- リーフでSIMD flipをscalar flipへ置き換える案
- `should_split`でidle workerを必須にする案
- YBWC jobの遅延生成、投入失敗時の親による直接再帰
- TT手を手順評価から除外する案
- Egaroucid式の`eval_depth = n_empties >> 4`
- TT先頭手を手順付け前に完全窓で探索する案
- 正確読み前にselectivity 4の追加探索を行う案
- NWS改善候補のPVS再探索窓を`[-beta, -g]`まで狭める案
- TTを既定256MiBから1024MiBへ増やす案

静的パターン手順評価を11～13空きに入れた再実験では、FFO53のノード数は約2.439Bまで減ったが、実行時間が4.875秒まで悪化した。現状ではノード削減量より評価コストが大きい。

## 検証状況

- `cargo test -p deft-reversi-engine --lib`: 95件中92件成功、3件ignored、失敗0件
- native release build: 成功
- `git diff --check`: 問題なし
- 計測終了後にDeft、Egaroucid、Edaxの探索プロセスが残っていないことを確認済み

ビルド時には以下の警告が残っている。

- `flip_std`がdead code
- `mm_flip`の引数`OP`がsnake_caseではない

## 現在の評価と次の課題

終盤リーフ、パターン評価差分更新、YBWCの実行効率により、変更前より大幅に高速化した。一方、同一マシンのEgaroucidに対してFFO40～53の合計時間とノード数はまだ多い。

ルートtraceでは、FFO53の最初の最善手D8の部分木だけで約957Mノード、残りの兄弟手の証明に約1.13Bノードを使用していた。次の優先事項は以下である。

1. ルートおよび上位数plyの手順精度を改善し、兄弟手のnull-window証明を短縮する。
2. Egaroucid/EdaxとTTの格納条件、置換方針、対称局面利用、ETC適用位置を比較する。
3. パターン評価を適用する局面を固定空き数ではなく、分岐数やTTヒット状況から選び、評価コストを回収できる場合だけ使う。
4. root各手のboundをaspiration反復間で保持し、main TTから追い出されても同じ窓を再探索しないようにする。
5. main TTとは独立した小さなPV tableを追加し、ルート近傍とPV情報を保護する。
6. FFO40～59全体を固定CPU affinity、固定TTサイズ、同一ビルド条件で再計測する。
