2026-10-27 最終1マスの改善後に残る実装差

追記：この調査後、合法手生成の重複と14→13マスの評価状態更新を改善した。[実装と比較結果](../2026-10-27-ordering-reuse/README.md)を参照。以下は変更前の調査記録である。

対象はrefactor/engine-layout、基点8ab1682の作業ツリー。直前に採用した表引き・インライン化・安定石表参照の改善を含む。nativeバイナリはd75409b3fd8300f0688e0e9f336808c48f6c0db87c660431ac93a3c1a064dc55。Edaxは786421f355219ade872ce7285c1cab1eaf67836a、Egaroucidは2a65c87860be999373088ad833aa00b0844883b6。manifest.jsonにソースのハッシュと機械語の範囲を保存した。

今回はソース比較と既存native命令の確認のみ。実装差があることと、その差が実時間を何割説明するかは分ける。以下は追加の処理費用・失う情報・不足するカットを示すが、各変更で速くなることを測定したものではない。エンジンの探索コードは変更していない。

直前に解決した「PEXT結果の範囲検査」「線ごとの位置情報」「最終1マスの独立呼出し」「安定石表の4重LazyLock確認」は、未解決の理由に数えない。[直前の最適化](../2026-10-27-last-empty-optimization/README.md)を参照。

**比較の要点**

| 対象 | 現deft | Edax | Egaroucid |
|---|---|---|---|
| NWSの戻り値 | 14マス以上でもLineとPV配列を扱う | NWSは整数 | NWSは整数 |
| 14マス付近の候補評価 | 通常の44特徴評価＋評価内の両者の合法手数＋別の合法手数計算 | fast評価。実合法手・潜在合法手・隅の安定性等 | 通常の手順付け評価。合法手を保存して子へ渡す |
| 11〜13マスの既定NWS手順付け | 合法手数・隅重み・四分割parity | 実／潜在合法手、隅の安定性、升価値、parity、TT手 | 専用パターン評価＋合法手数（11〜13） |
| 5〜6マスの安定石カット | なし | あり | あり。パス直後は省く |
| 4マスの安定石カット | あり | あり | USE_LAST4_SC=false |
| 6マスのTT | 局所TTを照会・保存 | 浅い探索へ切替え、TTなし | fast探索へ切替え、6マス入口はTT照会なし |
| 7〜10マスの1合法手 | 局所TT照会・保存を行う | 局所TT照会・保存を省く | 親側で保存する経路があり、Edaxと同一ではない |
| 局所ハッシュ | P xor rotl(O,32) の後に1回乗算 | CRC32Cで両盤面を順に混合 | PとOを別々に乗算してからxor（有効な通常分岐） |

EdaxはDEPTH_TO_SHALLOW_SEARCH=6、DEPTH_TO_LOCAL_ENDGAME=10、DEPTH_MIDGAME_TO_ENDGAME=15。EgaroucidはEND_FAST_DEPTH=6、END_SIMPLE_DEPTH=10、MID_TO_END_DEPTH=13、MPC有効時の専用終盤入口は9。異なる深さの関数を同じものとして比較しない。

**1. 14マス以上のNWSも、PV付きの汎用関数を通る**

deft [tree.rs:62](../../../deft-reversi-engine/src/search/tree.rs)のnodeはResult<Line,()>を返す。ゼロ窓を整数専用の終盤へ渡すのは13マス以下だけ（76〜81行）。14マス以上のゼロ窓でも、Line::leafとLine::prependを扱う。

[session.rs:77](../../../deft-reversi-engine/src/search/session.rs)のLineには128要素のactionsがあり、leafは全要素をNoneで初期化する。prependは子の手順をコピーする。返した値を整数だけ使うNWSでも、汎用関数のインターフェース上はPVを保持する。nativeのnode_line_initialization_excerpt.asm.txtにはevaluate_state後の配列初期化が実際に残る。全初期化が全経路で同じ量だけ実行されるとまでは言わない。

Edax src/endgame.c:568のNWS_endgame、Egaroucid src/engine/midsearch_nws.hpp:337のnega_alpha_ordering_nwsは整数を返す。現deftでも13以下で既に行っている「スコアだけを返す」設計を、14以上のNWSへ広げる余地がある。PV探索本体、選択率タグ、上下限の合成、中断後の保存禁止は維持する必要がある。

**2. 候補の評価内と手順付け側で、同じ合法手を計算する**

deft [ordering.rs:36](../../../deft-reversi-engine/src/search/ordering.rs)は子の評価状態を作り、評価または浅いordering_searchを呼び、その後50行でchild.get_movesを呼ぶ。一方、[evaluation/mod.rs:84](../../../deft-reversi-engine/src/evaluation/mod.rs)のevaluate_stateは既にchild.get_movesとchild.get_opponent_movesを呼び、モデルのmobility項を計算する。

