2026-10-27 最適化後に残る終盤探索の実装差

追記：この調査時点の後、項目1・4・6・7について実装と比較を進めた。[最終1マスの最適化結果](../2026-10-27-last-empty-optimization/README.md)を参照。以下は変更前の調査記録である。

対象は現在のdeft作業ツリー（refactor/engine-layout、基点8ab1682、TT改善と5〜6マスカーネル改善を含む）、Edax 786421f355219ade872ce7285c1cab1eaf67836a、Egaroucid 2a65c87860be999373088ad833aa00b0844883b6。今回はソースと既存のnativeバイナリを調べた。探索実装の変更・追加の速度測定は行っていない。

確認できたのは、処理の存在・不足・データの持ち方・有効なコンパイル分岐である。実時間差を何割説明するかは未確定。以下の優先順位は処理の頻度・余分な処理の明確さ・変更範囲から判断した。現deft全体がEdax/Egaroucidより何%遅いかを、この調査で再測定したものではない。

| 優先 | 実装差 | 主な範囲 | 根拠 |
|---|---|---|---|
| 1 | 最終反転数の表引きに境界チェック・複数の位置情報読出しが残る | 最後の1マス | ソース＋native命令で確認 |
| 2 | MPCの静的評価による試行選別がない、確認探索が単純 | 選択探索・準備探索 | 3者のソースで確認 |
| 3 | MPCのために評価状態を再構築する | 選択率付き12〜13マス | 状態を捨てる入口と再構築の両方を確認 |
| 4 | 合法性だけで済む場合にも最終反転数を数える | 最後の1マス | EgaroucidのAVX2経路との差 |
| 5 | 5〜6マスに安定石カットがない | NWS 5〜6 | Edax/Egaroucidには呼出しがある |
| 6 | 末端でスカラー化・関数呼出し・制御処理が残る | NWS 1〜4 | EgaroucidのSIMD盤面と比較、nativeも確認 |
| 7 | 安定石表のLazyLockを各辺で参照し、初期化も探索中に起き得る | 安定石判定 | ソース＋native命令で確認 |
| 8 | 安定石計算の方向並列処理と既知深さの利用に差がある | 主に4〜13マス | 有効な分岐を確認。Edaxの無効分岐を区別 |

**1. 最終反転数のPEXT結果に、releaseでも範囲検査がある**

現deft [board/last.rs:95](/workspace/deft-reversi/deft-reversi-engine/src/board/last.rs:95) は横1本＋縦・斜線3本を表引きする。3本についてLineのmaskとpositionを読み、PEXT結果をusizeとしてCOUNTSに渡す。各maskのビット数は8以下なので結果は255以下だが、コンパイラはその意味を推論できていない。

現nativeバイナリのsolve_score_1_emptiesには、各PEXTの後に`cmp $0xff`と条件分岐が3組残っている。パスで相手の石数も数える経路にも同種の検査がある。square<64の検査もある。これはdebugだけのassertではない。

Edax [count_last_flip_bmi2.c:123](/workspace/edax-reversi/src/count_last_flip_bmi2.c:123) はCOUNTS相当の表を横用と縦用の2ポインタにまとめ、縦・斜線3本で共通のCOUNT_FLIP_Vを使う。線ごとのposition読出しを必要としない。マスクと表の設計を組み合わせている。

方向：まずPEXT結果の範囲をu8等の型に明示して、正しい入力に対する値を保ったまま検査を消せるか確認する。さらにEdax型の線の位置合わせでメタデータ読出しを減らす。安易にget_uncheckedへ置換する必要はない。

**2. MPCを試す価値を静的評価で選別していない**

現deft [tree.rs:468](/workspace/deft-reversi/deft-reversi-engine/src/search/tree.rs:468) はsigmaとRuleがあれば上側の確認探索を試し、成立しなければ下側も試す。範囲外の閾値は除外するが、静的評価で見込みの薄い方向を除外する入口はない。

