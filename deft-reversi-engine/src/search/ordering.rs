//! 中盤および終盤探索ノード向けのムーブオーダリング（有望そうな手から先に探索するための並び替えスコア算出）モジュールです。
//! Alpha-Beta 法では、良い手から先に調べるほど枝刈り（カット）が増えて探索が速くなります。
//!
//! ## 出典
//!
//! 着手順序の評価に使う探索の深さ・窓・重みは、[Egaroucid](https://github.com/Nyanyan/Egaroucid)（山名琢翔、GPL-3.0-or-later）の `move_ordering.hpp` の値を使っています。
use super::*;
use crate::board::CORNERS;

impl Context {
    /// 終盤完全読み・終盤選択探索向けのムーブオーダリングスコアを計算します。
    /// 浅い評価値・相手の合法手数（および隅への着手）・置換表への登録有無を重み付きで合算して返します。
    pub(super) fn endgame_ordering(
        &mut self,
        parent: Position,
        child: Board,
        bit: u64,
        flips: u64,
        alpha: i32,
        beta: i32,
    ) -> Result<i32, Aborted> {
        let empties = parent.board.empty_count();
        let narrow = beta - alpha == 1;
        let depth = if narrow {
            // 空き 20 マス以上では Null-Window（幅 1 の窓）探索時に各子局面を 1 手浅く先読みして順序の精度を上げます（Egaroucid 方式）。
            if empties >= 20 { 2 } else { empties >> 4 }
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
                // depth == 0 の `ordering_search` と同様のノード訪問カウントとノード上限チェックを保ちます。
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
        let mobility = -2 * moves.count_ones() as i32 - (moves & CORNERS).count_ones() as i32;
        let presence = if depth > 2 && self.runtime.table.get(child).is_some() {
            1 << 15
        } else {
            0
        };
        Ok(-value * value_weight + mobility * mobility_weight + presence)
    }

    /// 中盤探索および終盤準備イテレーション向けのムーブオーダリングスコアを計算します。
    pub(super) fn midgame_ordering(
        &mut self,
        parent: Position,
        child: Board,
        bit: u64,
        flips: u64,
        alpha: i32,
        depth: u8,
    ) -> Result<i32, Aborted> {
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
        Ok(-value - 2 * legal.count_ones() as i32 - (legal & CORNERS).count_ones() as i32)
    }

    /// オーダリング専用の軽量な Alpha-Beta 探索です（置換表への書き込みや ProbCut 枝刈りを行いません）。
    pub(super) fn ordering_search(
        &mut self,
        position: Position,
        mut alpha: i32,
        beta: i32,
        depth: u8,
    ) -> Result<i32, Aborted> {
        self.enter()?;
        let Position { board, state } = position;
        if depth == 0 {
            // `node` と異なり終局チェックをまだ行っていないため、ここで両者パス（終局）を確認します。
            let (value, moves) = self.evaluate_and_moves(board, &state);
            if moves == 0 && board.get_opponent_moves() == 0 {
                return Ok(board.final_score());
            }
            return Ok(value);
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
            let bit = moves.isolate_lowest_one();
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