NWSのendgame_orderingではdepthが0または1なので、14マス以上で候補を通常評価する経路は、直接評価または深さ0のordering_searchになる。したがって子の現手番の合法手生成が二重になり、相手側の合法手生成も別途必要になる。TT候補で先にカットした場合や1候補の場合は、他候補のこの費用を払わない。

nativeでもevaluate_stateとordering_searchは独立した関数として残る。node_ordering_excerpt.asm.txtの0x59056でordering_searchを呼び、0x592afへ進み、0x592ed以後で合法手生成を再び行う。直接evaluate_stateを呼ぶ0x592a7からも同じ地点へ進む。単にソースに同じ式があるだけで、自動的に共有されたと判断していない。

Edax src/move.c:324の有効なAVX2分岐はget_moves_and_potentialで実合法手と潜在合法手をまとめる。14マスのNWSはsrc/endgame.c:633のmovelist_evaluate_fastを使い、この手順付けで通常のパターン評価は行わない。Egaroucidの14マス以上は通常のmove_evaluate_nwsを使う（src/engine/move_ordering.hpp:266）。n_legalを保存して再帰へ渡し、mid_evaluate_diff自体には合法手生成がない（evaluate_simd.hpp:529）。11〜13マスに使う後述の専用評価と、14以上の経路は区別する。

方向は、まず同じモデル値を保ったまま合法手を返す／渡すことで重複をなくすこと。通常評価のmobility項を削る変更は別のアイデアであり、同じ評価関数の実装改善とは分ける。

**3. 局所TTのハッシュに、確定する構造的な衝突がある**

deft [tail.rs:371](../../../deft-reversi-engine/src/search/tail.rs)は次のキーを作る。

```text
K = P xor rotl(O, 32)
index = wrapping_mul(K, 0x9e3779b97f4a7c15) >> 53
```

delta=rotl(delta,32)を満たすビット集合で両者の色を入れ替えると、

```text
P' = P xor delta
O' = O xor delta
K' = K xor delta xor rotl(delta,32) = K
```

になる。後から掛ける定数や表サイズを変えても、この時点で同じキーなので分離できない。例えば盤面index20と52の色を入れ替えた、6空き・自石数28で同じ占有升を持つ2盤面は、ともにスロット320へ入る。[hash-collision.json](hash-collision.json)に盤面と計算結果を保存した。

全盤面照合があるため誤った上下限を使う問題ではない。しかし局所TTは同じ層の同じスロットを上書きするので、これらの局面を両方訪ねれば保持情報を失う。構成した盤面が初期局面から到達可能だという証明や、実際のFFOでこの形の衝突が多いという測定はしていない。構造的な別名化の証明と、その出現頻度は別である。

Egaroucid src/engine/endsearch_nws.hpp:155の有効な乗算式では、この例のindexは218と958に分かれる。CRC32C分岐は今回の設定では無効なので根拠に使わない。Edax src/board.c:1459はPとOへ順にCRC32Cを適用する。これらのハッシュにも一般の衝突はあるが、deftの上記の式変形がそのまま成立するわけではない。

再現：`python3 docs/benchmarks/2026-10-27-post-leaf-source-review/reproduce_hash_collision.py`。スクリプトは現在のハッシュ式がソースに残っていることも確認する。乗算を混合前に分ける方式などは候補だが、混合費用と実際のヒット率を合わせて判断する。

**4. 5〜6マスは安定石で止めず、6マスはTTの費用を払う**

deft [tail.rs:420](../../../deft-reversi-engine/src/search/tail.rs)のsix_moves_scoreと467行のfive_empty_scoreには安定石判定がない。7〜13マスと4マスにはある。Edax src/endgame.c:422、Egaroucid src/engine/endsearch_nws.hpp:56には5〜6マスでの判定がある。

逆にdeftは6マスでも局所TTの入口を通り、照会・qualityの読出し・保存を行う。7マスの候補から6マスの子を照会する経路もある。Edaxの6以下はTTなしの浅い探索。Egaroucidも6以下のfast探索自身はTTを引かない。ただしEgaroucidでは7マスの親から6マスの子のTTを照会・保存する経路があり、「6マスの情報を一切保存しない」とは言えない。

さらにdeftの6マス経路にはbest_moveの受渡しがない。tail.rs:165でprobe.bestを取るが、169行でsix_moves_scoreへ渡さず、戻った後も更新せずに187行で保存する。新規スロットのbest=64はこの経路では有効な手に変わらない。したがって6マスのTTは上下限の利益だけで費用を回収する必要があり、「候補手も保存しているから6マスの手順付けに効く」とは評価できない。

6マスTTなし／上下限だけ／手も使う構成と、5・6マスそれぞれの安定石カットを分離して比較するべきである。TTを外してノードが増えても、実時間で速くなる場合がある。安定石を追加してノードが減っても、判定費用で遅くなる場合がある。

**5. 1合法手でも局所TT処理を行う**

deftはendgame_score_enteredで局所TTの初期化・照会を済ませた後、ordered_endgameの230行でmoves.is_power_of_twoを調べる。手順付けは省けるが、その局面自身のTT照会と戻り時の保存は行う。

