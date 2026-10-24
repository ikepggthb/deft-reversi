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
        let value = if depth == 0 {
            self.evaluate(child, &state)
        } else {
            self.ordering_search(
                Position {
                    board: child,
                    state,
                },
                low,
                high,
                depth - 1,
            )?
        };
        let moves = child.get_moves();
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
        let value = if ordering_depth == 0 {
            self.evaluate(child, &state)
        } else {
            self.ordering_search(
                Position {
                    board: child,
                    state,
                },
                (-alpha - 6).max(-64),
                (-alpha + 16).min(64),
                ordering_depth - 1,
            )?
        };
        let legal = child.get_moves();
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