Edax [midgame.c:209](/workspace/edax-reversi/src/midgame.c:209) はeval_scoreとeval_errorから、その方向を試す価値を判定してからNWS_midgameを呼ぶ。Egaroucid [multi_probcut.hpp:82](/workspace/Egaroucid/src/engine/multi_probcut.hpp:82) もd0valueを計算し、140・153行付近の条件を満たす方向だけ確認探索する。

また現deftの確認先 [ordering.rs:98](/workspace/deft-reversi/deft-reversi-engine/src/search/ordering.rs:98) は升番号順の単純探索で、TTも通常の手順付けも使わない。Edaxは通常のNWS（浅ければ専用浅い探索）、Egaroucidはnega_alpha_ordering_nwsを使い、確認探索自身も探索情報を再利用する。

以前「確認探索を現deftの通常nodeへ置換」は一部で遅くなったが、それだけでこの差を無効とは扱えない。通常nodeにはLine/PV処理等の費用がある。整数スコアだけを返し、MPCの再入を止めた確認用NWSでTTと手順付けを使う設計は別の試行になる。quality・exact_mode・depthの一致条件と、中断時に未完成の値を保存しない条件を維持する必要がある。

静的ゲートは確認探索を省く代わりに有効なカットも見逃し得る。既存の浅い探索との回帰係数から、深さ0用の誤差が自動的に得られるわけではない。ゲート追加で必ず速くなるとは言わず、同じモデルで校正・比較する。

EgaroucidはUSE_MPC_PRE_CALCULATION=trueで誤差を事前計算する。現deftは各試行で浮動小数点の除算・ceil/floorを行う。選択率ステージが固定されている間の閾値表の事前計算も候補だが、現在のslope/interceptと丸め方を厳密に維持することが前提になる。

**3. 12〜13マスで、MPC用の評価状態を作り直す**

現deft [tree.rs:76](/workspace/deft-reversi/deft-reversi-engine/src/search/tree.rs:76) は13以下のNWSをBoardだけの局所探索へ渡す。その先 [tail.rs:217](/workspace/deft-reversi/deft-reversi-engine/src/search/tail.rs:217) はsigmaがあればEvalState::from_boardを呼び、MPC用に44特徴×両視点の状態を再構築する。

Egaroucid [midsearch_nws.hpp:341](/workspace/Egaroucid/src/engine/midsearch_nws.hpp:341) は完全読みなら13、MPC有効時は9以下で専用終盤へ切り替える。選択率付き10〜13では、通常探索が維持している評価状態をMPCへ使える。現deftには「更新済みの状態を入口で落として、その後に盤面から作り直す」経路がある。

この費用はMPC無効の完全読み本体には発生しない。選択率付きの準備・選択探索が対象。全候補へ176バイトを追加する方式は前の試行で不利だったため、必要な深さだけ状態を保持する／12〜13だけ別入口にする等を比較する。実験用パターン手順付けの作業状態を再利用しても、現在のMPC側のfrom_board呼出しは別に残っている。

**4. Egaroucidの最終1マスには、反転数計算を省く追加のカットがある**

現deft [last_four.rs:43](/workspace/deft-reversi/deft-reversi-engine/src/search/last_four.rs:43) はまず自分の反転数を数える。パス後の相手の反転数をalpha次第で省く処理は既にあり、lazy cut自体がないわけではない。

EgaroucidのAVX2経路 [endsearch_nws_last_simd.hpp:122](/workspace/Egaroucid/src/engine/endsearch_nws_last_simd.hpp:122) は、石数だけの基準値がalphaを超える場合、合法性をSIMDで確認して、置ければ最小反転分を足して帰る。正確な反転数を得なくても、ゼロ窓のカットを証明できるためである。

Edaxの今回読んだBMI2 solve_1は現deftと同じく自分の反転数を先に数える。この追加の差は主にEgaroucidに対するもの。移植先もNWS専用に限り、正確なPV値を要求する経路へ無条件に混ぜない。

