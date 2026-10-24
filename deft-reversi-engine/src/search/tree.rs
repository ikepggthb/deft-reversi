use super::*;
const SPLIT_DEPTH: u8 = 8;
const SPLIT_EMPTIES: u8 = 10;
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
        cuts: Vec<Arc<AtomicBool>>,
    ) -> Self {
        Self {
            runtime,
            stats: SearchStats::default(),
            ancestors,
            cuts,
            end_cache: Vec::new(),
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
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
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
        if exact && beta - alpha == 1 && board.empty_count() <= super::tail::LOCAL_END_MAX {
            return self.tail_score_entered(board, alpha, beta).map(Line::leaf);
        }
        if exact && board.empty_count() <= super::tail::TAIL_EMPTIES {
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
        let odd_regions = if exact { odd_regions(board) } else { 0 };
        while moves != 0 {
            let bit = moves & moves.wrapping_neg();
            moves &= moves - 1;
            let square = Square::new(bit.trailing_zeros() as u8).unwrap();
            let flips = board.flip_mask(bit);
            let child = board.after_known_move(bit, flips);
            // Reference ETC uses the original alpha to skip moves. Preserve
            // their largest upper bound so a fail-low remains a sound bound.
            if (if exact {
                board.empty_count() > 12
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
            let mut ordering = if !exact {
                self.midgame_ordering(position, child, bit, flips, alpha, depth)?
            } else if board.empty_count() >= super::tail::LOCAL_END_MAX {
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
            if square.index() == tt_move {
                ordering += tt_priority;
            } else if cached.is_some_and(|entry| entry.second == square.index()) {
                ordering += tt_priority / 2;
            }
            candidates[count] = Candidate {
                square,
                flips,
                ordering,
            };
            count += 1;
        }
        if count == 0 {
            self.stats.tt_cuts += 1;
            return Ok(Line::leaf(skipped_upper));
        }
        pick_best(&mut candidates[..count], 0);
        let first = candidates[0];
        let first_position = first.child(
            position,
            !exact
                || board.empty_count() > super::tail::LOCAL_END_MAX
                || self.runtime.sigma().is_some(),
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
        let mut best = Line::prepend(first.action(), &child);
        alpha = alpha.max(best.score);
        if alpha < beta && count > 1 {
            let can_split = self.runtime.parallel
                && allow_probcut
                && if exact {
                    board.empty_count() >= SPLIT_EMPTIES
                } else {
                    depth >= SPLIT_DEPTH
                };
            if can_split && count >= 3 && self.runtime.pool.can_offer(&self.ancestors) {
                best = self.split(
                    position,
                    candidates[1..count].to_vec(),
                    (alpha, beta),
                    SearchSpec {
                        depth: depth - 1,
                        exact,
                        probcut: allow_probcut,
                    },
                    best,
                    None,
                )?;
            } else {
                for index in 1..count {
                    pick_best(&mut candidates[..count], index);
                    if let Some(line) = self.sibling(
                        position,
                        candidates[index],
                        alpha,
                        beta,
                        SearchSpec {
                            depth: depth - 1,
                            exact,
                            probcut: allow_probcut,
                        },
                    )? {
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
        let (lower, upper) = if best.score <= original_alpha {
            (-64, best.score as i8)
        } else if best.score >= original_beta {
            (best.score as i8, 64)
        } else {
            (best.score as i8, best.score as i8)
        };
        let best_move = match best.actions[0] {
            Some(Action::Place(s)) => s.index(),
            _ => 64,
        };
        let entry = Entry {
            lower,
            upper,
            depth,
            quality,
            best: best_move,
            second: 64,
            generation: self.runtime.generation,
            exact_mode: exact,
        };
        self.runtime.table.store(board, entry);
        if exact && original_beta - original_alpha > 1 {
            self.runtime.pv_table.store(board, entry);
        }
        Ok(best)
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
        let next = candidate.child(
            position,
            !exact
                || position.board.empty_count() > super::tail::LOCAL_END_MAX
                || self.runtime.sigma().is_some(),
        );
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
pub(super) fn odd_regions(board: Board) -> u64 {
    let empty = !(board.player_bits() | board.opponent_bits());
    [
        0x000000000f0f0f0f,
        0x00000000f0f0f0f0,
        0x0f0f0f0f00000000,
        0xf0f0f0f000000000,
    ]
    .into_iter()
    .filter(|region| (empty & region).count_ones() % 2 == 1)
    .fold(0, |bits, region| bits | region)
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
            let bit = moves & moves.wrapping_neg();
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

pub(super) fn endgame_ordering(legal: u64, bit: u64, odd: u64) -> i32 {
    let mobility = legal.count_ones() + (legal & 0x8100000000000081).count_ones();
    -(mobility as i32) * 18 + if bit & odd != 0 { 17 } else { 0 }
}

#[cfg(test)]
mod tt_tests {
    use super::super::session::test_context;
    use super::*;
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
            let bit = moves & moves.wrapping_neg();
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
