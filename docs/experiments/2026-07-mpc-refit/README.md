# NNUE S2 向け MPC 再フィット実験(2026-07-14)

## 背景

MPC(multi prob-cut)の回帰係数(a/b/e_std)は旧パターン評価の誤差分布で
フィットされた値が `MpcConfig::default()` にハードコードされ、NNUE でも流用されていた。
さらにエンジンファイルの `mpc` セクションはロード時に破棄されていた(配管切れ)。

本実験で (1) 配管修正、(2) NNUE S2 用の収集→フィット→埋め込みパイプライン構築、
(3) 再フィット値の実測評価を行った。

## 成果物

- 配管: `Solver::from_file` / `Solver::from_str_data`(ファイルの mpc を Solver まで配管)
- 収集: `deft-reversi-cli mpc-collect --mode eval|final`(浅読み vs 深読みペア、
  既存 Python 互換 CSV)
- フィット: `tools/fit_mpc_nnue.py`(numpy のみ、MpcConfig JSON を直接出力、
  z プロファイル焼き込みオプション付き)
- 埋め込み: `deft-reversi-train set-mpc` / `pack-nnue --mpc`
- データ: `data/log/mpc-s2-collect/`(中盤 12,300 ペア lv4-13、終盤 2,750 ペア e12-22)
- フィット済み設定: `runs/small-S2-full-acc128/mpc-s2.json` /
  同梱 bin `nnue-eval-mpcfit.bin`

## 実測(fforum-60-79.obf, level 14, S2)

| MPC 構成 | total nodes | 時間 | 対オフ短縮 |
|---|---|---|---|
| オフ(selectivity 6) | 176.1M | 619s | 1x |
| 再フィット(z プロファイル 85→96%) | 150.1M | 192s | 3.2x |
| **旧デフォルト(採用継続)** | **62.7M** | **130s** | **4.8x** |

対戦(level 11、100 ペア 200 局、同一 S2 重み): 旧 95勝 15分 90敗(勝率 51.25%)
= **統計的に互角**。思考時間は旧 854ms/手 vs 新 1271ms/手。

## 主要な知見

1. **フィット自体は正確**: 再フィット e_std は実残差と一致(比 0.91〜1.06)。
   旧デフォルトは NNUE に対し、深い探索・高 empties では σ を最大 1.7 倍過大評価、
   **低 empties では過小評価(実効信頼度 60% 相当の過剰カット)**
2. **旧デフォルトの速さは「無謀カット」由来**: 木の大部分を占める低 empties /
   浅depth 域を真の信頼度 60〜80% で刈ることで 4.8x を達成している。
   統計的に正直な 85〜95% MPC では原理的に届かない(3.2x が上限近傍)
3. **無謀カットの棋力コストは検出されず**(level 11、200 局で互角)。
   旧プロファイルは偶然「葉に近いほど無謀、根に近いほど安全」という
   工学的に合理的な形になっており、カット誤りが手の選択まで波及しにくい
4. 「NNUE は評価誤差が小さいから MPC がよく効く」という事前予想は不成立。
   浅読みと深読みの**差分**の σ は実測 3〜4 石で、パターン評価と大差ない
   (eval-mae の差は絶対精度の差であって浅深相関の差ではない)

## 追加実験(ハイブリッド切り分け、fforum 60-62 サブセット)

| 構成 | nodes | 備考 |
|---|---|---|
| 旧 a/b/e_std | 16.4M | tries 11.9k / cuts 6.8k(問題3) |
| 新フィット一式 | 23.1M | tries 18.7k / cuts 10.9k — **切る回数は多いのに遅い** |
| 旧 a/b + 新 e_std | 23.0M | 遅い |
| 新 a/b + 旧 e_std | 24.7M | 遅い |