Edax src/endgame.c:507〜535は複数合法手のときだけ局所TTを照会・保存し、1合法手は直ちに子へ進む。1合法手でもTTによる値のカットには意味があるので、省くと必ず速いとは言わない。親から子へのETC・渡されたEndProbe・既知合法手を活かしながら、この経路に固有の費用対効果を確認する余地がある。

**6. 11〜13マスは情報が少なく、通常評価を足すと重すぎるという隔たりが残る**

deftの既定NWSは[tree.rs:587](../../../deft-reversi-engine/src/search/tree.rs)の「隅を二重に数えた相手合法手数×-18＋parityの17」。Edaxのfast評価はこれに加えて潜在合法手、隅の安定性、升価値とTTの2候補を使う。

Egaroucidは11〜13マスでsrc/engine/endsearch_nws.hpp:377のmid_evaluate_move_ordering_endを使う。src/engine/evaluate_simd.hpp:492〜499では専用配列pattern_move_ordering_end_arrから、2組の8特徴を集計する。通常評価全体を呼ぶ構造ではない。

deftの実験featureは通常モデルの44特徴を使う。前の試作がノードを減らしても実時間で遅かったことは、この情報を使うアイデア自体の否定にならない。「終盤手順付け用の小さい特徴集合・係数」「差分更新の対象」「mobility項を含む通常評価の費用」を分けて調べる必要がある。既定featureを有効にするだけでEgaroucidと同じ実装にはならない。

5〜6マスではdeftは四分割parity優先、EgaroucidはUSE_END_PO=trueでempty1_bbによる孤立空き升優先を使う。empty1_bbは斜めも含む8近傍に他の空きがない升を返す。四分割領域が奇数であることと、幾何学的に孤立していることは別情報なので、同じ手順付けとは扱わない。

**7. 完全読みでも14→13の境界で不要な評価状態更新がある**

deft [tree.rs:302](../../../deft-reversi-engine/src/search/tree.rs)の最初の子、239行付近のTT候補、430行のsiblingは、親のempty_count()>13ならCandidate::childでEvalState::playedを実行する。

完全読み・実験featureなしのNWSで親14→子13へ進むと、子は76〜81行でBoardだけの局所探索へ入り、今更新したstateを使わない。MPC無効でも発生する境界の費用である。14マス側で既に候補評価のためにstate.playedを計算した場合でも、Candidateはその状態を保持しないので再帰前にもう一度更新する構造になる。

Egaroucidは専用終盤でmove_endsearch／move_noevalを区別し、Edaxもsearch_update_endgameで通常評価の更新を避ける。deftでも、子が本当に評価状態を使うかを窓・深さ・feature・sigmaから判断する必要がある。広い窓の13マスは現deftで依然評価を使うので、「14→13は常に更新不要」と一括で変えるのは誤りである。

**選択探索に限って残る差**

MPCの静的評価による方向選別がないこと、確認探索がTT・通常手順付けなしのordering_searchであること、選択率付き12〜13マスでEvalState::from_boardを作り直すことは、[前の調査](../2026-10-27-remaining-source-review/README.md)から未変更である。これらは選択探索と準備探索の遅さの候補であり、sigmaなしの完全読み本体の余分なノードを直接説明するものではない。MPCの誤差・丸め・選択率の安全な分離を維持する必要がある。

**判断と次の優先順位**

まず同じ値・同じ探索規則で削れる、評価内外の合法手生成重複と14→13の不要な状態更新を確認する。次に局所ハッシュの混合式、6マスTTと1合法手のTT処理、5〜6マスの安定石カットを、それぞれ独立した試作にする。14以上の整数NWSと専用の軽い手順付け評価は、より広い変更として扱う。

大きい局所TTが必ず有利とも判断しない。deftは2048エントリ×8層、Egaroucidは1024×7層、Edaxの局所表は1024エントリを使う設定であり、保持量とキャッシュへの収まりの交換条件が違う。ただしサイズ差だけで遅さを断定しない。

未修正の中断・予算処理にも費用はあるが、node_limit=Noneなら毎ノードatomic加算するわけではない。Edaxにも共有TTの同期があり、「Rustのatomicだから遅い」という説明はしない。EdaxのUSE_IIDとUSE_SOLIDはfalseであり、無効な機能を差の根拠にしない。

今回はハッシュ衝突の再現スクリプトとソース・機械語の照合まで行った。速度測定・エンジンの変更・新規のエンジンテストは実施していない。どの差が実時間の主因かは、実装と探索量を分けて検証する段階に残る。


後続の修正: 局所TTの混合式と終盤探索の配置は [レビュー後の修正・検証](../2026-10-27-review-fixes/README.md) で変更した。この文書と衝突再現スクリプトは調査時点のソースを対象とする。現在のコードでは `search/endgame/local_tt.rs` の回帰テストを使う。
