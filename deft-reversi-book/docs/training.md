# Book を継続して育てる手順

ここでの「育成」とは、固定した評価モデルを使って局面を事前探索し、Book の候補と分岐を増やす作業を指す。ニューラルネットやパターン評価の重み更新ではない。モデル自体を再学習した後に Book を更新する場合は、互換性を確認したうえで `reanalyze` を使う。

## 1. バイナリとモデルを準備する

CLI は `deft-reversi-cli` パッケージに含まれている。

```sh
cargo build --locked --release -p deft-reversi-cli
target/release/deft-reversi-cli book --version
```

通常はモデル固有の MPC 校正を持つ DPR2 モデルを使う。v1 由来で校正のないモデルを選択探索へ使うと拒否される。旧 main 向けの校正規則を明示的に使う場合だけ、生成系コマンドの各呼び出しへ `--legacy-main-mpc` を付ける。

以下の例にある `MODEL` は実在するモデルファイルのパスへ置き換える。出力先は新しいディレクトリへ作り、モデルのハッシュ、バイナリ版、feature、コマンド、実行時間を運用記録として残しておくと後から照合しやすい。

## 2. 候補を小さく広く集める

```sh
target/release/deft-reversi-cli book build --model MODEL \
  --work openings.dbook-work --book openings.dbook \
  --levels 2,4 --max-plies 4 --loss 3 --cumulative-loss 6 --leaf-loss 3 \
  --schedule breadth --seconds 60 --threads 1 --tt-mib 256 --memory-mib 512
```

未登録候補をまとめて探索し、最良手の点評価とほか候補の上限を保存する。損失制限を満たす手を登録し、PV に沿って子を伸ばしながら残り候補も処理する。全手の個別点評価は強制しない。

`--max-plies` は各開始局面からの登録距離であり、探索の先読み深さではない。パスは着手距離を増やさない。`--levels` は候補を指した子局面で解決され、保存品質は親基準へ換算する。たとえば子の深さ 2 は通常、親から見ると深さ 3 の候補評価になる。

`build` は既存の `.dbook-work` の上書きを拒否する。最初から長時間の育成を始めるのではなく、まず小さい予算で保存と再開が動くことを確かめる。

```sh
target/release/deft-reversi-cli book inspect --work openings.dbook-work
target/release/deft-reversi-cli book verify --work openings.dbook-work
target/release/deft-reversi-cli book verify --book openings.dbook
```

## 3. 同じ仕事を続ける

```sh
target/release/deft-reversi-cli book resume --model MODEL --work openings.dbook-work \
  --book openings.dbook --seconds 300 --threads 4 --tt-mib 512
```

`resume` は保存されたレベル・損失・登録距離・方針と、集合探索の記録・PV の優先仕事を引き継いで続行する。時間・ノード予算、探索スレッド数、TT 容量は変更できるが、`--levels` や `--loss` などの生成条件を変えることはできない。

Ctrl-C を押すと実行中の探索を停止し、その終了後にワークスペースとスナップショットを保存する。強制終了や電源断では、最後の正常なチェックポイント以降の計算が失われる。

## 4. 有力進行を伸ばす

```sh
target/release/deft-reversi-cli book extend --model MODEL --work openings.dbook-work \
  --book openings.dbook --levels 4,8 --max-plies 12 \
  --loss 2 --cumulative-loss 6 --leaf-loss 2 --schedule promising --seconds 300
```

`extend` は生成設定を置き換えて処理を進める。同じモデル識別値（`AnalysisIdentity`）で必要品質を満たす完成済みの直接値は再利用する。旧形式の完了済み候補は読み込み中に取り込まれ、設定や仕事一覧を変更しても評価は失われない。未完了の目標を完成扱いにすることはない。

損失制限を狭めても古いリンクは削除しない。現在の有力経路から外れた枝は自動強化の対象から外し、のちに再び有力になれば再開できるよう保持する。逆に許容差を大きくすると候補数と保存容量が急増し得るため、`inspect` と予算制限を併用する。

