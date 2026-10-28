# Deft Reversi Engine

ネイティブ向けのリライト実装。旧エンジン（基準コミット `f4192a0`）を参照しています。

実装・元モデル直接読込・実データ照合・通常/並列ストレステストを用意しています。検証件数や測定結果は各時点のレポートを参照してください。ビルドにはルートの `rust-toolchain.toml` と `Cargo.lock` を使います。

## 構成

- `board`: 非公開のビットボードと検証付き着手。内部探索は合法手と反転ビットを再利用。
- `evaluation`: 11パターン×4回転、60フェーズ、合法手数差、バイアス。両手番の特徴量を差分更新。
- `search/api`: 要求・結果・停止・統計・エラー。
- `search.rs`: モデルと資源の所有、反復深化、最大24手の終盤準備探索と選択性の段階移行、アスピレーション窓。
- `model` / `mpc`: 元のv3/v4パターンモデルとMPC設定の同時読込、回帰係数・深さ表、元の既定設定。
- `search/root`: ルート候補ごとの境界、アスピレーションの安定化、最善手順の継承。
- `search/level`: 元のレベルからの深さ・選択率・終盤への移行と序盤補正。
- `search/tree`: PVS/NWS、TT、手順付け、深さ1の評価、MPC。
- `board/stability`: 元実装の辺の全パターン表・満杯のライン・接触による確定石判定。
- `search/endgame/mod`: 終盤探索の入口と通常窓の処理。
- `search/endgame/local_tt`: 空き6〜13ごとの局所TT。境界・候補手・選択率を保持し、照会結果は値として再帰へ渡す。
- `search/endgame/ordered`: 空き7〜13の手順付けと探索。
- `search/endgame/shallow`: 空き5〜6の専用探索。
- `search/endgame/last_four` / `small`: 空き4以下の展開済みカーネルと小局面の直接入口。
- `search/endgame/parity`: 四分割領域とパリティの共通定義。
- `board/last`: コンパイル時生成の反転数表。BMI2構成では最後の1マスの計算に使用。
- `search/ybwc`: 最初の候補を先に探索し、残りを共有分割点から配布。
- `parallel`: 常駐ワーカーのメールボックスと、待機中の祖先への直接ハンドオフ。
- `transposition`: 64バイトクラスタ、完全な盤面照合、atomicスナップショット。通常窓・ゼロ窓のカット、両境界の絞り込み、ETC、候補2手。PV専用表は主表の1/16（最低1 MiB）。

UI、対局履歴、ファイルパスの決定、環境変数、標準出力は探索ランタイムの責務に含めない。呼び出し側が指定したモデルファイルの読込はEngine APIで提供する。

## API

```rust,no_run
use deft_reversi_engine::{Board, Engine, EngineOptions, SearchRequest};
use std::time::Duration;

let mut engine = Engine::from_file("eval-v4.bin", EngineOptions { threads: 4, tt_mib: 256 })?;
let mut request = SearchRequest::for_level(Board::new(), 16);
request.time_limit = Some(Duration::from_secs(1));
let stop = request.stop.clone();
let result = engine.search(request)?;
// 制限なしの通常探索は engine.solve(board, level) でも呼べる。
# Ok::<(), Box<dyn std::error::Error>>(())
```

- スレッド数は呼び出し元を含む。既定値1ではワーカーを作らない。
- 同じEngineへの要求は直列。別Engineは独立したTTとワーカーを持つ。
- `Board::from_bits` は石の重複を拒否する。A1が最下位ビット。
- `Action::Place(Square)` と `Action::Pass` を区別。`result.action == None` は終局。
- `SearchRequest::for_level(board, level)` / `Engine::solve(board, level)` は元の1〜60へのクランプ、探索方式、選択率、序盤補正を適用する。
- `selectivity` は元の68/77/85/95/98/99/100%の7段階。`result.selectivity` と `result.probcut_sigma` は最後に完了した段階の設定を表す。カスタムsigmaを明示した場合は前者がNoneになる。
- `SearchMode::Depth(n)` は着手数基準。パスでは深さを減らさない。
- `SearchMode::Exact` は最終段階でMPCを使わない終盤完全読み。元の既定MPC設定または読込モデルのMPC設定で、その前に元実装と同じ選択性の段階で準備探索する。最終段階にMPCを指定する要求は拒否する。
- `SelectiveEndgame` はMPCを使う終盤探索。既定選択率は85%。完全読みの保証を持たない。
- `requested_mode` は要求、`mode` は最後に完了した反復の方式。完全読み要求の中断結果が `Depth(n)` や `SelectiveEndgame` になることがある。`mode == Exact` かつ `selective == false` の完了結果だけを完全読みとして扱う。
- `score == None` は完了反復がない状態。合法な代替手は返す。深さ0の着手は探索で選んだ最善手ではない。
- `Bound::Exact` は返された探索方式における窓内の値。固定深さ評価や選択的評価を、ゲームの確定値とは扱わない。
- スコアは入力手番側から見た石単位の整数。終局時は空きマスを勝者に加算する。
- ノード上限は全ワーカー・反復を通じた再帰入口の総数。手順付けの浅い探索もノード上限に含み、評価回数も別カウンタへ記録する。
- 停止要求と並列カットは別状態。配布した仕事を回収してから戻る。停止・時刻の確認は最大256ノード間隔。ノード上限は各ノードで予約し、超過しない。
- StopHandleは一方向。再要求には新しいSearchRequestまたは新しいStopHandleを使う。
- 局所TTは要求中のContextが所有し、主スレッド分をEngineで再利用する。各層は必要時に確保し、最大約384 KiB。メインTTのMiB指定とは別の作業領域。
- `clear_tt`、`set_mpc_config`、`set_probcut_rules` は主表・PV表・局所表を無効化する。`set_model` は全表を無効化し、元の既定MPC設定に戻す。
- 通常窓のTTカット後もPVを返せるよう、PV表・主表の候補を使って確定値に到達する手を検証する。この復元も探索時間とノード予算に含む。スコア確定後、PV復元中に上限へ達した場合は、確定スコアを保持したまま停止理由と取得済みPVを返す。

