//! 中盤および広域終盤ノードの Alpha-Beta / PVS 探索木を実装するモジュールです。
//!
//! 置換表（TT：一度調べた局面の再利用）、ETC（Enhanced Transposition Cutoff：子局面を事前に置換表で調べて
//! 枝刈りする手法）、確定石による上界枝刈り、Multi-ProbCut（MPC：浅い探索から枝刈りを統計予測する手法）、
//! YBWC 並列分割、および葉近傍の簡易探索（`simple_search`）を統合しています。
use super::endgame::parity::odd_regions;
use super::*;
use crate::board::CORNERS;
use std::ops::ControlFlow;
// 中盤ノードは残り深さ 4 手（4 プライ）まで並列分割を許可します。
// 終盤完全読み直前の浅い準備イテレーションでスレッドが遊ばないようにするためです。
pub(super) const SPLIT_DEPTH: u8 = 4;
/// スレッド数が少ない場合、および完全読み前の選択ステージにおいて、
/// 完全読みノードが YBWC 分割を行う最小空きマス数です。
/// 14 マス未満ではルートだけ分割可能で、ローカル終盤ソルバ（`LOCAL_END_MAX`）は分割しません。
pub(super) const SPLIT_EMPTIES: u8 = 10;
/// 最終完全読みステージで YBWC 分割を行う最小空きマス数をスレッド数に応じて返します。
/// スレッド数が多い環境では常に空きスレッドが存在するため、小さすぎる分割はハンドオフと合流（join）の
/// オーバーヘッドが並列化の削減効果を上回ります（20 スレッド FFO 40-59 で 14→17 により 10% 高速化、
/// 4 スレッドでは 14 が最速でした。選択ステージは枝刈りが強いため `SPLIT_EMPTIES` を維持します）。
pub(super) fn split_empties(threads: usize) -> u8 {
    if threads < 8 {
        SPLIT_EMPTIES
    } else {
        13 + threads.min(16) as u8 / 4
    }
}
/// Null-Window（幅 1 の窓）の中盤ノードでこの深さ未満なら置換表をスキップし、`ORDERED_SEARCH_DEPTH` 未満なら
/// ムーブオーダリング（候補手の並び替え）もスキップします（旧 `SWITCH_SIMPLE_SEARCH_LEVEL` / `SWITCH_NEGAALPHA_SEARCH_LEVEL` 相当）。
/// 小型ニューラルネット併用時は 4 と 3 が最速でした（線形評価のみでは 5 と 3）。
pub(super) const SIMPLE_SEARCH_DEPTH: u8 = 4;
pub(super) const ORDERED_SEARCH_DEPTH: u8 = 3;
/// 静的評価からの回帰予測値が現在の探索境界のこのマージン（石数）以内に収まるときだけ
/// ProbCut の浅い探索を実行します（Egaroucid は 4、小型ニューラルネット併用評価向けに上界マージンを調整済みです）。
const PROBCUT_MIDGAME_UPPER_MARGIN: f64 = 8.0;
const PROBCUT_EXACT_UPPER_MARGIN: f64 = 2.0;
const PROBCUT_LOWER_MARGIN: f64 = 4.0;
/// 終盤ノード（13 マス以上〜汎用探索木）のムーブオーダリング重みです。
struct OrderingWeights {
    mobility: i32,
    corner: i32,
    tt: i32,
    parity: i32,
}
const ORDERING: OrderingWeights = OrderingWeights {
    mobility: 16,
    corner: 128,
    tt: 1 << 20,
    parity: 17,
};

