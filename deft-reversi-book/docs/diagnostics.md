# 品質、保存状態、処理時間を調べる

## 個別局面を見る

```sh
target/release/deft-reversi-cli book inspect --work openings.dbook-work --positions roots.obf
target/release/deft-reversi-cli book inspect --book openings.dbook --positions roots.obf
```

`OBF` は盤面（64 文字）と手番（`X`/`O`）を 1 行で指定する形式である。`inspect` は局面の現在値、最終石差について分かっている上下限、各候補手の登録状態を表示する。`.dbook-work` では、直接探索・手動・子からの計算・未登録候補の集合探索という採用元も確認できる。

直接探索には要求・予定・完了した方式と深さ・選択率、対象手集合がある。子から計算した値には現在の品質要約がある。過去の計算入力をたどる番号は表示せず、方式不明の旧推定を実際の中盤・終盤探索として表示もしない。

旧形式の `.dbook-work` を読み込んだ場合は、変換元の版と採用値が変わった局面・候補の件数を標準エラーへ表示する。ファイルは保存するまで書き換えない。`deft-reversi-cli book --version` で探索 revision と feature 構成も確認できる。

## 全体の統計

`BookEditor::stats()` は保存された全局面と候補を走査する診断用 API である。現在の生成範囲外の枝や、保持している孤立局面も含めて集計する。グラフ全体を走査して一時領域を確保するため、対局の通常照会ごとには呼ばない。

| 項目 | 意味 |
|---|---|
| `positions` / `registered_moves` | 局面数と、進行として登録した手の数 |
| `valued_positions` / `solved_positions` | 現在値がある局面数と、最終石差が一点まで確定した局面数 |
| `complete_positions` | 全候補に個別の点評価がある局面数（集合の最大値だけ分かる場合は含まない） |
| `leaf_positions` | 登録した出辺を持たない局面数 |
| `unregistered_candidates` / `unknown_unregistered_candidates` | 未登録候補数と、そのうち点評価がない候補数 |
| `unknown_candidates` / `bounded_candidates` | 点評価がない候補数と、上限・下限のみの候補数 |
| `direct_values_used` / `child_values_used` / `manual_values_used` / `move_search_values_used` / `final_score_values_used` | 現在使う値の採用元ごとの件数 |
| `inactive_manual_records` | 古いモデル等に対応するため採用しない手動値の数 |
| `qualities` / `value_kinds` / `mixed_derivations` | 品質と探索方法の分布（計算値の到達深さを直接探索の深さと混同しない） |
| `root_lines` / `lines_saturated` | 登録された進行の経路数と、`u64::MAX` 上限への到達有無 |

`root_lines` は、登録先がない局面を 1 とし、親では各子の数を足して求める。合流する同じ子も入ってくる経路ごとに数えるため、Egaroucid の `n_lines` とは定義が異なる。

## 残りの未登録候補を再計算する

```sh
target/release/deft-reversi-cli book leaves --model MODEL --work openings.dbook-work --levels 8 --nodes 100000
target/release/deft-reversi-cli book resume --model MODEL --work openings.dbook-work --nodes 100000
```

`leaves` は保存した登録範囲を増やさず、未登録側の候補を調べ直す。時間またはノード予算を必要とする。中断後に `resume` すると同じ設定を引き継ぐ。範囲や条件を変える場合は `extend` を使う。

## 生成時間とメモリ

`GenerationProfile` には探索、資源の再配分、登録、予定作成、仕事の選択、経路判定、結果の公開、定期保存の時間を表示する。これは複数ワーカーの CPU 時間の合計ではなく、各処理の経過時間である。モデル読み込みなどを含むプロセス全体の時間とは別になる。

`managed_bytes` は Book を生成・更新するために保持する局面、記録、仕事一覧などのメモリ見積もりである。探索エンジン、モデル、置換表（TT）、読み込み・保存中の一時領域をすべて含む最大使用量ではない。対局用データの常駐見積もりも別に報告する。詳細は [運用](operations.md) と [検証](validation.md) を参照。