## モデルとMPC

`Engine::from_file` / `Engine::from_bytes` は元のv3/v4パターンモデル（JSON・bincode）から、重み・メタデータ・MPC設定をまとめて読み込む。変換やsidecarの手動設定は不要。パターンの構成、位相、スケール、評価の丸め方は維持する。NNUEは対象外。

`Engine::mpc_config()` で読込設定を取得し、`set_mpc_config` で元と同じ回帰係数・深さ表を変更できる。確認探索の深さは、実際に使う各表の範囲で本探索より浅い必要があります。不正な設定は読込時と設定時に拒否します。MPCは通常窓にも適用し、浅い確認探索にはTTや再帰的なMPCを使わない。係数は元の式で展開し、実行時は直接参照する。回帰式を外挿した結果の負の偏差も、元設定と同じ符号のまま保持する。

`Engine::new(PatternModel, options)`、旧JSONパターンラッパー、重みだけのDPR2読込では元の既定MPC設定を使う。モデル固有の設定を持つ場合は、元の統合モデルファイルを直接読む。DPR2の `.mpc.tsv` は自動では読まない。

`set_probcut_rules` は個別ルールを明示する互換APIとして残す。この場合、回帰係数形式の設定がないため `mpc_config()` はNone。`set_probcut_rules(Vec::new())` は校正を空にする。Exactでは全段階MPCなしとなり、選択的探索を要求した場合は校正不足のエラーになる。

DPR2は重みだけの互換形式として引き続き使用できる。変換ツール `tools/convert_pattern_v2.py` も残すが、通常の利用・性能比較では元のモデルファイルを使う。

旧 `main` の `data/eval/eval.json` は `tools/convert_legacy_eval.py` で変換できる。変換済みの旧モデルには `engine.set_legacy_main_mpc()?` を明示して `main` のMPC係数表を使える。表はモデルに埋め込まれていないため、別のモデルには自動適用しない。切替時にはTTを無効化し、`mpc_config()` は `None` を返す。例のsolveでは `--legacy-main-mpc` を指定する。

## 検証

まず1ビルドジョブ・1テストスレッドで通常検証する。

```sh
CARGO_BUILD_JOBS=1 cargo test -p deft_reversi_engine -- --test-threads=1
RUSTFLAGS="-Ctarget-cpu=native" CARGO_BUILD_JOBS=1 cargo test -p deft_reversi_engine --release -- --include-ignored --test-threads=1
```

検証ケース: 座標参照実装との照合、perft、差分特徴量、スコア・フェーズ、モデル往復、不正入力、単純完全読みとの一致、合法PV、停止・再利用、MPC分離、TT同時アクセス、祖先ハンドオフ、1/2/4/8/20スレッド反復。

```sh
RUSTFLAGS="-Ctarget-cpu=native" cargo run --locked --release -p deft_reversi_engine --example solve -- MODEL.bin POSITIONS.obf exact 4 256
```

OBFの盤面64文字と手番X/Oを読み、結果・ノード数・分割数・時間をTSV出力する。任意の末尾引数で1局面の時間上限（ミリ秒）を指定できる。
`PatternModel::zero()` はルール・探索テスト用。棋力のあるモデルではない。

## 旧版との比較

[tools/bench/compare_reference.py](../tools/bench/compare_reference.py) は、旧エンジンへの依存を生成先の独立したCargoプロジェクトへ設定します。通常ワークスペースに旧実装を含めません。

