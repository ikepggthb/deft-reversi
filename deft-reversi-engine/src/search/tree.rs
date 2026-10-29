use super::endgame::parity::odd_regions;
use super::*;
use crate::board::CORNERS;
use std::ops::ControlFlow;
// Midgame nodes split down to four plies of remaining depth: the shallow
// iterations before an endgame solve otherwise leave most threads idle.
pub(super) const SPLIT_DEPTH: u8 = 4;
pub(super) const SPLIT_EMPTIES: u8 = 10;
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
    pub(super) fn aborted(&self) -> bool {
        self.runtime.control.check() || self.cuts.iter().any(|cut| cut.load(Ordering::Acquire))
    }
    pub(super) fn enter(&mut self) -> Result<(), Aborted> {
        // Poll cancellation/deadline every 256 nodes; the optional hard node
        // budget below is still reserved for every individual visited node.
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
    pub(super) fn evaluate(&mut self, board: Board, state: &EvalState) -> i32 {
        self.stats.evaluations += 1;
        self.runtime.model.evaluate_state(&board, state)
    }
    fn child_needs_eval(&self, board: Board, exact: bool, narrow: bool) -> bool {
        if !exact {
            return true;
        }
        let empties = board.empty_count();
        if empties > super::endgame::LOCAL_END_MAX + 1 {
            return true;
        }
        let selective = self.runtime.sigma().is_some();
        if empties == super::endgame::LOCAL_END_MAX + 1 {
            // A wide thirteen-empty child still orders moves using its state.
            // The experimental narrow entry seeds its pattern cache from it too.
            return !narrow || selective || cfg!(feature = "endgame-pattern-ordering");
        }
        selective
    }
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
        // The caller's window decides the bound type stored for the result.
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
        let quality = if allow_probcut && self.runtime.sigma().is_some() {
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
        // A move list has at most 64 entries and never allocates per node.
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
        // Complete cheap child-TT cutoffs before spending search effort on
        // the TT move. Remaining moves are still unevaluated here.
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
    /// The TT entry for `board`. A wide exact window also reads the PV table
    /// and prefers the narrower of two entries that both fit the search.
    // This and the next three helpers are parts of `node`. Kept inline:
    // as calls they cost 0.6% at one thread.
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
    /// Enhanced transposition cutoff: looks up every child in the main table,
    /// fills `candidates` with the moves that still need a search, and
    /// returns their count and the largest upper bound of the skipped ones.
    /// Breaks with the score when a child alone fails high.
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
            // Null-window children with thirteen or fewer empties live only
            // in the local endgame cache, so the main table cannot cut them.
            // The window narrows during the pass, so this is checked per move.
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
                // Reference ETC uses the original alpha to skip moves. Keep
                // their largest upper bound so a fail-low remains sound.
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
    /// Sets the ordering key of every candidate; `second` is the TT's
    /// second-best move.
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
    /// Searches `candidates[start..]` after the first move returned `best`,
    /// splitting when idle threads can take the remaining moves.
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
                position.board.empty_count() >= SPLIT_EMPTIES
            } else {
                depth >= SPLIT_DEPTH
            };
        let mut index = start;
        while index < count {
            // Like the reference node_split, retry after every sibling:
            // a thread that became idle meanwhile can still take the
            // remaining moves instead of waiting for another split point.
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
    /// Searches one later move with a null window and, if it lands inside
    /// the window, again with the full window.
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
    pub(super) fn probcut(
        &mut self,
        board: Board,
        state: EvalState,
        depth: u8,
        alpha: i32,
        beta: i32,
        exact: bool,
    ) -> Result<Option<i32>, Aborted> {
        let Some(sigma) = self.runtime.sigma() else {
            return Ok(None);
        };
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
        let high =
            ((f64::from(beta) - rule.intercept + sigma * rule.deviation) / rule.slope).ceil();
        if high < 64.0 {
            let bound = high.max(-64.0) as i32;
            let score = self.ordering_search(
                Position { board, state },
                bound - 1,
                bound,
                rule.shallow_depth,
            )?;
            if score >= bound {
                self.stats.probcut_cuts += 1;
                return Ok(Some(beta));
            }
        }
        let low =
            ((f64::from(alpha) - rule.intercept - sigma * rule.deviation) / rule.slope).floor();
        if low > -64.0 {
            let bound = low.min(64.0) as i32;
            let score = self.ordering_search(
                Position { board, state },
                bound,
                bound + 1,
                rule.shallow_depth,
            )?;
            if score <= bound {
                self.stats.probcut_cuts += 1;
                return Ok(Some(alpha));
            }
        }
        Ok(None)
    }
}
/// Whether a TT entry was stored by a search of the same depth, mode and
/// selectivity, so that its bounds apply.
fn fits(entry: Entry, spec: SearchSpec, quality: u8) -> bool {
    entry.depth == spec.depth && entry.quality == quality && entry.exact_mode == spec.exact
}
/// The bounds the child-TT pass reads and narrows.
#[derive(Clone, Copy)]
struct ChildBounds {
    original_alpha: i32,
    alpha: i32,
    beta: i32,
    known_lower: i32,
}
pub(super) fn pick_best(candidates: &mut [Candidate], index: usize) {
    let best = (index..candidates.len())
        .max_by_key(|&i| candidates[i].ordering)
        .unwrap();
    candidates.swap(index, best);
}

impl Context {
    // At depth one, evaluating children for ordering evaluates every leaf twice.
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
            // Find an interior score so sibling must perform its wide re-search.
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
        let line = context
            .node(
                Position {
                    board,
                    state: EvalState::from_board(board),
                },
                -1,
                0,
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
        context
            .runtime
            .table
            .store(board.board_after_move(bit), entry(3, -7, -7));
        assert_eq!(search(&mut context, -1, 0), 7);
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
