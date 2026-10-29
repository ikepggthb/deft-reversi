use super::endgame::parity::odd_regions;
use super::*;
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
    pub(super) fn enter(&mut self) -> Result<(), ()> {
        // Poll cancellation/deadline every 256 nodes; the optional hard node
        // budget below is still reserved for every individual visited node.
        if self.stats.nodes & 255 == 0 && self.aborted() {
            return Err(());
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
            self.runtime.control.set(3);
            return Err(());
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
    ) -> Result<Line, ()> {
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
        let original_alpha = alpha;
        let original_beta = beta;
        let mut moves = board.get_moves();
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
                SearchSpec {
                    depth,
                    exact,
                    probcut: allow_probcut,
                },
            )?;
            return Ok(Line::prepend(Action::Pass, &child));
        }
        if depth == 0 {
            self.stats.leaves += 1;
            return Ok(Line::leaf(self.evaluate(board, &state)));
        }
        if !exact && depth == 1 {
            return self.one_ply(Position { board, state }, alpha, beta, allow_probcut);
        }
        let quality = if allow_probcut && self.runtime.sigma().is_some() {
            self.runtime.quality.load(Ordering::Relaxed)
        } else {
            0
        };
        let main = self.runtime.table.get(board);
        let pv = if exact && original_beta - original_alpha > 1 {
            self.runtime.pv_table.get(board)
        } else {
            None
        };
        let matches = |entry: Entry| {
            entry.depth == depth && entry.quality == quality && entry.exact_mode == exact
        };
        let cached = match (pv, main) {
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
        };
        let mut known_lower = -64;
        if let Some(entry) = cached {
            self.stats.tt_hits += 1;
            if entry.depth == depth && entry.quality == quality && entry.exact_mode == exact {
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
        let tt_move = cached.map(|e| e.best).unwrap_or(64);
        // A move list has at most 64 entries and never allocates per node.
        let position = Position { board, state };
        let placeholder = Candidate {
            square: Square::new(0).unwrap(),
            flips: 0,
            ordering: 0,
        };
        let mut candidates = [placeholder; 64];
        let mut count = 0;
        let mut skipped_upper = -65;
        while moves != 0 {
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            let square = Square::new(bit.trailing_zeros() as u8).unwrap();
            let flips = board.flip_mask(bit);
            let child = board.after_known_move(bit, flips);
            // Reference ETC uses the original alpha to skip moves. Preserve
            // their largest upper bound so a fail-low remains a sound bound.
            // Null-window children with thirteen or fewer empties live only in
            // the local endgame cache, so the main table cannot cut them.
            if (if exact {
                board.empty_count() > 12 + 2 * u8::from(beta - alpha == 1)
            } else {
                depth >= 4
            }) && let Some(entry) = self.runtime.table.get(child)
                && entry.exact_mode == exact
                && entry.depth == depth - 1
                && entry.quality == quality
            {
                let lower = -i32::from(entry.upper);
                let upper = -i32::from(entry.lower);
                if lower >= beta {
                    self.stats.tt_cuts += 1;
                    return Ok(Line::leaf(lower));
                }
                if upper <= original_alpha {
                    skipped_upper = skipped_upper.max(upper);
                    continue;
                }
                alpha = alpha.max(lower);
                known_lower = known_lower.max(lower);
            }
            candidates[count] = Candidate {
                square,
                flips,
                ordering: 0,
            };
            count += 1;
        }
        if count == 0 {
            self.stats.tt_cuts += 1;
            return Ok(Line::leaf(skipped_upper));
        }
        // Complete cheap child-TT cutoffs before spending search effort on
        // the TT move. Remaining moves are still unevaluated here.
        let preferred = if let Some(index) =
            (0..count).find(|&i| candidates[i].square.index() == tt_move)
        {
            let candidate = candidates[index];
            candidates.copy_within(index + 1..count, index);
            count -= 1;
            let child = self.node(
                candidate.child(
                    Position { board, state },
                    self.child_needs_eval(board, exact, beta - alpha == 1),
                ),
                -beta,
                -alpha,
                SearchSpec {
                    depth: depth - 1,
                    ..spec
                },
            )?;
            let mut line = Line::prepend(candidate.action(), &child);
            alpha = alpha.max(line.score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                if self.aborted() {
                    return Err(());
                }
                line.score = line.score.max(known_lower);
                self.store_node_line(board, &line, (original_alpha, original_beta), spec, quality);
                return Ok(line);
            }
            Some(line)
        } else {
            None
        };
        let only_move = count <= 1;
        let odd_regions = if exact { odd_regions(board) } else { 0 };
        for candidate in &mut candidates[..count] {
            let square = candidate.square;
            let bit = square.bit();
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
                if bit & 0x8100000000000081 != 0 {
                    value += ORDERING.corner;
                }
                value
            };
            let tt_priority = if exact { ORDERING.tt } else { 1 << 8 };
            if cached.is_some_and(|entry| entry.second == square.index()) {
                ordering += tt_priority / 2;
            }
            candidate.ordering = ordering;
        }
        let start = usize::from(preferred.is_none());
        let mut best = if let Some(line) = preferred {
            line
        } else {
            pick_best(&mut candidates[..count], 0);
            let first = candidates[0];
            let first_position = first.child(
                position,
                self.child_needs_eval(board, exact, beta - alpha == 1),
            );
            let child = self.node(
                Position {
                    board: first_position.board,
                    state: first_position.state,
                },
                -beta,
                -alpha,
                SearchSpec {
                    depth: depth - 1,
                    exact,
                    probcut: allow_probcut,
                },
            )?;
            Line::prepend(first.action(), &child)
        };
        alpha = alpha.max(best.score);
        if alpha < beta && count > start {
            let can_split = self.runtime.parallel
                && allow_probcut
                && if exact {
                    board.empty_count() >= SPLIT_EMPTIES
                } else {
                    depth >= SPLIT_DEPTH
                };
            let child_spec = SearchSpec {
                depth: depth - 1,
                exact,
                probcut: allow_probcut,
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
                    best = self.split(
                        position,
                        candidates[index..count].to_vec(),
                        (alpha, beta),
                        child_spec,
                        best,
                        None,
                    )?;
                    break;
                }
                pick_best(&mut candidates[..count], index);
                index += 1;
                if let Some(line) =
                    self.sibling(position, candidates[index - 1], alpha, beta, child_spec)?
                {
                    if line.score > best.score {
                        best = line;
                    }
                    alpha = alpha.max(best.score);
                    if alpha >= beta {
                        self.stats.beta_cuts += 1;
                        break;
                    }
                }
            }
        } else if alpha >= beta {
            self.stats.beta_cuts += 1;
        }
        if self.aborted() {
            return Err(());
        }
        if skipped_upper > best.score {
            best = Line::leaf(skipped_upper);
        }
        best.score = best.score.max(known_lower);
        self.store_node_line(board, &best, (original_alpha, original_beta), spec, quality);
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
    pub(super) fn sibling(
        &mut self,
        position: Position,
        candidate: Candidate,
        alpha: i32,
        beta: i32,
        spec: SearchSpec,
    ) -> Result<Option<Line>, ()> {
        let SearchSpec {
            depth,
            exact,
            probcut: allow_probcut,
        } = spec;
        let update_eval = self.child_needs_eval(position.board, exact, true);
        let next = candidate.child(position, update_eval);
        let child = self.node(
            Position {
                board: next.board,
                state: next.state,
            },
            -alpha - 1,
            -alpha,
            SearchSpec {
                depth,
                exact,
                probcut: allow_probcut,
            },
        )?;
        let mut line = Line::prepend(candidate.action(), &child);
        if line.score > alpha && line.score < beta {
            let next = if !update_eval && self.child_needs_eval(position.board, exact, false) {
                candidate.child(position, true)
            } else {
                next
            };
            let child = self.node(
                Position {
                    board: next.board,
                    state: next.state,
                },
                -beta,
                -alpha,
                SearchSpec {
                    depth,
                    exact,
                    probcut: allow_probcut,
                },
            )?;
            line = Line::prepend(candidate.action(), &child);
        }
        Ok(Some(line))
    }
    pub(super) fn probcut(
        &mut self,
        board: Board,
        state: EvalState,
        depth: u8,
        alpha: i32,
        beta: i32,
        exact: bool,
    ) -> Result<Option<i32>, ()> {
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
    ) -> Result<Line, ()> {
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
                best = Line::prepend(
                    Action::Place(Square::new(bit.trailing_zeros() as u8).unwrap()),
                    &child,
                );
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
            let mut board = Board::new();
            let mut random = seed;
            while board.empty_count() > 14 {
                let mut moves = board.get_moves();
                if moves == 0 {
                    assert!(!board.is_terminal());
                    board = board.board_after_pass();
                    continue;
                }
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                for _ in 0..random % u64::from(moves.count_ones()) {
                    moves &= moves - 1;
                }
                board = board.board_after_move(moves.isolate_lowest_one());
            }
            let moves = board.get_moves();
            assert_ne!(moves, 0);
            let bit = moves.isolate_lowest_one();
            let candidate = Candidate {
                square: Square::new(bit.trailing_zeros() as u8).unwrap(),
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
                .unwrap()
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
