# 時間配分の統合作業：引き継ぎ（2026-11-20）

ブランチ `ccr-a29b8244-tag4vo`（`feature/cli-v2` をマージ済み）。ベンチマーク用のPCで作業を続けるためのメモ。

## 1. 今の状態

| コミット | 内容 |
|---|---|
| `5d005a2` | `feature/cli-v2`（`525a908`、`a21b8c9`）をマージした。時間配分は `deft-reversi-cli/src/game_clock.rs` の `Budget` / `Forecast` を土台にし、エンジン側の重複実装（`GameClock`、`MoveTime`、`SearchRequest::target_time`）は削除した。CLI の `gtp.rs`・`flipsight.rs`・`README.md` は `feature/cli-v2` と同じ内容 |
| `5e70509` | `Progress::next` を追加した。次に実行する予定の反復の種類で、最後の反復のあとは `None`。`Progress` は `#[non_exhaustive]` なので、互換性を保った追加になる。`tests/search.rs` で確かめている |

`feature/cli-v2` との差は、`Progress::next`、設計文書 `docs/design/2026-11-18-time-management.md`、このディレクトリ、`tools/time-control/` だけ。`Forecast` はまだ `next` を使っていない。

## 2. 残りの作業

### 2.1 `Forecast` を `p.next` で場合分けする（`game_clock.rs`）

今は、次の反復の種類が分からないまま倍率を決めている。中盤は直近の比、終盤はすべて 1.6 倍。次のように分ける。

| `p.kind` → `p.next` | 予測の倍率 |
|---|---|
| どれでも → `None` | 予測しない（`false` を返す） |
| 中盤 d → 中盤 d′ | 今の比（1.5〜6 に収める）を `(d′−d)/2` 乗する |
| 中盤 → 終盤（最初のステージ） | 遷移の倍率（2.2 の測定で決める） |
| 終盤の選択的ステージ → 完全読み | 完全読みの倍率（同上） |
| 終盤の選択的ステージ → 次のステージ | 1.6（今のまま） |

`kind == Percent99` なら次は完全読みだと推測もできるが、24空きのように 95% を飛ばす計画もあるので、`next` で判断する。

### 2.2 遷移の倍率の測定（途中まで）

`ratio-partial-1thread.txt` に、1スレッド・TT 64 MiB・`timed_levels(60, 15s)` で、29局面分の反復ごとの完了時刻がある。v3 モデル、4論理CPUのクラウド環境で、3プロセスを同時に動かしたので少し遅めに出ている。各行は `種類:完了時刻(秒):評価値:最善手`。

ここまでの読み取り（1スレッド）：

- **中盤 → 85%**：最後の中盤反復にかかった時間の、多くは3〜5倍、最大10倍（20〜30空き）。32〜34空きでは、15秒以内に終盤ステージへ届かなかった。
- **99% → 完全読み**（20空きは 98% → 完全読み）：22〜24空きで2〜10倍、中央値は約4〜5倍。4スレッドで測ったとき（`docs/design/2026-11-18-time-management.md`）は、24空きで11倍、26空きで43倍だった。スレッド数で倍率が変わるので、ベンチマークで使うスレッド数で測り直すこと。
- 初期案：中盤 → 終盤は `max(3, 4·1.3^(空き−28))`、完全読みは `max(2, 5·1.5^(空き−23))`。少し多めに見積もっても、そのステージが次の手番に回るだけなので損は小さい。少なく見積もると、`hard` まで時間を捨てることになる。

測り直し：

```sh
cd tools/time-control/probe && cargo build --release
DEFT_MODEL=model.dpr2 ./target/release/probe GAMES.txt 15 34,32,30,28,26,24,22,20 8 1 0 > ratio.txt
# 引数: 棋譜ファイル 制限秒 空き数リスト 局数 スレッド数 飛ばす局数
# 棋譜: deft-reversi-cli self-play -l 8 --random 8 --seed 11 -o GAMES.txt 24
```

### 2.3 時間がないときに探索せず指す問題（GTP）

`gtp.rs` では、`limit.is_zero()` のときに合法手の先頭（ビット位置が最小の手）を返している。ほぼ時間を使わない 1〜2手読みにする。

### 2.4 対局で確かめる

`a21b8c9`（`feature/cli-v2`）と統合版で対局させる。`tools/time-control/referee.py` は、2つの GTP プロセスを動かす簡易な審判（未使用、動作未確認）。`tools/bench/match.py` があれば、そちらの方がよいかもしれない。

```sh
python3 tools/time-control/referee.py NEW_BIN OLD_BIN 秒 開局ファイル 開局数 開局手数 worker番号 worker数
# 出力: 開局番号 Aが黒か 石差(A視点) 時間切れ A残り B残り
```

見るべき条件：GTP 2秒、GTP 15〜60秒、FlipSight 2秒・6秒。1スレッドと4スレッドの両方。差は小さいはずなので、数百局か SPRT が必要。指標は石差の平均と95%信頼区間（開局ペアごとの t 区間）。調整に使った開局と最終評価の開局は分ける。

診断として、`DEFT_GTP_LOG=1` で「最後に完了したのが `Endgame{Percent99}`（または中盤の最終深さ）で、使用時間が `hard` に近い手」の割合を、修正前と修正後で比べる。2.1 の効果が直接分かる。

### 2.5 文書

- `docs/benchmarks/2026-11-20-game-clock/README.md` の GTP +17.89石 / +9.42石は、比較対象の `2df7a40` の GTP がおかしかったことの影響が大きい。空き20以下で `moves_left = 1` になり、残り時間を1手で使い切っていた。そのことを書き添える。
- 第2段階（`a21b8c9`）の結果は、どれも信頼区間が0をまたいでいる。「改善」ではなく「悪化は見られない」と書く。
- `docs/design/2026-11-18-time-management.md` は、エンジン側で時間を管理していた版の設計なので、統合後の設計（`Budget` ＋ `Progress::next`）に書き直す。

## 3. 参考：これまでの対局

`match-old-vs-engine-pacer-20s.txt`：`2df7a40` の FlipSight 方式と、エンジン側の時間管理（削除済み）の比較。4スレッド・各20秒・40局で、21勝5分14敗、石差 +1.55 ± 2.6。有意ではない。