**5. 現deftの5〜6マスには安定石カットがない**

現deft [six_moves_score:420](/workspace/deft-reversi/deft-reversi-engine/src/search/tail.rs:420) と [five_empty_score:467](/workspace/deft-reversi/deft-reversi-engine/src/search/tail.rs:467) には安定石判定がない。7〜13のordered_endgameと4マスのカーネルにはある。

Edax [endgame.c:422](/workspace/edax-reversi/src/endgame.c:422) は6以下の浅い探索でも判定する。NWS_STABILITY_THRESHOLDは5マスで8、6マスで10。Egaroucid [endsearch_nws.hpp:56](/workspace/Egaroucid/src/engine/endsearch_nws.hpp:56) もfast経路でUSE_END_SCが有効で、パス直後を除き判定する。Egaroucidは逆にlast4のSCを既定では無効にしている。

deftは子孫へ降りる前に証明できる上限を、5〜6では使わない。探索ノードが増える候補である。ただし呼出し頻度が高く、全安定石計算の費用で逆に遅くもなる。6だけ／5も／alpha閾値と安価な前段判定、を分けて比較するべきである。以前の「7〜13で安定石判定を外す」試行とは対象が違う。

この有無は今回の末端改善より前から同じなので、FFO44の最近の約2.5%悪化をこれだけで説明したことにはならない。

**6. 末端の表現と呼出し境界が違う**

現deftはplayer/opponentをu64二つで渡し、反転は一度128bitへ組み立ててから結果をu64へ戻す。Egaroucid [last2_nws:173](/workspace/Egaroucid/src/engine/endsearch_nws_last_simd.hpp:173) は盤面を__m128iで受け取り、Flip::calc_flip、board_flip_nextのXORとshuffle、その次の末端関数までSIMDの表現をつなぐ。Edaxの今回のendgame.cのsolve2/3はスカラー引数なので、この差をEdaxにも一律に適用しない。

現nativeではsolve2→solve1が実際の関数呼出しとして残り、vzeroupperと一部のYMM値のスタック保存・復帰が見える。solve1自身は6個のcallee-savedレジスタを保存する。手元のEdax nativeはsolve1がsolve2へインライン化され、solve2にcall命令がないことも確認した。ただしそのCバイナリとdeftはコンパイラ・制御機能まで同じ条件ではない。

現deftのsolve2のlocal stack領域は0x98、solve3は0xe8。solve3では一部の引数もスタックから読んでいる。関数サイズ・stack量は時間そのものではないが、末端の状態と呼出しを軽くする余地の具体的な根拠になる。むやみにinline(always)を付けると命令キャッシュを圧迫し得るため、先に最終表引きの検査とメタデータを減らし、生成コードを確認する。

**7. 安定石表のLazyLock確認が各辺に残る**

現deft [edge_stability.rs:128](/workspace/deft-reversi/deft-reversi-engine/src/board/edge_stability.rs:128) はEDGE_STABILITYを4回直接参照する。nativeでも、4辺の参照に対応した初期化状態のload/testが4組残る。初期化後も通る確認である。表を一度借りてから4回引く形なら、同じ安全な共有参照で確認をまとめる余地がある。

さらに表の作成はLazyLockで、Engine初期化時に強制する経路は見つからない。初めて安定石判定へ到達した探索では、表作成がEngine::searchの計時開始後に起こり得る。Edax main.c:128とEgaroucidのConsole初期化は探索前に表を作る。

「初期化を外へ動かす」は初回探索時間の内訳・比較条件の改善であり、処理自体を消した定常状態の高速化とは区別する。過去のdeft前後比較は双方同じLazyLockなので、今回これを見つけたことだけで前後比較を無効とはしない。ただし他エンジンとの短い新プロセス比較では、モデルやTTだけでなくこの初期化位置もそろえる必要がある。

