//! 置換表カットオフで最善手列（PV：互いに最善手を指し続けた場合の予想手順）が省略された後に、
//! 確定したスコアと一致する手順を置換表や再探索から復元・検証するモジュールです。
use super::*;

/// 手順 `pv` を最後まで辿って終局する場合、初期局面視点での終局石差を返します。
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
    /// 確定した評価値 `score` に一致する PV（Principal Variation）をルートから最大 128 手まで復元します。
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
            // 選択探索（ProbCut）時は、新たなプローブ結果によって報告済みのルート最善手が書き換わらないよう固定します。
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
                // 全幅探索では親スコアが厳密に決まっているため片側テスト（`<= -score`）で十分です。
                // MPC 有効時は確率的変動があるため点スコア一致（`== -score`）を要求します。
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
