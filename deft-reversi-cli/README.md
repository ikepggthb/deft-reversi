# deft-reversi-cli

端末から対局・局面解析・棋譜の検討・自己対戦・perft・GTPを使うアプリケーション。

## 起動

```sh
cargo build --locked --release -p deft-reversi-cli
export DEFT_MODEL=path/to/model.dpr2
./target/release/deft-reversi-cli play
```

既定は人が黒、AIが白、レベル10。`f5` のように座標を入力して着手する。`help` で操作一覧、`quit` またはEOFで終了する。

モデルはDPR2形式。環境変数の代わりに各コマンドで `--model FILE` を指定できる。`--threads N` は呼び出し元を含むスレッド数で、省略すると利用可能な全論理CPU。`--tt-mib N` は主置換表の要求容量（既定256 MiB）で、実際の主表容量は2の累乗に切り下げる。例えば3 MiB指定では2 MiB。補助表のメモリは別に必要になる。

## 探索設定を簡単に指定する

レベルだけなら `--level 10`。個別に指定する場合は **`--eval 深さ@MPC確率` と `--endgame 確率:空きマス数,...`** を使う。

```sh
./target/release/deft-reversi-cli play --mode 3 \
  --eval 12@83 --endgame 95:24,99:22,100:18 --time-ms 1000
```

| 空きマス数 | この例で要求する探索 |
|---|---|
| 25以上 | eval searchを12手、MPC83% |
| 23〜24 | 終局まで、MPC95% |
| 19〜22 | 終局まで、MPC99% |
| 18以下 | 完全読み（100%） |

`--time-ms 1000` は1回の探索につき最大1秒。設定はあくまで要求であり、時間制限・Ctrl-Cで中断した場合の結果には実際に完了した深さや停止理由を表示する。`--time-ms 0` と対局中の `time 0` は、どちらも時間制限なし。

使えるMPC段階は **83・85・95・98・99・100%**。100%は確率的な枝刈りを使わない設定で、MPCの数値は勝率ではない。83〜99%には校正付きモデルが必要。終盤設定は「指定した空きマス数以下から使う」という意味で、複数に該当すれば最も高い確率を使う。`100:18` が完全読みの開始位置。`95:0` は95%の段階を無効にする。

`--eval 12` のように確率を省略することもできる。レベルなしの個別設定は「eval search 10手・100%、終盤切替なし」が基になり、指定箇所だけを上書きする。`--level 24 --eval 12@83 --endgame 100:18` とすれば、レベル24の設定が基になる。`--endgame` を繰り返す場合、同じ確率は最後の指定が有効。

細分化した `--eval-depth 12 --eval-mpc 83` も引き続き利用できる。ただし `--eval` と同時には使えない。固定深さ・100%だけなら `--depth N`、完全読みだけなら `--exact`。`--depth`・`--exact` は個別設定との同時指定をエラーにする。

これらの探索オプションは **play・solve・review・self-playで共通**。

## 現在の設定を確認・変更する

対局画面には常に設定と現在の局面で使う段階を表示する。

```text
Search: eval 12@83; endgame 95:24,99:22,100:18; time 1000 ms/search
Now (26 empties): eval depth 12, MPC 83%
```

対局中は、起動時と同じオプションを `search` に渡せる。

```text
search
search --eval 12@83 --endgame 95:24,99:22,100:18 --time-ms 1000
search --level 10
search --time-ms 0
```

`search` 単独は設定確認。個別設定を一括指定すると起動時と同じ基準から構成し直すため、前の終盤設定を暗黙には引き継がない。時間だけの変更は探索設定を保持し、時間を省略すると現在の時間制限を保持する。入力に誤りがあれば設定を変更しない。

`settings` はさらにスレッド数、要求TTと実際の主表容量、読み込んだモデルのSHA-256と校正の有無、定石、実行バイナリのSHA-256、ビルド情報を表示する。モデルのハッシュは実際に読み込んだバイト列から計算する。

従来の段階別操作も使える。

```text
endgame clear
eval 12@83
endgame 95 24
endgame 99 22
endgame 100 18
```