**8. 安定石処理はSIMDの有無を正確に分ける必要がある**

現deft stability.rsはfull_axesと接触伝播を主にスカラーの4方向処理として記述している。Egaroucid stability.hppはSIMD時に斜線のfull処理を128bitでまとめ、接触伝播を256bitで4方向並列に処理する。Edaxの有効なget_full_linesもSSE2で横・斜線を処理する。

ただしEdaxの全面AVX2版と接触伝播AVX2版には`#if 0`と「Zen3では遅い」のコメントがある。Edaxの有効な接触伝播はスカラーであり、「Edaxは全部AVX2だから速い」は誤り。現deftのコンパイラが一部を自動SIMD化することもある。命令数だけで優劣を決めず、対象CPUに合わせて段階的に比べる。

現deftは4マス専用カーネルからも汎用null_window_upperを呼び、そのnative入口でempty_countのPOPCNTと閾値表参照が残る。4マスと分かる入口では閾値6を直接使う専用入口が候補になる。7〜13でも入口、子照会、手順付けの条件にempty_countの再計算があり、既知深さを利用する余地が残る。

**制御処理と、今回主因と断定しない差**

現deftのLeafSearch::visit→Context::enterは末端でもfailed確認、256ノードごとの停止確認、任意の厳密なnode_limit、統計更新を扱う。nativeのsolve2/3にはその複数の経路が取り込まれ、コード量を増やす。Edax solve2はローカルなノード数を最後にまとめ、EgaroucidのlastNは主に整数カウンタを更新する。

しかしnode_limit=Noneなら予算確保のatomic RMWは実行されない。逆アセンブルにlock cmpxchgが見えることだけで「通常探索が毎ノードatomicする」とは言えない。制御機能を消して速くする比較も同等ではない。上限なしの高速経路と厳密上限の経路を分ける場合でも、中断後に未完成のTT値を公開しない条件は残す。

共有TTの同期・prefetch不足、既定11〜13マスの手順付け情報の少なさ、Egaroucidの孤立空き優先との差は以前のレビューから残る。一方、局所TTの候補手不足・子の重複照会、6マスの既知合法手の再計算、6→5→4のparity再構築は既に改善した。今回それらを未修正の理由として数えていない。

ビルド条件にも注意する。EgaroucidのAVX512経路にはHAS_AVX512指定が必要であり、CPUや-march=nativeだけで常に選ばれるとは限らない。今回のcheckoutではUSE_CRC32C_HASH_LTTもfalse。無効な分岐を、実際の速度差の原因として扱わない。

**次に実装する順序**

1. 最終反転数の範囲を型で明示し、残る検査と線ごとのメタデータ読出しを減らす。生成コードを確認する。
2. MPCの静的ゲート、評価状態の再利用、整数スコアの確認用NWSを別々に比較する。係数・quality・中断安全性を維持する。
3. 最終1マスの合法性だけによるカットを、NWS専用に試す。
4. 5〜6の安定石カットを深さごとに試し、LazyLock参照の集約も分けて比較する。
5. SIMD盤面を保つ末端処理、安定石のSIMD化、上限なし制御経路の専用化は、変更範囲が大きいためその後にする。

**証拠ファイル**

assembly-manifest.jsonに現在のnativeバイナリのSHA-256とシンボル範囲を記録した（ff17c4b535fed25e8955c73c207ad85ebef4d8e1e2cdd131d6b702f6cbf4e207）。solve_score_1/2/3_empties.asm.txtとstability_null_window_upper.asm.txtはその逆アセンブル。範囲検査、関数呼出し、保存・復帰、LazyLockの状態確認はこれらで確認できる。Rust 1.99.1、release、target-cpu=native、既定feature、AMD EPYC 9V74のビルドであり、別ビルドで同じ命令列になる保証はしない。

今回は調査のみのため新しい速度改善率は報告しない。また、どの差がFFO44の悪化を説明するかも未確定である。
