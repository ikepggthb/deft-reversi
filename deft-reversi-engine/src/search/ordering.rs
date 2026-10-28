use super::*;

impl Context {
    /// Port of the reference final-search ordering depths, windows and weights.
    pub(super) fn endgame_ordering(
        &mut self,
        parent: Position,
        child: Board,
        bit: u64,
        flips: u64,
        alpha: i32,
        beta: i32,
    ) -> Result<i32, ()> {
        let empties = parent.board.empty_count();
        let narrow = beta - alpha == 1;
        let depth = if narrow {
            if empties < 24 { empties >> 4 } else { 1 }
        } else {
            (empties >> 3).max(1)
        };
        let (low, high, value_weight, mobility_weight) = if narrow {
            (
                (-alpha - 6).max(-64),
                (-alpha + 16).min(64),
                7 + 25 * i32::from(depth),
                17,
            )
        } else {
            (
                (-beta - 8).max(-64),
                (-alpha + 12).min(64),
                269 + 94 * i32::from(depth),
                35,
            )
        };
        let state = parent.state.played(bit, flips);
        let (value, moves) = if depth <= 1 {
            if depth == 1 {
                // Preserve the depth-zero ordering_search visit and hard budget.
                self.enter()?;
            }
            self.stats.evaluations += 1;
            self.runtime.model.evaluate_state_and_moves(&child, &state)
        } else {
            let value = self.ordering_search(
                Position {
                    board: child,
                    state,
                },
                low,
                high,
                depth - 1,
            )?;
            (value, child.get_moves())
        };
        let mobility =
            -2 * moves.count_ones() as i32 - (moves & 0x8100000000000081).count_ones() as i32;
        let presence = if depth > 2 && self.runtime.table.get(child).is_some() {
            1 << 15
        } else {
            0
        };
        Ok(-value * value_weight + mobility * mobility_weight + presence)
    }

    /// Reference midgame ordering, also used by endgame preparation iterations.
    pub(super) fn midgame_ordering(
        &mut self,
        parent: Position,
        child: Board,
        bit: u64,
        flips: u64,
        alpha: i32,
        depth: u8,
    ) -> Result<i32, ()> {
        let ordering_depth = match depth {
            0..=4 => 0,
            5..=7 => 1,
            8..=11 => 2,
            _ => 3,
        };
        let state = parent.state.played(bit, flips);
        let (value, legal) = if ordering_depth <= 1 {
            if ordering_depth == 1 {
                self.enter()?;
            }
            self.stats.evaluations += 1;
            self.runtime.model.evaluate_state_and_moves(&child, &state)
        } else {
            let value = self.ordering_search(
                Position {
                    board: child,
                    state,
                },
                (-alpha - 6).max(-64),
                (-alpha + 16).min(64),
                ordering_depth - 1,
            )?;
            (value, child.get_moves())
        };
        Ok(-value
            - 2 * legal.count_ones() as i32
            - (legal & 0x8100000000000081).count_ones() as i32)
    }

    /// Small score-only search, without TT writes or probabilistic pruning.
    pub(super) fn ordering_search(
        &mut self,
        position: Position,
        mut alpha: i32,
        beta: i32,
        depth: u8,
    ) -> Result<i32, ()> {
        self.enter()?;
        let Position { board, state } = position;
        if depth == 0 {
            return Ok(self.evaluate(board, &state));
        }
        let mut moves = board.get_moves();
        if moves == 0 {
            if board.get_opponent_moves() == 0 {
                return Ok(board.final_score());
            }
            return self
                .ordering_search(
                    Position {
                        board: board.board_after_pass(),
                        state: state.passed(),
                    },
                    -beta,
                    -alpha,
                    depth,
                )
                .map(|value| -value);
        }
        let mut best = -65;
        while moves != 0 {
            let bit = moves & moves.wrapping_neg();
            moves &= moves - 1;
            let flips = board.flip_mask(bit);
            let child = Position {
                board: board.after_known_move(bit, flips),
                state: state.played(bit, flips),
            };
            let score = -self.ordering_search(child, -beta, -alpha, depth - 1)?;
            best = best.max(score);
            if score >= beta {
                break;
            }
            alpha = alpha.max(score);
        }
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::super::session::test_context;
    use super::*;

    #[test]
    fn static_ordering_preserves_visits_and_hard_budget() {
        let board = Board::new();
        let bit = board.get_moves() & board.get_moves().wrapping_neg();
        let flips = board.flip_mask(bit);
        let child = board.after_known_move(bit, flips);
        let parent = Position {
            board,
            state: EvalState::from_board(board),
        };
        for (depth, visits) in [(4, 0), (7, 1)] {
            let mut context = test_context();
            context
                .midgame_ordering(parent, child, bit, flips, 0, depth)
                .unwrap();
            assert_eq!(context.stats.nodes, visits);
            assert_eq!(context.stats.evaluations, 1);

            let mut limited = test_context();
            Arc::get_mut(&mut limited.runtime)
                .unwrap()
                .control
                .node_limit = Some(0);
            let result = limited.midgame_ordering(parent, child, bit, flips, 0, depth);
            assert_eq!(result.is_err(), visits != 0);
            assert_eq!(limited.stats.nodes, 0);
            assert_eq!(limited.stats.evaluations, u64::from(visits == 0));
        }
    }
}