`eval 12 83` も同じ意味。対局中の `eval` は既存の終盤段階を保持する。`endgame clear` で全段階を無効にし、`level 10` で標準設定に戻す。`depth N` は終盤切替なしの固定深さ・100%、`exact` は完全読みに切り替える。スレッド数やモデル自体の変更は起動し直して行う。

## 対局・局面解析

| 操作 | 内容 |
|---|---|
| `f5` | 着手。強制パスは自動で処理 |
| `play f5d6c3` | 棋譜を現在位置から続けて打つ。不正な棋譜は全体を適用しない |
| `undo` / `redo` | 戻す／やり直す。人対AIでは人の手番まで戻す／やり直す |
| `go` | 今の手番をAIに打たせる |
| `new` / `init` | 初期盤面に戻し、履歴・TTをクリア。探索設定は保持 |
| `mode 0` / `1` / `2` / `3` | 人が黒／人が白／AI同士／人同士 |
| `hint` | 最善手・評価・深さ・PVを表示。着手しない |
| `analyze` | 全候補手の評価値とPVを表示。着手しない |
| `review` | 現在の棋譜を検討。着手しない |
| `review f5d6c3` | 指定棋譜を初期盤面から検討。現在の対局は変更しない |
| `search` / `settings` | 探索設定／詳細な実行設定を表示 |
| `help` / `quit` | 操作一覧／終了 |

`hint`・`analyze`・`review` の評価は手番側から見た石差。結果の `d12@95%` は深さ12・MPC95%、`end@99%` は終局まで・MPC99%、`exact` は完全読み。推定値と完全読みを区別する。

解析中のCtrl-Cは解析を停止して入力待ちに戻り、盤面・棋譜・undo/redo履歴を維持する。全候補の評価が揃わない場合、候補の順位を表示しない。`analyze` の候補手PVは追加探索し、評価値と探索条件が一致したものだけ表示する。時間不足や中断等の場合は `unavailable`。追加PVの取得を含めて解析開始から時間制限を共有する。校正不足等で対局中の検索が失敗したときも盤面を維持し、自動対局はmode 3に切り替えて入力を受け付ける。

### 任意の局面を読み込む

```text
setboard --file tools/bench/positions/ffo33-51.obf --line 18
hint
analyze
```

この例はFFO50を読み込む。`--line` は空行・コメントを除いた局面の1始まりの番号で、省略すると1。空白を含むパスは `setboard --file "my positions.obf" --line 2` のように引用する。`setboard <64マスの盤面> <XまたはO>` で直接読み込むこともできる。

読み込み成功時は、その局面を履歴の起点とし、TTをクリアしてmode 3に切り替える。直後にAIが自動着手しないので、先に設定や候補を確認できる。続けて打つ場合は `go` や `mode 0` 等を指定する。読み込み失敗時は盤面・履歴・モードを変更しない。

## 棋譜の検討：review

```sh
./target/release/deft-reversi-cli review f5d6c3 --eval 4@100
./target/release/deft-reversi-cli review --file games.txt --level 10 --time-ms 1000
```

ファイルは1行1棋譜。空行と `#` コメントを無視する。パスは棋譜に含めず、自動で推定する。ファイル中の全棋譜を検証してから探索を始め、不正な棋譜を途中まで検討することはしない。

各手について、手数、色、実際の手、推奨手、実際の手の評価、最善評価、損失、探索の種類を表示する。損失は「最善評価 − 実際の手の評価」。どちらも同じ手番側・探索条件で比較し、同点なら実際の手も最善手として扱う。自分がどこで評価を落としたかを見直すために使う。

**完全読みでない行の損失は、その探索条件での推定値**。浅い探索やMPCの結果を確定した悪手とは表示しない。深さ0や時間切れ等で候補が揃わなければ `?` / `unscored` とし、損失0とは扱わない。末尾に採点できた手数・未採点・未訪問・最大損失を表示する。時間制限は1手を検討するごとに適用し、時間切れの手は未採点で次へ進む。Ctrl-Cはその場で検討を終了する。

対局中の `review` は、`setboard` で読み込んだ起点を含む現在の履歴を検討する。`review RECORD` は初期盤面から指定棋譜を検討する。どちらも対局の状態は変更しない。空の履歴はエラーにする。