ハイブリッドがどちらも遅い理由: このベンチ(level 14)は empties 24〜27 の問題で
**Final ソルバー(終盤 MPC)**を通り、ハイブリッド2種はどちらも新しい final_search
回帰を含むため。旧デフォルトは終盤 MPC でも σ を過小評価(モデル 2.5〜2.7 vs
実測 3〜4.6 @ e21-22)して無謀カットしており、eval 側と同じ構図が final 側にもある。

計測用に `DEFT_MPC_STATS=1` 環境変数で solve ごとの
`mpc_tries / cuts / high / low` を stderr に出せるようにした(solver.rs)。

## プローブ深さ自動チューニング(2026-07-14、最終)

較正済み σ を維持したまま、プローブ深さ対応表を座標降下で実測最適化した
(`tools/tune_mpc_mapping.py`: 候補ごとに再フィット → モデル σ/実測 σ 比の
健全性チェック → set-mpc → 実時間ベンチ)。

- **中盤**(fforum #74-79, l14): 12.1s → 2.5s。最適解はほぼ旧マッピング
  (唯一 d10: probe 2→4)。深いプローブ(gap 4)は全域で損
  (プローブが MPC 無効の全幅探索で高価なため)
- **終盤**(fforum #60-65, l14): 243.8s → 43.3s。最適解は**旧より浅い**プローブ
  (e12/14/16→lv2、e20/21→lv3、e18→lv4、e22→lv4)
- 採用マッピング: eval `4:0,5:1,6:2,7:1,8:2,9:1,10:4,11:3,12:4,13:3` /
  final `12:2,13:3,14:2,15:3,16:2,17:5,18:4,19:5,20:3,21:3,22:4`

### 最終検証(tuned = 較正 σ + 最適プローブ)

| 検証 | 旧デフォルト | tuned |
|---|---|---|
| l14 フル20問 | 130s / 62.7M | **40.8s / 127.1M**(3.2倍速) |
| fforum 1-19 l25 | 全問正答 | 全問正答 |
| 対戦 200局 l11 | — | 98勝9分93敗(互角)、**思考時間 650ms vs 957ms/手** |

**統計的に正しい設定が、速度でも旧の誤較正設定を上回った。**
ノード数は多い(127M vs 62.7M)が、安いプローブ由来の高 nps ノードが主で実時間は 1/3。

## 結論

- **較正済み(NNUE フィット)+ プローブ最適化の設定を正式採用**
  (`runs/small-S2-full-acc128/nnue-eval.bin` に埋め込み済み。
  設定は `mpc-s2-tuned.json`)。selectivity の信頼度ラベルが実統計と一致し、
  かつ旧の誤較正設定より 3.2 倍速・棋力互角
- `MpcConfig::default()`(コード内定数)は旧パターン評価フィットのまま。
  これは mpc セクションを持たないレガシー eval.json(パターン評価)用の
  フォールバックとしては正しい値なので変更しない
- 厳密性が必要な用途(完全読みの保証)は従来どおり selectivity 6(MPC オフ)
- ファイルへの埋め込みと配管は完了しているため、将来モデルが変わったときは
  本パイプライン(mpc-collect → fit_mpc_nnue.py → set-mpc)で数時間で再評価できる
- 再フィット値は `mpc-s2.json` / `nnue-eval-mpcfit.bin` として保存
  (長時間持ち時間などカット誤りが効きうる条件で再検証する場合に使用)

## 再現

```bash
# 収集(並列実行例は data/log/mpc-s2-collect/job_*.log 参照)
cargo run -p deft-reversi-cli --release -- mpc-collect \
  --eval runs/small-S2-full-acc128/nnue-eval.bin --data egaroucid_rd_data_set \
  --mode eval --levels 4,...,13 --positions-per-bucket N --out <dir>
# フィット
python3 tools/fit_mpc_nnue.py --collect-dir <dir> --out mpc.json
# 埋め込み
cargo run -p deft-reversi-train --release -- set-mpc \
  --eval <bin> --mpc mpc.json --out <bin>
```
