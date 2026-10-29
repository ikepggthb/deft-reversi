//! TT cutoffs need no PV copies. Verify the chosen line after the score is known.
use super::*;
impl Context {
    pub(super) fn extract_pv(
        &mut self,
        mut board: Board,
        mut score: i32,
        mut depth: u8,
        exact: bool,
        mut first: Option<Action>,
    ) -> Result<Vec<Action>, Aborted> {
        let mut pv = Vec::new();
        while depth > 0 && !board.is_terminal() && pv.len() < 128 {
            if self.aborted() {
                return Err(Aborted);
            }
            let mut moves = board.get_moves();
            if moves == 0 {
                pv.push(Action::Pass);
                board = board.board_after_pass();
                score = -score;
                first = None;
                continue;
            }
            let hint = first
                .take()
                .and_then(|action| match action {
                    Action::Place(square) => Some(square),
                    _ => None,
                })
                .or_else(|| {
                    self.runtime
                        .pv_table
                        .get(board)
                        .or_else(|| self.runtime.table.get(board))
                        .and_then(|entry| Square::new(entry.best))
                });
            let preferred = hint.map_or(0, Square::bit) & moves;
            let state = EvalState::from_board(board);
            let mut chosen = None;
            while moves != 0 {
                let bit = if preferred & moves != 0 {
                    preferred
                } else {
                    moves.isolate_lowest_one()
                };
                moves &= !bit;
                let flips = board.flip_mask(bit);
                let child = board.after_known_move(bit, flips);
                let line = self.node(
                    Position {
                        board: child,
                        state: state.played(bit, flips),
                    },
                    -score,
                    -score + 1,
                    SearchSpec {
                        depth: depth - 1,
                        exact,
                        probcut: true,
                    },
                )?;
                // Since the parent value is known, a move reaching that value
                // is optimal. A hint alone is not proof that the move is best.
                if line.score <= -score {
                    chosen = Some((bit, child));
                    break;
                }
            }
            let Some((bit, child)) = chosen else {
                break;
            };
            pv.push(Action::Place(Square::lowest(bit)));
            board = child;
            score = -score;
            depth -= 1;
        }
        Ok(pv)
    }
}