## 局面ファイルの解析：solve / --solve

同じ探索設定をsolveにも渡せる。設定・モデル・スレッド数・TTを標準エラーに表示してから処理する。

```sh
./target/release/deft-reversi-cli solve positions.obf \
  --eval 12@83 --endgame 95:24,99:22,100:18 --threads 8

# 旧形式の --solve も同じコマンドとして使える
./target/release/deft-reversi-cli --solve positions.obf \
  --eval 12@83 --endgame 95:24,99:22,100:18 --threads 8
```

`-s FILE` と `--solve=FILE` にも対応。既定は完全読み。1行に1局面（64マスと手番）を読み、旧CLI付属の空白区切りの正解付きOBFと、`; A2:+38; C7:+36; ...` 形式を扱う。正解の並び順には依存せず、最大の値を最善とする。入力なし・不正な行はエラー。

各行に評価・着手・探索種別・ノード数・NPS・時間・PVを出す。正解付きでは値と手を照合し、不一致の `NG` があれば終了コード1。近似探索なら正解と異なる場合がある。中断した結果は `?` とし、正解一致とは扱わない。Ctrl-Cで残りの局面の処理を止める。最後に訪問数・不完全な解・不一致数を標準エラーへ表示する。既定は局面間でTTを保持し、`--clear-tt` で毎回クリアする。

`--tsv` は見出しと1局面1行だけを標準出力に書き、秒数を小数点以下9桁で出す。設定と集計は標準エラーなので機械処理に混ざらない。ベンチマークはこのCLIから実行する。[tools/bench](../tools/bench/README.md) を参照。

## 自己対戦・perft

```sh
./target/release/deft-reversi-cli self-play 10 --level 8 --random 20 --seed 7 --out games.txt
./target/release/deft-reversi-cli perft 8
```

自己対戦は最初の `--random` 手を無作為に打ち、残りをAIが打つ。1行1局の棋譜を `--out` に書く（既存ファイルは置き換える）。`--seed` で序盤を再現できる。Ctrl-Cは終了済みの対局までで止め、未完の対局は書かない。

perftはモデル不要。初期局面から深さ1〜Nまでの葉・パス・早期終局の数を表示する。既定はパスを手数に数えず、`--count-pass` でパスも1手に数える。

## GTP

```sh
./target/release/deft-reversi-cli gtp --level 18 --threads 1
./target/release/deft-reversi-cli gtp --game-ms 10000 --threads 4
```

`--level` と `--game-ms` はどちらか一方が必須。後者は各色の対局全体の持ち時間（ミリ秒）。GTPはプロトコル応答だけを標準出力へ出す。

対応コマンドは `protocol_version`・`name`・`version`・`known_command`・`list_commands`・`boardsize`・`clear_board`・`komi`・`play`・`genmove`・`time_settings`・`time_left`・`showboard`・`final_score`・`quit`。コマンド番号と `#` コメントを扱い、成功は `=番号`、失敗は `?番号` で返す。

盤面サイズは8、コミは0のみ。`boardsize 8` と `clear_board` は盤面・TT・時計をリセットする。`time_settings MAIN 0 0` と `time_left COLOR REMAINING 0` に対応し、秒読みは扱わない。不正な引数、非数・負の時間、不正な着手や手番にはエラーを返して状態を維持する。`final_score` は終局後だけ、黒勝ちなら `B+差`、白勝ちなら `W+差`、引き分けなら `0`。時間設定の指定後は、その持ち時間を次のリセット時の基準にする。

## 検証

```sh
cargo test --locked -p deft-reversi-cli
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tools/bench/tests
```

## 移行と保留範囲

`deft-reversi-cli` の名称を維持し、旧エンジンAPIから現行APIへ移植している。旧 `--eval-path` は `--model`、`--self-play N` は `self-play N`、`--self-play-out` は `--out`、`--self-play-start-rand` は `--random`、`--perft N` は `perft N`。旧JSON評価ファイルの代わりにDPR2モデルを指定する。

CLI内蔵FFOベンチ、対局の保存・再開、定石機能の拡張、OBF出力専用コマンド、人対AI用の対局時計は保留として追加していない。
