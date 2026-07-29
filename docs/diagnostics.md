# 探索の診断機能

環境変数を設定すると、探索中の統計を stderr へ出力する。
いずれも設定されていなければ何も出力せず、探索速度にも影響しない。

| 環境変数 | 内容 |
|---|---|
| `DEFT_SEARCH_TRACE` | 探索段階、ルート各手の値・ノード数、TT情報 |
| `DEFT_MPC_STATS` | MPCの試行数とカット数 |
| `DEFT_STABILITY_STATS` | stability cutの試行数とカット数 |
| `DEFT_YBWC_STATS` | YBWC split数とabort数 |
| `DEFT_TT_STATS` | 置換表の排他制御でどれだけ競合したか |

出力は `solve()` 1回につき1行。

```bash
DEFT_TT_STATS=1 ./deft-reversi-cli \
  -s deft-reversi-cli/problem/fforum-40-59.obf \
  -e data/eval/eval.bin --threads 4 -l 60
```

## DEFT_TT_STATS

```text
TTSTATS probe=2706315 saw_writer=0 (0.0000%) exhausted=0 (0.000000%) | save=306895 cas_failed=4 (0.0013%) dropped=0 (0.000000%)
```

置換表は entry ごとの seqlock で保護している。読み取りは seqlock の snapshot、
書き込みは `seq` の CAS で entry を確保する。競合しても探索の正しさは壊れないが、
その代わり次のことが起こり得る。

- probe 中に writer が entry を保持していると、その entry を読めない
- store の CAS に負け続けると、探索結果を保存せずに捨てる

この2つが実際にどの程度起きているかを数える。

| 項目 | 内容 |
|---|---|
| `probe` | probe を呼んだ回数 |
| `saw_writer` | probe 中に writer が保持中の entry に遭遇し、その周回をやり直した回数 |
| `exhausted` | probe の retry 上限 (3回) に達し、hit 判定を諦めた回数 |
| `save` | entry への保存を試みた回数 |
| `cas_failed` | entry を確保する CAS に負けた回数 |
| `dropped` | retry 上限 (4回) に達し、探索結果を保存せずに捨てた回数 |

`saw_writer`、`exhausted`、`dropped` が増えていれば置換表の排他制御が
ボトルネックになっている。

### 実測値の目安

FFO40-49、level 60、Intel Xeon 4コア 2.80GHz での実測では、
4スレッドでも `saw_writer` と `exhausted` は 0、`dropped` も 0 であり、
`cas_failed` が save 30万回あたり 4 回 (0.0013%) だった。
現状の構成では置換表の排他制御は競合していない。

共有置換表を触るのは空きマス13以上のノードだけで、それ以下は
`nws_final_simple` のスレッドローカルTTを使う。そのため共有置換表の probe は
全ノードの1%未満にとどまる。

### 実装上の注意

カウンタの計上は `DEFT_TT_STATS` が設定されているときだけ行う。
有効・無効は最初に置換表を構築した時点で `Once` により確定するため、
途中で環境変数を変えても切り替わらない。

無効時は読み取り専用の `AtomicBool` を relaxed load して分岐するだけなので、
probe / store の実行速度には影響しない。
