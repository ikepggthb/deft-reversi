# Bookを継続して育てる手順

ここでいう学習は、固定した評価モデルで局面を事前探索し、bookの候補と分岐を増やす作業である。ニューラルネットやパターン評価の重み更新ではない。モデル学習を行った後のbook更新は、互換性を確認して明示的に再解析する。

## 1. バイナリとモデルを準備する

```sh
cargo build --locked --release -p deft_reversi_book --features book-cli --bin book
target/release/book --version
```

通常はモデル固有のMPC校正を持つDPR2を使う。v1由来で校正のないモデルを選択探索へ使うと拒否される。旧main向けの規則を明示的に使う場合だけ--legacy-main-mpcを生成系コマンドの各呼出しへ付ける。校正を黙って推測したり、エラーを避けるために無条件に旧校正へ切り替えたりしない。

以下のMODELは実在するモデルパスへ置き換える。出力先は新しいディレクトリへ作り、モデルのハッシュ、バイナリ版、feature、コマンド、実行時間を記録する。校正表や探索revisionを変えたら同じ解析ではなくなる。

## 2. 候補を小さく広く集める

```sh
target/release/book build --model MODEL \
  --work openings.dbook-work --book openings.dbook \
  --levels 2,4 --max-plies 4 --loss 3 --cumulative-loss 6 --leaf-loss 3 \
  --schedule breadth --seconds 60 --threads 1 --tt-mib 256 --memory-mib 512
```

未登録候補をまとめて探索し、最良手の点評価と他候補の上限を保持する。損失制限を満たす手を登録し、PVに沿って子を伸ばしながら残り候補も処理する。全手の個別点評価は強制しない。

max_pliesは各開始局面からの登録距離であり、解析の先読み深さではない。パスは着手距離を増やさない。levelsは候補を指した子で解決され、保存品質は親基準へ換算する。例えば子の深さ2は通常、親から深さ3の候補評価になる。

buildは既存workの上書きを拒否する。初回から長い育成を始めず、保存と再開を確認する。

```sh
target/release/book inspect --work openings.dbook-work
target/release/book verify --work openings.dbook-work
target/release/book verify --book openings.dbook
```

## 3. 同じ仕事を続ける

```sh
target/release/book resume --model MODEL --work openings.dbook-work \
  --book openings.dbook --seconds 300 --threads 4 --tt-mib 512
```

resumeは保存されたレベル・損失・登録距離・方針とFrontier・PVジョブを使う。時間・ノード予算、探索スレッド数、TT容量は変更できる。--levelsなどを付けて設定を変えることはできない。

Ctrl-Cは実行中の探索を停止し、その終了後に正本と必要なスナップショットを保存する。強制終了や電源断では、最後の正常なチェックポイント以降の計算は失う。保存間隔は長い候補探索を強制的に分割する期限ではない。

## 4. 有力進行を伸ばす

```sh
target/release/book extend --model MODEL --work openings.dbook-work \
  --book openings.dbook --levels 4,8 --max-plies 12 \
  --loss 2 --cumulative-loss 6 --leaf-loss 2 --schedule frontier --seconds 300
```

extendは設定を置き換えて処理する。同じidentityで必要品質を満たす完成済みの直接値は再利用する。旧pendingの完了スロットを共通の公開処理で独立証拠へ取り込んでから、設定やキューを置き換える。未完了の目標を完成扱いにしない。新方式の完了した集合探索はFrontierと独立証拠として保持する。

損失を狭めても古いリンクは削除しない。現在の有力経路から外れた枝を自動強化から外し、再び有力になれば再開できるよう保持する。許容差を大きくした場合は候補数と保存容量が急増し得る。inspectと予算を併用する。

--black-lossと--white-lossで片側だけ広くすることもできる。共通--lossより色別指定が優先される。CLIで設定を引き継いだ際、--lossを変えただけでは既存の色別指定は消えない。解除が必要ならAPIでNoneへ設定するか、CLIで両色へ新しい共通値を明示する。

## 5. レベルを上げて終盤を解析する

```sh
target/release/book extend --model MODEL --work openings.dbook-work \
  --book openings.dbook --levels 8,12,16 --max-plies 20 --seconds 300
```

同じレベルでも残り空き数に応じ、中盤深さ・選択的終盤・完全読みに切り替わる。序盤補正もDeftの計画に従う。大きいレベル番号だけで「証明済み」「高品質」と判断しない。

完全読みが終わるとsolvedとして保存する。選択的終盤は終局深さまで読んでも推定のままで、選択率と実深さを記録する。最良点評価の証明と、他候補がそれ以下である証明がそろえば親局面も証明済みになる。他候補は個別点評価でなくProvenUpperでもよい。

登録距離を下げて再設定することもできる。既存グラフを削除せず、その範囲内の育成へ計算を集中する。モデルを変えない通常のレベル変更で、保持した証明を再探索する必要はない。

## 6. 改善を親へ戻す

子を育てた結果は自動で親へ反映する。以前の直接探索が深かったことを理由に更新を止める方式は廃止した。品質の説明も、現在使う値に合わせる。

```sh
target/release/book repair --work openings.dbook-work --book openings.dbook
```

この操作は探索せず、保存情報から現在の値を作り直す。旧`--propagation`は使わない。[更新規則](propagation.md)を参照。

## 7. 新しい分岐の前に末端を再計算する

```sh
target/release/book leaves --model MODEL --work openings.dbook-work \
  --levels 8 --schedule frontier --seconds 60
target/release/book resume --model MODEL --work openings.dbook-work --seconds 60
```

保存グラフの未登録候補だけを再探索する。この修復パスでは休止中の登録枝もたどり、1手・累積損失で選別しない。新しい局面・リンクは追加しない。強い既存解析を弱い値で上書きせず、必要なら新旧両方の品質を満たす探索へ引き上げる。

末端の評価が改善すると、通常育成で新しく有力になる分岐も変わる。次にextendで通常育成へ戻し、登録距離と損失条件を指定して分岐を追加する。leavesの再計算設定で処理を続けたいときはresumeを使う。

## 8. 任意局面から育てる

--positions FILE.obfをbuildまたは明示的な再設定に渡す。1行は64文字の盤面と手番X/Oで、盤面はX/O/-などOBFの表記に従う。開始Contextは絶対手番色を保持する。盤面の正規化は手番側/相手側の相対表現で行い、同じ盤面を重複保存しない。

パスで絶対色を切り替える。複数開始局面から合流しても、距離と累積損失をそれぞれ満たす実在の経路を保持する。異なる経路の最短距離と最小損失だけを組み合わせた架空の資格は作らない。

## 9. 対局へ投入する

```sh
target/release/book export --work openings.dbook-work --book openings.dbook
target/release/book verify --book openings.dbook
```

BookPlayerは探索を呼ばず、利用できる登録手がないとNoneを返す。呼出し側はそのとき通常のEngineまたはEngineWorkerで探索する。既定のBestは、適格な全候補の最善評価に並ぶ登録手から選ぶ。RandomWithinLossは未登録候補も含む基準の最善値から許容損失内の登録手を選ぶ。

bookの有効性を確認する対局では、book外へ出る率、book最終手の品質、相手が未登録手を選んだ後の探索への移行も記録する。保存された評価の改善を、そのままEloの向上と読み替えない。
