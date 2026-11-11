# 品質、保存状態、処理時間を調べる

## 個別局面を見る

```sh
book inspect --work openings.dbook-work --positions roots.obf
book inspect --book openings.dbook --positions roots.obf
```

OBFは盤面と手番を一行で指定する形式。CLIは局面の現在値、最終石差について分かっている上下限、各候補手の登録状態を表示する。workでは、直接探索・手動・子からの計算・未登録候補の集合探索という採用元も確認できる。

直接探索には要求・予定・完了した方式と深さ・選択率、対象手集合がある。子から計算した値には現在の品質要約がある。過去の計算入力を辿る番号は表示しない。方式不明の旧推定を、実際の中盤・終盤探索として表示しない。

旧workの読込では、変換元の版と、採用値が変わった局面・候補の件数を標準エラーへ表示する。ファイルは保存するまで書き換えない。`book --version`で探索revisionとfeature構成も確認できる。

## 全体の統計

workの`stats()`は保存された全局面と候補を走査する診断用API。現在の生成範囲外の枝や、保持している孤立局面も含む。対局の通常照会ごとには呼ばない。

|項目|意味|
|---|---|
|positions / registered_moves|局面数と、進行として登録した手の数|
|valued_positions / solved_positions|現在値がある局面と、最終石差が点まで確定した局面|
|complete_positions|全候補に個別の点評価がある局面。集合の最大値だけ分かる場合とは異なる|
|leaf_positions|登録した出辺を持たない局面|
|unregistered_candidates / unknown_unregistered_candidates|未登録候補と、そのうち点評価がない候補|
|bounded_candidates|上限・下限のみの候補|
|direct_values_used / child_values_used / manual_values_used / move_search_values_used|現在使う値の採用元|
|inactive_manual_records|古いモデル等に対応するため採用しない手動値|
|qualities / value_kinds / mixed_derivations|品質と探索方法の分布。計算した値の到達深さを直接探索の深さと混同しない|
|root_lines / lines_saturated|登録された進行の経路数と、数値上限への到達有無|

`root_lines`は、登録先がない局面を1とし、親では各子の数を足す。合流する同じ子も入ってくる経路ごとに数える。Egaroucidのn_linesとは定義が異なる。

## 残りの未登録候補を再計算する

```sh
book leaves --model MODEL --work openings.dbook-work --levels 8 --nodes 100000
book resume --model MODEL --work openings.dbook-work --nodes 100000
```

`leaves`は保存した登録範囲を増やさず、未登録側の候補を調べ直す。時間またはノード予算を必要とする。再開は同じ設定を引き継ぐ。範囲や条件を変える場合は`extend`を使う。

## 生成時間とメモリ

`profile`には探索、資源の再配分、登録、予定作成、仕事の選択、経路判定、結果の公開、定期保存の時間を表示する。複数workerのCPU時間の合計ではなく、各処理の経過時間である。モデル読込などを含むプロセス全体の時間とは別。

`managed_bytes`はbookを生成・更新するために保持する局面、記録、仕事一覧などのメモリ見積もりである。探索エンジン、モデル、探索中の局面結果を再利用する表、読込・保存中の一時領域を全て含む最大使用量ではない。対局用データの常駐見積もりも別に報告する。[運用](operations.md)と[実測](validation.md)を参照。
