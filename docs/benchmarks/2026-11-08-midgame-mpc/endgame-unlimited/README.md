# 終盤は完全読みの完了を待つ

この設定変更はユーザーの質問の誤解によるものだったため取り消した。現行の終盤上限は25 ms。以下は取り消し前の実装・補助検証の履歴であり、現行仕様ではない。

空き18以下では時間制限なしで `Goal::Endgame` / `Full` の完了を待つ設定を追加し、対戦ツールとGTPの既定にした。`--endgame-ms 0` が時間制限なし、正の値は従来どおりms単位の上限。中盤は100 ms／手・1手刻みの反復深化、切り替え境界とTTの扱いも維持する。

終盤のログの `budget_ms` は `null`。対戦ツールは、この設定で `solved=true`・`stop=Completed` にならない終盤着手をエラーにする。時間指定がなくなるため終盤準備探索は既存仕様の2手刻みになる。中盤は引き続き時間指定があり、1手刻みである。

## 前回に完全読みが間に合わなかった着手の再確認

前回の600局で25 msの上限内に完全読みが終わらなかった40着手・39局を対象にした。`main-100ms/games.tsv` の棋譜から、各着手の空き数をeとして最初の `60-e` 着手をGTPの `play` で再現し、TTを空にして同じ局面を解いた。棋譜に省略されたパスは合法手の有無から復元する。各ケースの元の局・側・空き数・棋譜の先頭部分は [recheck-cases.json](recheck-cases.json) に保存した。

- 40件すべて完全読みが完了した。
- 探索時間は最小15.542 ms、平均35.742 ms、最大78.644 ms。
- 再確認全体は約2.31秒。これは同一のGTPプロセスでケースごとに `clear_board` と棋譜を適用して測った時間。
- CPU 2・1スレッド・TT 64 MiB・同じPENモデル。GTPバイナリのSHA256と起動コマンドは [recheck-summary.json](recheck-summary.json)、各手・反復の記録は [recheck.jsonl](recheck.jsonl) にある。

終盤準備の反復列も1手刻みから2手刻みに変わるため、時間の差を上限解除だけの速度比とは解釈しない。

## 対局の動作確認

```sh
RUSTFLAGS='-Ctarget-cpu=native' CARGO_TARGET_DIR=target-native \
  cargo build --locked --release -p deft_reversi_engine --example gtp
python3 tools/bench/mpc_match.py \
  --out docs/benchmarks/2026-11-08-midgame-mpc/endgame-unlimited/match-smoke \
  --openings 4 --seed 2 --budgets 100 --candidates 68 --baseline 85 \
  --endgame-empties 18 --endgame-ms 0 --cpu 2
python3 tools/bench/mpc_report.py \
  docs/benchmarks/2026-11-08-midgame-mpc/endgame-unlimited/match-smoke
```

8局を完了し、共通終盤の144着手すべてで完全読みが完了した。1局平均3.483秒、全8局で27.87秒。境界・確率・時間設定の検証も成功した。得点率は少数の動作確認なので評価しない。

結果は [match-smoke/diagnostics.json](match-smoke/diagnostics.json)、時間・深さは [match-smoke/move-statistics.md](match-smoke/move-statistics.md)。Pythonのテスト7件、Clippy・fmt・差分の空白検査も成功した。

前回の600局は25 ms上限での結果として保持する。時間制限なしの条件で600局を再測定したものではない。