impl Context {
    /// スレッドローカルな探索コンテキストを生成します。
    pub(super) fn new(
        runtime: Arc<Runtime>,
        ancestors: Vec<Arc<Helper>>,
        cuts: Vec<Arc<Padded<AtomicBool>>>,
    ) -> Self {
        Self {
            runtime,
            stats: SearchStats::default(),
            ancestors,
            cuts,
            end_cache: EndgameCache::default(),
            #[cfg(feature = "endgame-pattern-ordering")]
            end_ordering: [None; 3],
        }
    }
    /// 現在のステージにおいて完全読みノードが並列分割を行う最小空きマス数を返します。
    pub(super) fn exact_split_empties(&self) -> u8 {
        if self.runtime.selective() {
            SPLIT_EMPTIES
        } else {
            self.runtime.split_empties
        }
    }
    /// 全体の停止条件または祖先分割点でのベータカットが発生しているかを返します。
    pub(super) fn aborted(&self) -> bool {
        self.runtime.control.check() || self.cuts.iter().any(|cut| cut.load(Ordering::Acquire))
    }
    /// ノード訪問時に呼び出し、中断チェックとノード数上限の予約を行います。
    pub(super) fn enter(&mut self) -> Result<(), Aborted> {
        // 停止要求・時間制限のポーリングは負荷軽減のため 256 ノードごとに間引きますが、
        // ハードなノード上限（`node_limit`）は訪問ノードごとに厳密に 1 ずつ予約します。
        if self.stats.nodes & 255 == 0 && self.aborted() {
            return Err(Aborted);
        }
        if let Some(limit) = self.runtime.control.node_limit
            && self
                .runtime
                .control
                .issued
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    if n < limit { Some(n + 1) } else { None }
                })
                .is_err()
        {
            self.runtime.control.set(StopReason::NodeLimit);
            return Err(Aborted);
        }
        self.stats.nodes += 1;
        Ok(())
    }
    /// 差分評価状態を用いて静的評価値を計算します。
    pub(super) fn evaluate(&mut self, board: Board, state: &EvalState) -> i32 {
        self.stats.evaluations += 1;
        self.runtime.model.evaluate_state(&board, state)
    }
    /// 静的評価値と、そのモビリティ計算で副産物として得られた合法手ビットマスクを返します。
    pub(super) fn evaluate_and_moves(&mut self, board: Board, state: &EvalState) -> (i32, u64) {
        self.stats.evaluations += 1;
        self.runtime.model.evaluate_state_and_moves(&board, state)
    }
    /// 子ノードで `EvalState` の差分更新が必要かを判定します（完全読みローカル領域では不要な場合が多いです）。
    fn child_needs_eval(&self, board: Board, exact: bool, narrow: bool) -> bool {
        if !exact {
            return true;
        }
        let empties = board.empty_count();
        if empties > super::endgame::LOCAL_END_MAX + 1 {
            return true;
        }
        let selective = self.runtime.selective();
        if empties == super::endgame::LOCAL_END_MAX + 1 {
            // 空き 13 マスの広域窓子ノードはムーブオーダリングで状態を使います。
            // 実験的パターンオーダリング有効時は狭域入口でもキャッシュの種まきに使います。
            return !narrow || selective || cfg!(feature = "endgame-pattern-ordering");
        }
        selective
    }
    /// 汎用探索ノード（PVS / Alpha-Beta）のエントリポイントです。
    pub(super) fn node(
        &mut self,
        position: Position,
        mut alpha: i32,
        mut beta: i32,
        spec: SearchSpec,
    ) -> Result<Line, Aborted> {
        let Position { board, state } = position;
        let SearchSpec {
            depth,
            exact,
            probcut: allow_probcut,
        } = spec;
        // 浅い Null-Window 中盤ノードは置換表をスキップして軽量探索へ流します。
        if !exact && beta - alpha == 1 && (1..SIMPLE_SEARCH_DEPTH).contains(&depth) {
            return self
                .simple_search(position, alpha, depth, allow_probcut)
                .map(Line::leaf);
        }
        self.enter()?;
        if exact && beta - alpha == 1 && board.empty_count() <= super::endgame::LOCAL_END_MAX {
            #[cfg(feature = "endgame-pattern-ordering")]
            if board.empty_count() == super::endgame::LOCAL_END_MAX {
                self.end_ordering[2] = Some((board, state));
            }
            return self.tail_score_entered(board, alpha, beta).map(Line::leaf);
        }
        if exact && board.empty_count() <= super::endgame::TAIL_EMPTIES {
            return self.tail_entered(board, alpha, beta);
        }
        // 呼び出し元の元の窓幅を保持します（置換表格納時の上下界種別判定に使います）。
        let window = (alpha, beta);
        let moves = board.get_moves();
        if moves == 0 {
            if board.get_opponent_moves() == 0 {
                self.stats.leaves += 1;
                return Ok(Line::leaf(board.final_score()));
            }
            let child = self.node(
                Position {
                    board: board.board_after_pass(),
                    state: state.passed(),
                },
                -beta,
                -alpha,
                spec,
            )?;
            return Ok(Line::prepend(Action::Pass, &child));
        }
        if depth == 0 {
            self.stats.leaves += 1;
            return Ok(Line::leaf(self.evaluate(board, &state)));
        }
        if !exact && depth == 1 {
            return self.one_ply(position, alpha, beta, allow_probcut);
        }
        let quality = if allow_probcut {
            self.runtime.quality.load(Ordering::Relaxed)
        } else {
            0
        };
        let cached = self.probe_tables(board, spec, quality, beta - alpha > 1);
        let mut known_lower = -64;
        if let Some(entry) = cached {
            self.stats.tt_hits += 1;
            if fits(entry, spec, quality) {
                let lower = i32::from(entry.lower);
                let upper = i32::from(entry.upper);
                let cut = if upper <= alpha {
                    Some(upper)
                } else if lower >= beta || lower == upper {
                    Some(lower)
                } else {
                    None
                };
                if let Some(score) = cut {
                    self.stats.tt_cuts += 1;
                    return Ok(Line::leaf(score));
                }
                alpha = alpha.max(lower);
                beta = beta.min(upper);
                known_lower = lower;
            }
        }
        if exact
            && beta - alpha == 1
            && let Some(score) = crate::board::stability::null_window_upper(board, alpha)
        {
            self.stats.stability_cuts += 1;
            return Ok(Line::leaf(score));
        }
        if allow_probcut
            && let Some(score) = self.probcut(board, state, depth, alpha, beta, exact)?
        {
            return Ok(Line::leaf(score));
        }
        // 候補手リストは最大 64 要素の固定配列で持ち、ノードごとのヒープ割り当てを避けます。
        let mut candidates = [Candidate {
            square: Square::lowest(1),
            flips: 0,
            ordering: 0,
        }; 64];
        let bounds = ChildBounds {
            original_alpha: window.0,
            alpha,
            beta,
            known_lower,
        };
        let (mut count, skipped_upper, bounds) =
            match self.child_tt_pass(board, moves, spec, quality, bounds, &mut candidates) {
                ControlFlow::Continue(pass) => pass,
                ControlFlow::Break(score) => {
                    self.stats.tt_cuts += 1;
                    return Ok(Line::leaf(score));
                }
            };
        let ChildBounds {
            mut alpha,
            known_lower,
            ..
        } = bounds;
        let child_spec = SearchSpec {
            depth: depth - 1,
            ..spec
        };
        // 子 TT カットオフ（ETC）で安価に枝刈りできるものを先に処理してから、置換表に記録された最善手の探索へ進みます。
        // 残りの手はこの時点ではまだ評価関数を通していません。
        let tt_move = cached.map_or(64, |e| e.best);
        let preferred =
            if let Some(index) = (0..count).find(|&i| candidates[i].square.index() == tt_move) {
                let candidate = candidates[index];
                candidates.copy_within(index + 1..count, index);
                count -= 1;
                let child = self.node(
                    candidate.child(
                        position,
                        self.child_needs_eval(board, exact, beta - alpha == 1),
                    ),
                    -beta,
                    -alpha,
                    child_spec,
                )?;
                let mut line = Line::prepend(candidate.action(), &child);
                alpha = alpha.max(line.score);
                if alpha >= beta {
                    self.stats.beta_cuts += 1;
                    if self.aborted() {
                        return Err(Aborted);
                    }
                    line.score = line.score.max(known_lower);
                    self.store_node_line(board, &line, window, spec, quality);
                    return Ok(line);
                }
                Some(line)
            } else {
                None
            };
        let candidates = &mut candidates[..count];
        let second = cached.map_or(64, |e| e.second);
        self.score_moves(position, candidates, alpha, beta, spec, second)?;
        let (start, first) = if let Some(line) = preferred {
            (0, line)
        } else {
            pick_best(candidates, 0);
            let first = candidates[0];
            let child = self.node(
                first.child(
                    position,
                    self.child_needs_eval(board, exact, beta - alpha == 1),
                ),
                -beta,
                -alpha,
                child_spec,
            )?;
            (1, Line::prepend(first.action(), &child))
        };
        let mut best = self.search_siblings(
            position,
            candidates,
            start,
            first,
            (alpha, beta),
            child_spec,
        )?;
        if self.aborted() {
            return Err(Aborted);
        }
        if skipped_upper > best.score {
            best = Line::leaf(skipped_upper);
        }
        best.score = best.score.max(known_lower);
        self.store_node_line(board, &best, window, spec, quality);
        Ok(best)
    }
    /// `board` に対応する置換表エントリを取得します。
    /// 広域の完全読み窓の場合は PV 専用テーブルも参照し、両方適合すれば窓が狭い方（より情報が正確な方）を優先します。
    // この関数と続く 3 つのヘルパーは `node` の一部であり、1 スレッド時 0.6% の呼び出しコストを避けるためインライン化しています。
    #[inline(always)]
    fn probe_tables(
        &self,
        board: Board,
        spec: SearchSpec,
        quality: u8,
        wide: bool,
    ) -> Option<Entry> {
        let main = self.runtime.table.get(board);
        let pv = if spec.exact && wide {
            self.runtime.pv_table.get(board)
        } else {
            None
        };
        let matches = |entry: Entry| fits(entry, spec, quality);
        match (pv, main) {
            (Some(pv), Some(main)) if matches(pv) && matches(main) => Some(
                if i32::from(pv.upper) - i32::from(pv.lower)
                    <= i32::from(main.upper) - i32::from(main.lower)
                {
                    pv
                } else {
                    main
                },
            ),
            (Some(pv), _) if matches(pv) => Some(pv),
            (_, Some(main)) if matches(main) => Some(main),
            _ => pv.or(main),
        }
    }
    /// Enhanced Transposition Cutoff（ETC）：すべての局面を実際に展開する前に子局面をメイン置換表で事前照会し、
    /// 探索が必要な手だけを `candidates` に詰めて、その個数とスキップした手の最大上界を返します。
    /// 1 手でもベータカットが確定した場合はそのスコアで即座に `Break` します。
    #[inline(always)]
    fn child_tt_pass(
        &mut self,
        board: Board,
        mut moves: u64,
        spec: SearchSpec,
        quality: u8,
        mut bounds: ChildBounds,
        candidates: &mut [Candidate; 64],
    ) -> ControlFlow<i32, (usize, i32, ChildBounds)> {
        let SearchSpec { depth, exact, .. } = spec;
        let mut count = 0;
        let mut skipped_upper = -65;
        while moves != 0 {
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            let flips = board.flip_mask(bit);
            // 空き 13 マス以下の Null-Window 子ノードはローカル終盤キャッシュだけに存在するため、
            // メイン置換表での照会は無駄になります。パス中に窓が狭まる可能性があるため毎手判定します。
            let probe = if exact {
                board.empty_count() > 12 + 2 * u8::from(bounds.beta - bounds.alpha == 1)
            } else {
                depth >= 4
            };
            if probe
                && let Some(entry) = self.runtime.table.get(board.after_known_move(bit, flips))
                && entry.exact_mode == exact
                && entry.depth == depth - 1
                && entry.quality == quality
            {
                let lower = -i32::from(entry.upper);
                let upper = -i32::from(entry.lower);
                if lower >= bounds.beta {
                    return ControlFlow::Break(lower);
                }
                // 参照 ETC と同様に元の alpha を基準にスキップします。
                // fail-low（すべての手が alpha 以下で終わる場合）の上界の正しさを保つため、
                // スキップした手の最大上界を記録しておきます。
                if upper <= bounds.original_alpha {
                    skipped_upper = skipped_upper.max(upper);
                    continue;
                }
                bounds.alpha = bounds.alpha.max(lower);
                bounds.known_lower = bounds.known_lower.max(lower);
            }
            candidates[count] = Candidate {
                square: Square::lowest(bit),
                flips,
                ordering: 0,
            };
            count += 1;
        }
        if count == 0 {
            return ControlFlow::Break(skipped_upper);
        }
        ControlFlow::Continue((count, skipped_upper, bounds))
    }
    /// 各候補手にオーダリングキーを付与します。`second` は置換表の次善手インデックスです。
    #[inline(always)]
    fn score_moves(
        &mut self,
        position: Position,
        candidates: &mut [Candidate],
        alpha: i32,
        beta: i32,
        spec: SearchSpec,
        second: u8,
    ) -> Result<(), Aborted> {
        let SearchSpec { depth, exact, .. } = spec;
        let board = position.board;
        let only_move = candidates.len() <= 1;
        let odd_regions = if exact { odd_regions(board) } else { 0 };
        let tt_priority = if exact { ORDERING.tt } else { 1 << 8 };
        for candidate in candidates {
            let bit = candidate.square.bit();
            let flips = candidate.flips;
            let child = board.after_known_move(bit, flips);
            let mut ordering = if only_move {
                0
            } else if !exact {
                self.midgame_ordering(position, child, bit, flips, alpha, depth)?
            } else if board.empty_count() >= super::endgame::LOCAL_END_MAX {
                self.endgame_ordering(position, child, bit, flips, alpha, beta)?
            } else {
                let mut value = -(child.get_moves().count_ones() as i32) * ORDERING.mobility;
                if bit & odd_regions != 0 {
                    value += ORDERING.parity;
                }
                if bit & CORNERS != 0 {
                    value += ORDERING.corner;
                }
                value
            };
            if candidate.square.index() == second {
                ordering += tt_priority / 2;
            }
            candidate.ordering = ordering;
        }
        Ok(())
    }
    /// 先頭手探索後、残りの兄弟候補 `candidates[start..]` を探索します。
    /// 空きスレッドがあれば YBWC 分割（[`split`](Self::split)）へ移行します。
    #[inline(always)]
    fn search_siblings(
        &mut self,
        position: Position,
        candidates: &mut [Candidate],
        start: usize,
        mut best: Line,
        (alpha, beta): (i32, i32),
        child_spec: SearchSpec,
    ) -> Result<Line, Aborted> {
        let mut alpha = alpha.max(best.score);
        if alpha >= beta {
            self.stats.beta_cuts += 1;
            return Ok(best);
        }
        let count = candidates.len();
        let depth = child_spec.depth + 1;
        let can_split = self.runtime.parallel
            && child_spec.probcut
            && if child_spec.exact {
                position.board.empty_count() >= self.exact_split_empties()
            } else {
                depth >= SPLIT_DEPTH
            };
        let mut index = start;
        while index < count {
            // 参照 `node_split` と同様に各兄弟の終了ごとに再試行します。
            // その間に空いたスレッドが次の分割点を待たずに残り候補を引き受けられるようにするためです。
            let enough = if index == start {
                count >= 3
            } else {
                count - index >= 2
            };
            if can_split && enough && self.runtime.pool.can_offer(&self.ancestors) {
                return self.split(
                    position,
                    candidates[index..].to_vec(),
                    (alpha, beta),
                    child_spec,
                    best,
                    None,
                );
            }
            pick_best(candidates, index);
            let line = self.sibling(position, candidates[index], alpha, beta, child_spec)?;
            index += 1;
            if line.score > best.score {
                best = line;
            }
            alpha = alpha.max(best.score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                break;
            }
        }
        Ok(best)
    }
    fn store_node_line(
        &mut self,
        board: Board,
        line: &Line,
        (alpha, beta): (i32, i32),
        spec: SearchSpec,
        quality: u8,
    ) {
        let (lower, upper) = if line.score <= alpha {
            (-64, line.score as i8)
        } else if line.score >= beta {
            (line.score as i8, 64)
        } else {
            (line.score as i8, line.score as i8)
        };
        let best_move = match line.actions[0] {
            Some(Action::Place(s)) => s.index(),
            _ => 64,
        };
        let entry = Entry {
            lower,
            upper,
            depth: spec.depth,
            quality,
            best: best_move,
            second: 64,
            generation: self.runtime.generation,
            exact_mode: spec.exact,
        };
        self.runtime.table.store(board, entry);
        if spec.exact && beta - alpha > 1 {
            self.runtime.pv_table.store(board, entry);
        }
    }
    /// 2 番目以降の手を Null-Window で探索し、窓内（`alpha < score < beta`）に入った場合だけフルウィンドウで再探索します。
    pub(super) fn sibling(
        &mut self,
        position: Position,
        candidate: Candidate,
        alpha: i32,
        beta: i32,
        spec: SearchSpec,
    ) -> Result<Line, Aborted> {
        let update_eval = self.child_needs_eval(position.board, spec.exact, true);
        let next = candidate.child(position, update_eval);
        let child = self.node(next, -alpha - 1, -alpha, spec)?;
        let mut line = Line::prepend(candidate.action(), &child);
        if line.score > alpha && line.score < beta {
            let next = if !update_eval && self.child_needs_eval(position.board, spec.exact, false) {
                candidate.child(position, true)
            } else {
                next
            };
            let child = self.node(next, -beta, -alpha, spec)?;
            line = Line::prepend(candidate.action(), &child);
        }
        Ok(line)
    }
    /// Multi-ProbCut（MPC）：浅い探索結果と回帰モデルを用いて、高い確率でベータカットまたはアルファカットを判定します。
    pub(super) fn probcut(
        &mut self,
        board: Board,
        state: EvalState,
        depth: u8,
        alpha: i32,
        beta: i32,
        exact: bool,
    ) -> Result<Option<i32>, Aborted> {
        // 現在のイテレーションの選択率タグです（0 は全幅探索のため枝刈りしません）。
        let quality = self.runtime.quality.load(Ordering::Relaxed);
        if quality == 0 {
            return Ok(None);
        }
        // Edax と同様、Null-Window ノードだけで ProbCut を試みます。
        // 広い窓は PV ノードであり、誤って枝刈りしたときの悪影響が最も大きいためです。
        if beta - alpha > 1 {
            return Ok(None);
        }
        let sigma = self.runtime.sigmas.get(exact, quality);
        let Some(rule) = self
            .runtime
            .rules
            .get(rule_index(exact, depth, board.empty_count()))
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        self.stats.probcut_tries += 1;
        // 静的評価による予測値が境界から遠すぎる場合は、浅い探索自体をスキップします。
        let predicted = rule.slope * f64::from(self.evaluate(board, &state)) + rule.intercept;
        let upper_margin = if exact {
            PROBCUT_EXACT_UPPER_MARGIN
        } else {
            PROBCUT_MIDGAME_UPPER_MARGIN
        };
        let high =
            ((f64::from(beta) - rule.intercept + sigma * rule.deviation) / rule.slope).ceil();
        if high < 64.0 && predicted >= high - upper_margin {
            let bound = high.max(-64.0) as i32;
            let score = self.probcut_shallow(board, state, bound - 1, bound, rule.shallow_depth)?;
            if score >= bound {
                self.stats.probcut_cuts += 1;
                // 完全読みスコアは常に偶数（空き 60 マスの標準盤面）であるため、
                // `beta` 以上を意味する切り上げ偶数へ丸めて奇数窓端がルートへ漏れるのを防ぎます。
                return Ok(Some(if exact { beta + (beta & 1) } else { beta }));
            }
        }
        let low =
            ((f64::from(alpha) - rule.intercept - sigma * rule.deviation) / rule.slope).floor();
        if low > -64.0 && predicted <= low + PROBCUT_LOWER_MARGIN {
            let bound = low.min(64.0) as i32;
            let score = self.probcut_shallow(board, state, bound, bound + 1, rule.shallow_depth)?;
            if score <= bound {
                self.stats.probcut_cuts += 1;
                return Ok(Some(if exact { alpha - (alpha & 1) } else { alpha }));
            }
        }
        Ok(None)
    }
    /// ProbCut 用の浅い探索です。ルール測定時と同じ条件（ProbCut なしの純粋なミニマックス中盤探索）で実行します。
    fn probcut_shallow(
        &mut self,
        board: Board,
        state: EvalState,
        alpha: i32,
        beta: i32,
        depth: u8,
    ) -> Result<i32, Aborted> {
        let spec = SearchSpec {
            depth,
            exact: false,
            probcut: false,
        };
        Ok(self
            .node(Position { board, state }, alpha, beta, spec)?
            .score)
    }
    /// 葉近傍の Null-Window 中盤探索です（置換表を使わず、評価値とモビリティでオーダリングします。
    /// `ORDERED_SEARCH_DEPTH` 未満ではオーダリングも省略し、窓外の値もそのまま返す Fail-soft 方式で動作します）。
    fn simple_search(
        &mut self,
        position: Position,
        alpha: i32,
        depth: u8,
        allow_probcut: bool,
    ) -> Result<i32, Aborted> {
        let beta = alpha + 1;
        if depth < ORDERED_SEARCH_DEPTH {
            return self.ordering_search(position, alpha, beta, depth);
        }
        self.enter()?;
        let Position { board, state } = position;
        let mut moves = board.get_moves();
        if moves == 0 {
            if board.get_opponent_moves() == 0 {
                self.stats.leaves += 1;
                return Ok(board.final_score());
            }
            let passed = Position {
                board: board.board_after_pass(),
                state: state.passed(),
            };
            return Ok(-self.simple_search(passed, -beta, depth, allow_probcut)?);
        }
        if allow_probcut
            && let Some(score) = self.probcut(board, state, depth, alpha, beta, false)?
        {
            return Ok(score);
        }
        let mut candidates = [Candidate {
            square: Square::lowest(1),
            flips: 0,
            ordering: 0,
        }; 64];
        let mut count = 0;
        while moves != 0 {
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            candidates[count] = Candidate {
                square: Square::lowest(bit),
                flips: board.flip_mask(bit),
                ordering: 0,
            };
            count += 1;
        }
        let candidates = &mut candidates[..count];
        if count > 1 {
            for candidate in candidates.iter_mut() {
                let bit = candidate.square.bit();
                let child = board.after_known_move(bit, candidate.flips);
                candidate.ordering =
                    self.midgame_ordering(position, child, bit, candidate.flips, alpha, depth)?;
            }
        }
        let mut best = -65;
        for index in 0..count {
            pick_best(candidates, index);
            let child = candidates[index].child(position, true);
            let score = -self.simple_search(child, -beta, depth - 1, allow_probcut)?;
            if score >= beta {
                self.stats.beta_cuts += 1;
                return Ok(score);
            }
            best = best.max(score);
        }
        Ok(best)
    }
}
/// 置換表エントリが現在の探索仕様（深さ・品質・完全読みモード）に適合し、その上下界が有効かを判定します。
fn fits(entry: Entry, spec: SearchSpec, quality: u8) -> bool {
    entry.depth == spec.depth && entry.quality == quality && entry.exact_mode == spec.exact
}
/// 子 TT パス（ETC）が読み取り・更新する境界値です。
#[derive(Clone, Copy)]
struct ChildBounds {
    original_alpha: i32,
    alpha: i32,
    beta: i32,
    known_lower: i32,
}
/// `candidates[index..]` の中で最もオーダリングスコアが高い要素を `index` 位置へスワップします。
pub(super) fn pick_best(candidates: &mut [Candidate], index: usize) {
    let best = (index..candidates.len())
        .max_by_key(|&i| candidates[i].ordering)
        .unwrap();
    candidates.swap(index, best);
}

