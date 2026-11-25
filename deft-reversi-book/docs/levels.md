# レベル、深さ、選択率

## 指定方法

CLIは`--levels 4,8,12`、または`--stages 12,16,20 --selectivity 95`。同時指定は拒否する。Rustでは`GenerationConfig.analysis`の`AnalysisSchedule::Levels`または`Stages`を使う。使わない側の設定は存在せず、検査もしない。

レベルは1〜60の厳密な昇順。深さ系列は2〜60で深さを増やし、選択率を下げない。

```sh
target/release/book build --model MODEL --work trial.dbook-work \
  --levels 2,4 --max-plies 2 --seconds 10 --tt-mib 64
target/release/book extend --model MODEL --work trial.dbook-work \
  --levels 4,8,12 --max-plies 12 --seconds 60
target/release/book resume --model MODEL --work trial.dbook-work --seconds 60
```

## 現在の解釈

検索する親局面の空きマス数に対して、現在のEngineのレベル表を使う。序盤・中盤のレベルNは深さN・MPC83%。レベル22では空き28から選択的終盤、空き21から完全読みになる。Bookの着手数上限は別に指定できる。

要求は `RootLevel`、予定は `Engine::search_plan` で判定した条件、実際に完了した方式・深さ・選択率は `SearchResult` から記録する。旧 `ChildLevel` の記録は当時の探索条件として読み取る。既存の確定値を守るために探索を強化するときは、強化後の条件を記録する。

## 続行と再設定

`resume`は保存した条件を引き継ぐ。レベルや範囲を変更する場合は`extend`、同じ結果の再利用を無効にする場合は`reanalyze`を使う。旧形式に保存された完了結果は、条件変更の前に取り込む。

[評価の意味](value-model.md)、[旧データの引継ぎ](compatibility.md)を参照。