`--black-loss` と `--white-loss` で片側の手番だけ損失幅を広くすることもできる。共通の `--loss` より色別指定が優先される。CLI で設定を引き継いだ際、`--loss` を変えただけでは既存の色別指定は消えない。解除が必要な場合は Rust API で `None` へ設定するか、CLI で両色へ新しい共通値を明示する。

## 5. レベルを上げて終盤を解析する

```sh
target/release/deft-reversi-cli book extend --model MODEL --work openings.dbook-work \
  --book openings.dbook --levels 8,12,16 --max-plies 20 --seconds 300
```

同じレベルでも残り空きマス数に応じ、中盤深さ・選択的終盤・完全読みに切り替わる。大きいレベル番号というだけで「証明済み」「高品質」と判断してはいけない。

- 完全読みが終わると `solved`（確定）として保存する。
- 選択的終盤は終局深さまで読んでも推定（`Estimate`）のままであり、選択率と実深さを記録する。
- 最良の点評価の確定と、ほか候補がそれ以下である確定がそろえば親局面も確定済み（`solved`）になる。ほか候補は個別点評価でなく確定上限でもよい。

登録距離（`--max-plies`）を下げて再設定することもできる。既存グラフを削除せず、その範囲内の育成へ計算を集中できる。モデルを変えない通常のレベル変更で、保持している確定情報を再探索する必要はない。

## 6. 改善を親へ戻す

子を育てた結果は自動で親へ反映される。以前の直接探索が深かったことを理由に更新を止めることはない。品質の説明も現在使う値に合わせる。

```sh
target/release/deft-reversi-cli book repair --work openings.dbook-work --book openings.dbook
```

`repair` は探索を行わず、保存情報から現在の値を作り直す。詳細は [子からの更新](propagation.md) を参照。

## 7. 新しい分岐の前に末端を再計算する

```sh
target/release/deft-reversi-cli book leaves --model MODEL --work openings.dbook-work \
  --levels 8 --schedule promising --seconds 60
target/release/deft-reversi-cli book resume --model MODEL --work openings.dbook-work --seconds 60
```

`leaves` は保存グラフの未登録候補だけを再探索する。この再計算パスでは休止中の登録枝もたどり、1 手・累積損失で選別しない。新しい局面・リンクは追加しない。強い既存解析を弱い値で上書きせず、必要なら新旧両方の品質を満たす探索へ引き上げる。

末端の評価が改善すると、通常育成で新しく有力になる分岐も変わる。次に `extend` で通常育成へ戻し、登録距離と損失条件を指定して分岐を追加する。`leaves` の再計算設定で処理を続けたいときは `resume` を使う。

## 8. 任意局面から育てる

`--positions FILE.obf` を `build` や設定変更時に渡すと、任意の局面群を開始局面にできる。OBF ファイルは 1 行に 64 文字の盤面と手番 `X`/`O` を書く形式である。開始 `Context` は絶対手番色を保持する。一方、盤面の正規化は手番側・相手側の相対表現で行うため、同じ形の盤面を重複保存はしない。

パスで絶対色が切り替わる。複数の開始局面から合流しても、距離と累積損失をそれぞれ満たす実在の経路を保持する。異なる経路の最短距離と最小損失だけを組み合わせた架空の資格は作らない。

## 9. 対局へ投入する

```sh
target/release/deft-reversi-cli book export --work openings.dbook-work --book openings.dbook
target/release/deft-reversi-cli book verify --book openings.dbook
```

`BookPlayer` は探索を呼ばず、利用できる登録手がないと `None` を返す。呼び出し側はそのとき通常の `Engine` または `EngineWorker` で探索する。既定の `MovePolicy::Best` は、利用条件を満たす全候補の最善評価に並ぶ登録手から選ぶ。`RandomWithinLoss` は未登録候補も含む基準の最善値から許容損失内の登録手を選ぶ。

Book の有効性を確認する対局では、Book 外へ出る率、Book 最終手の品質、相手が未登録手を選んだ後の探索への移行も記録する。保存された評価値の改善を、そのまま棋力（Elo）の向上と読み替えないこと。