```sh
RUSTFLAGS="-Ctarget-cpu=native" python3 tools/bench/compare_reference.py --old-engine /path/to/old/deft-reversi-engine -- OLD_MODEL NEW_MODEL POSITIONS.obf level:60 5
```

現行の比較コマンドと配置は [比較ツールの説明](../tools/bench/README.md) を参照してください。

通常API同士の性能比較には `level:60` を使う。`exact` と固定深さは診断用で、通常APIの比較とは条件が異なる。評価値の一致を確認し、新旧を交互に実行、局面ごとにTTを空にし、探索スコアの一致と5回以上の中央値を報告する。初期化とモデル読み込みは測定外。
既存の参考先レビューにあるノード集計差があるため、NPSだけで採否を決めない。
基準局面、CPU割当、ビルド条件、モデルハッシュ、参考コミットを固定する。
旧版の `clear_tt` は世代更新だけなので、比較ツールでは旧Solverを作り直し、スレッド内の局所TTも新しいスレッドで初期化する。
`tools/generate_rewrite_fixtures.py` で固定した座標ルールのゲームから局面を再生成できる。
LinuxのCPU 1に固定する場合は、比較コマンドの先頭に `taskset -c 1` を付ける。

## 並列測定

```sh
python3 tools/bench_rewrite_parallel.py --binary target/release/examples/solve --model data/eval/pattern.dpr2 --positions deft-reversi-engine/tests/fixtures/ffo40-44.obf --out parallel.tsv
```

1/2/4/8/20スレッドを5周、往復順で測る。スレッド数ごとのCPU割当を固定し、全局面のスコア一致を確認する。
プロセス・TTは各実行で新しくし、モデル読み込みや初期化は探索時間に含めない。TTは1回の実行内の局面間では再利用する。
生データとバイナリ・モデル・局面のハッシュ、CPU割当、中央値を保存する。

## 終盤探索の最適化と実験

既定の汎用探索では、子TTのETCを確認してからTT候補を他候補の評価前に探索する。局所TTの照会結果は子へ渡し、同じ照会を繰り返さない。選択率タグと中断時の保存条件は維持する。

末端のゼロ窓探索では、6マス側で既知の合法手を再利用し、5マス側は空きマスから合法手を順に確認して専用4マス探索へ直接つなぐ。6→5→4マスの四分割parityは差分更新する。手順付け規則とノード上限・中断処理は維持している。[末端カーネルの比較](../docs/benchmarks/2026-10-27-tail-kernels/README.md)に段階的な試作と測定条件を記録した。

最終1マスでは、BMI2の抽出結果を8ビットに限定し、縦・斜線の表引き行を共通化して、不要な範囲検査と位置情報の読出しを削減した。最終1マスを2マス側へインライン展開し、安定石表のLazyLock参照は判定あたり1回にまとめる。[最終1マスの比較](../docs/benchmarks/2026-10-27-last-empty-optimization/README.md)に機械語の確認、正しさの検証、採用・保留した試作と測定上の制約を記録した。

評価と手順付けでは、通常評価がmobility項のために計算した合法手を共有する。既定の完全読みでは、ゼロ窓の14→13マスで不要な評価状態の更新を省き、広い窓で再探索する場合に状態を作る。[合法手共有と評価状態更新の比較](../docs/benchmarks/2026-10-27-ordering-reuse/README.md)に2方式の試作、正しさの検証、局面別の性能差を記録した。

`--features endgame-pattern-ordering` は、残り11〜13マスのゼロ窓探索にパターン評価を追加する実験用機能。7〜10マスは合法手数とparityによる軽い手順付けを使う。既存モデルの44パターンを使い、特徴量を差分更新し、候補評価では子の片側の特徴量だけを計算する。終盤手順付け専用に学習した係数ではなく、既定では無効。比較と試作の経緯は[測定レポート](../docs/benchmarks/2026-10-27-endgame-implementation/README.md)を参照。

```sh
RUSTFLAGS="-Ctarget-cpu=native" cargo build --locked --release --example solve --features endgame-pattern-ordering
```

現deft・Edax・Egaroucidの通常SIMD版／AVX-512版を、FFO40〜49・1コア・主TT約839万項目で比較した。[3エンジンの比較](../docs/benchmarks/2026-10-27-three-engines/README.md)に8周分の時間、局面別の差、評価データ・ハッシュ・計時条件を記録した。

## 対象外と制約

初回はネイティブエンジンのみ。Web/WASM、対局CLI、学習処理、NNUE、対局履歴の移行は含めていない。
速度は測定対象とCPUに依存する。特定の集合での改善を、全局面の改善とは扱わない。
過去の旧版比較は [調査レポート](../docs/benchmarks/2026-10-24-regression-investigation/README.md) に、その時点の条件とともに保存しています。
