//! TT cutoffs need no PV copies. Verify the chosen line after the score is known.
use super::*;

pub(super) fn terminal_score(board: Board, pv: &[Action]) -> Option<i32> {
    let board = pv
        .iter()
        .try_fold(board, |board, &action| board.play(action))
        .ok()?;
    board.is_terminal().then(|| {
        let score = board.final_score();
        if pv.len().is_multiple_of(2) {
            score
        } else {
            -score
        }
    })
}

impl Context {
    pub(super) fn extract_pv(
        &mut self,
        mut board: Board,
        mut score: i32,
        mut depth: u8,
        exact: bool,
        mut first: Option<Action>,
    ) -> Result<Vec<Action>, Aborted> {
        let selective = self.runtime.selective();
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
            let first = first.take().and_then(|action| match action {
                Action::Place(square) => Some(square),
                _ => None,
            });
            let hint = first.or_else(|| {
                self.runtime
                    .pv_table
                    .get(board)
                    .or_else(|| self.runtime.table.get(board))
                    .and_then(|entry| Square::new(entry.best))
            });
            let preferred = hint.map_or(0, Square::bit) & moves;
            // In a selective search, do not change the reported root move
            // based on a new probe.
            if selective && first.is_some() {
                moves = preferred;
            }
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
                    -score - i32::from(selective),
                    -score + 1,
                    SearchSpec {
                        depth: depth - 1,
                        exact,
                        probcut: true,
                    },
                )?;
                // At full width the known parent value makes a one-sided
                // test sufficient. With MPC, require the probe's point score.
                let matches = if selective {
                    line.score == -score
                } else {
                    line.score <= -score
                };
                if matches {
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
