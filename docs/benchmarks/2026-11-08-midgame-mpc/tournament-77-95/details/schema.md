# 生CSVの仕様

> 2026-11-15 追記：`moves.csv.gz` と `iterations.csv.gz` は git で管理せず、手元の `local/bench-runs/docs-raw/2026-11-08-midgame-mpc/tournament-77-95/details/` に置いた。

## 対局・着手・反復の結合

- games.csv：1行1局。元のgames.tsvと同じ成績・棋譜に実行順と対局時間の累積を加えた。cumulative_game_wall_secondsは実際の時計時刻ではなく、各局の所要時間の合計。
- game-player.csv：1行1局1候補。3,000局を両候補の視点へ展開した6,000行。diffとscoreはpercentの候補から見た値。game_wall_secondsは同じ局の両側に重複するため合算しない。
- moves.csv.gz：1行1回の着手探索。game_idとpercentとside_moveが結合キー。side_moveは候補側の探索回数（1始まり）。パス・終局で探索しなかったGTPの呼び出しは含まない。
- iterations.csv.gz：1行1完了反復。上の結合キーにiteration（1始まり）を加える。中断された反復の行はなく、その作業量の一部はmovesのafter_last_done_*から分かる。

## 盤面と着手

- percent：その対局で割り当てた中盤の選択確率。opponentは相手の確率。colorは黒/白。
- empties：着手前の空き数。placement_plyは初期局面からの着手番号（パスを除く、1始まり）。
- action：実際に選んだ座標。完全な棋譜から空き数に対応する手を復元した。
- player_bits/opponent_bits：手番側/相手側のビットボード、16進文字列。bit 0がa1。先頭の0を落とさず文字列として読む。
- legal_moves：合法手の個数。棋譜の全着手を合法手と照合している。
- position_obf：X=黒、O=白、-=空き。末尾のX/Oが手番。棋譜と強制パスを再生して復元した。
- phase：midgameは空き19以上、endgameは空き18以下。

## 時間

- budget_ms：割当時間。search_msはエンジン内部、wall_msは探索前のTT消去等を含む外側時間。最終ログ出力・着手適用・GTP通信はwall_msに含まれない。
- last_done_ms：最後の完了反復の累積経過時間。
- after_last_done_ms：search_ms-last_done_ms。未完了反復の作業と後処理を含む。TTに残る作業もあるので、すべてが無駄ではない。
- last_iteration_ms：最後に完了した反復と、その1つ前の完了時刻の差。
- iterationsのelapsed_msは累積時刻、delta_msは前の完了反復からの差。

## 深さ・ノード

- depth：最後の完了深さ。終盤モードの完了反復は残り空き数を使う。
- reported_depth：元ログのdepth。終盤モードの場合は空欄。terminal_iteration=1で区別できる。
- result_percent：元ログに記録された結果の実効確率。終盤の準備反復では候補の確率と異なり85%などになる。
- stage_percent：内部段階のタグ。81/89/92%は内部で85%のタグを使うが、実際の中盤倍率はmid_sigmaで指定している。
- solved=1：元エンジンが完全読みの結果と判定した。terminal_iteration=1だけではMPCなしの完全読みを意味しない。
- target_reached=1：中盤で残り空き数と同じ深さまで到達。MPCありでは完全読みを保証しない。
- last_done_nodes：最後の完了反復までの累積訪問ノード。停止時点の最終総ノードは未記録。
- completed_nodes_per_second：last_done_nodes/(last_done_ms/1000)。探索全体のNPSではない。

## MPCカウンタ

- tries：規則があり試行した回数。静的な事前判定を通らなかった試行も含む。
- searches：実際に行った浅い確認探索の回数。1試行で上下2回行う場合がある。
- cuts：MPCでカットできた回数。movesのカウンタは中断された反復の処理も含む。
- last_iteration_searches/cuts：最後に完了した反復内の差分。
- after_last_done_searches/cuts：最後の完了反復以後の差分。
- first_mpc_depth/ms：MPCの確認探索を含む反復が初めて完了した深さと累積時刻。MPC判定そのものを開始した瞬間ではない。
- iterationsのnodes/tries/searches/cutsはその完了反復までの累積値、delta_*は前の完了反復からの差。
- mid_sigma：中盤の実測倍率。終盤では空欄で、元の校正へ復元される。

## まとめCSVの単位

- score、ci、bootstrap_leader_share、mpc_cut_per_searchなどの率は0〜1の値。HTML/Markdownでは百分率で表示する。
- score_differenceと区間は0〜1単位の差。表でppと書いた列は100倍したパーセントポイント。
- *_msはミリ秒、*_secondsは秒、depthは手数。標準偏差は母標準偏差。
- 分位点は隣接値の線形補間、中央値は中央2件の平均（偶数件の場合）。
- pair-pentanomialのpoints_*は先後2局の合計得点の件数。points_1には1勝1敗と2引き分けが含まれる。
- paired-vs-referenceは共通の100開局を全候補同時に再標本化した10,000回（標準設定、seed 42）。既存のseed 1の個別集計とは区間の端点が僅かに異なることがある。
- bonferroni_family95_*は85%に対する5比較について、各比較99%のbootstrap区間を使った名目95%同時区間。
- bootstrap_leader_shareは再標本化した集合で首位になった割合。同率は人数で按分する。最強設定である事後確率ではない。
- 空欄は未記録・該当なし。着手ごとの評価値、PV、最善手からの損失は未記録で、復元していない。

## 読み込み例

Python標準ライブラリだけで読める。

    import csv, gzip
    with gzip.open("moves.csv.gz", "rt") as f:
        rows = csv.DictReader(f)
        for row in rows:
            if row["percent"] == "81" and row["phase"] == "midgame":
                print(row["game_id"], row["empties"], row["action"], row["depth"])

iterations.csv.gzは通常の表計算ソフトの行数上限を超える場合がある。実際の行数はmetadata.jsonのiterationsで確認する。