impl Context {
    // 深さ 1 ではオーダリングのために子局面を評価すると同じ葉を 2 回評価することになるため、直接探索します。
    fn one_ply(
        &mut self,
        position: Position,
        mut alpha: i32,
        beta: i32,
        probcut: bool,
    ) -> Result<Line, Aborted> {
        let mut moves = position.board.get_moves();
        let mut best = Line::leaf(-65);
        while moves != 0 {
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            let flips = position.board.flip_mask(bit);
            let next = Position {
                board: position.board.after_known_move(bit, flips),
                state: position.state.played(bit, flips),
            };
            let child = self.node(
                next,
                -beta,
                -alpha,
                SearchSpec {
                    depth: 0,
                    exact: false,
                    probcut,
                },
            )?;
            if -child.score > best.score {
                best = Line::prepend(Action::Place(Square::lowest(bit)), &child);
            }
            alpha = alpha.max(best.score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                break;
            }
        }
        Ok(best)
    }
}

#[cfg(test)]
mod tt_tests {
    use super::super::session::test_context;
    use super::*;
    #[test]
    fn thirteen_empty_research_matches_eager_evaluation_state() {
        let model = Arc::new(PatternModel::nonzero_for_tests());
        for seed in [17u64, 101] {
            let board = crate::test_util::random_board(seed, 14);
            assert_eq!(board.empty_count(), 14);
            let moves = board.get_moves();
            assert_ne!(moves, 0);
            let bit = moves.isolate_lowest_one();
            let candidate = Candidate {
                square: Square::lowest(bit),
                flips: board.flip_mask(bit),
                ordering: 0,
            };
            let position = Position {
                board,
                state: EvalState::from_board(board),
            };
            let next = candidate.child(position, true);
            let spec = SearchSpec {
                depth: 13,
                exact: true,
                probcut: false,
            };
            let make_context = || {
                let mut context = test_context();
                Arc::get_mut(&mut context.runtime).unwrap().model = model.clone();
                context
            };
            // sibling() がフルウィンドウ再探索を行う必要があるため、窓の内側のスコアを探します。
            let expected = -make_context().node(next, -64, 64, spec).unwrap().score;
            assert!((-64..64).contains(&expected));
            let alpha = expected - 1;
            let beta = 64;
            let mut eager = make_context();
            let narrow = eager.node(next, -alpha - 1, -alpha, spec).unwrap();
            assert!(-narrow.score > alpha && -narrow.score < beta);
            let reference = eager.node(next, -beta, -alpha, spec).unwrap();

            let mut lazy = make_context();
            let actual = lazy
                .sibling(position, candidate, alpha, beta, spec)
                .unwrap();
            assert_eq!(actual.score, -reference.score);
            assert_eq!(
                actual.moves(),
                Line::prepend(candidate.action(), &reference).moves()
            );
            assert!(lazy.stats.evaluations > 0);
            assert_eq!(lazy.stats.nodes, eager.stats.nodes);
            assert_eq!(lazy.stats.evaluations, eager.stats.evaluations);
        }
    }
    #[test]
    fn forced_move_skips_ordering_evaluation() {
        let board = Board::from_bits(1, 2).unwrap();
        assert!(board.get_moves().is_power_of_two());
        let mut context = test_context();
        let line = context
            .node(
                Position {
                    board,
                    state: EvalState::from_board(board),
                },
                -64,
                64,
                SearchSpec {
                    depth: 3,
                    exact: false,
                    probcut: false,
                },
            )
            .unwrap();
        assert_eq!(line.score, 64);
        assert_eq!(context.stats.evaluations, 0);
        assert_eq!(context.stats.nodes, 2);
    }
    fn entry(depth: u8, lower: i8, upper: i8) -> Entry {
        Entry {
            lower,
            upper,
            depth,
            quality: 0,
            best: 19,
            second: 64,
            generation: 1,
            exact_mode: false,
        }
    }
    #[test]
    fn tt_move_cuts_before_other_moves_are_evaluated() {
        let board = Board::new();
        let bit = 1u64 << (63 - board.get_moves().leading_zeros());
        let mut context = test_context();
        let mut hint = entry(1, -64, 64);
        hint.best = bit.trailing_zeros() as u8;
        context.runtime.table.store(board, hint);
        context
            .runtime
            .table
            .store(board.board_after_move(bit), entry(2, -7, -7));
        // 広域窓: 浅い Null-Window 中盤ノードは TT をスキップし、深さ 4 以上では子 TT パスが先に枝刈りします。
        let line = context
            .node(
                Position {
                    board,
                    state: EvalState::from_board(board),
                },
                -1,
                1,
                SearchSpec {
                    depth: 3,
                    exact: false,
                    probcut: false,
                },
            )
            .unwrap();
        assert_eq!(line.score, 7);
        assert_eq!(context.stats.nodes, 2);
        assert_eq!(context.stats.evaluations, 0);
        let saved = context.runtime.table.get(board).unwrap();
        assert_eq!(saved.best, hint.best);
        assert_eq!(saved.lower, 7);
        assert_eq!(saved.depth, 3);
    }
    #[test]
    fn child_tt_cut_precedes_tt_move_search() {
        let board = Board::new();
        let bit = 1u64 << (63 - board.get_moves().leading_zeros());
        let mut context = test_context();
        context.runtime.table.store(board, entry(1, -64, 64));
        context.runtime.table.store(
            board.board_after_move(bit),
            entry(SIMPLE_SEARCH_DEPTH, -7, -7),
        );
        let spec = SearchSpec {
            depth: SIMPLE_SEARCH_DEPTH + 1,
            exact: false,
            probcut: true,
        };
        let position = Position {
            board,
            state: EvalState::from_board(board),
        };
        assert_eq!(context.node(position, -1, 0, spec).unwrap().score, 7);
        assert_eq!(context.stats.nodes, 1);
        assert_eq!(context.stats.evaluations, 0);
    }
    fn search(context: &mut Context, alpha: i32, beta: i32) -> i32 {
        let board = Board::new();
        context
            .node(
                Position {
                    board,
                    state: EvalState::from_board(board),
                },
                alpha,
                beta,
                SearchSpec {
                    depth: 4,
                    exact: false,
                    probcut: true,
                },
            )
            .unwrap()
            .score
    }
    #[test]
    fn shallow_null_window_midgame_nodes_skip_the_tt() {
        let board = Board::new();
        let mut context = test_context();
        context
            .runtime
            .table
            .store(board, entry(SIMPLE_SEARCH_DEPTH - 1, 7, 7));
        let spec = SearchSpec {
            depth: SIMPLE_SEARCH_DEPTH - 1,
            exact: false,
            probcut: false,
        };
        let position = Position {
            board,
            state: EvalState::from_board(board),
        };
        let score = context.node(position, -1, 0, spec).unwrap().score;
        assert_eq!(context.stats.tt_cuts, 0);
        assert!(context.stats.nodes > 1);
        // TT を使った全幅探索と同じ値になることを確かめます。
        let mut wide = test_context();
        let full = wide.node(position, -64, 64, spec).unwrap().score;
        assert_eq!(score >= 0, full >= 0);
    }
    #[test]
    fn wide_window_tt_cuts_require_matching_depth_mode_and_selectivity() {
        let mut context = test_context();
        context.runtime.table.store(Board::new(), entry(4, 7, 7));
        assert_eq!(search(&mut context, -10, 10), 7);
        assert_eq!(context.stats.nodes, 1);
        assert_eq!(context.stats.tt_cuts, 1);
        for mismatch in 0..3 {
            let mut context = test_context();
            let mut value = entry(4, 7, 7);
            match mismatch {
                0 => value.depth = 5,
                1 => value.quality = 3,
                _ => value.exact_mode = true,
            }
            context.runtime.table.store(Board::new(), value);
            assert_eq!(search(&mut context, -10, 10), 0);
            assert!(context.stats.nodes > 1);
        }
    }
    #[test]
    fn wide_window_narrowing_combines_complementary_bounds() {
        for (lower, upper) in [(-64, 0), (0, 64)] {
            let mut context = test_context();
            context
                .runtime
                .table
                .store(Board::new(), entry(4, lower, upper));
            assert_eq!(search(&mut context, -10, 10), 0);
            let value = context.runtime.table.get(Board::new()).unwrap();
            assert_eq!((value.lower, value.upper), (0, 0));
        }
    }
    #[test]
    fn wide_etc_retains_the_largest_skipped_upper_bound() {
        let mut context = test_context();
        let board = Board::new();
        let mut moves = board.get_moves();
        let mut first = true;
        while moves != 0 {
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            context.runtime.table.store(
                board.board_after_move(bit),
                entry(3, if first { 0 } else { 5 }, 64),
            );
            first = false;
        }
        assert_eq!(search(&mut context, 0, 10), 0);
        assert_eq!(context.stats.nodes, 1);
        assert_eq!(context.stats.evaluations, 0);
    }
}
