//! Dedicated five- and six-empty null-window kernels.
use super::parity::SQUARE_PARITY;
use super::{Board, Context};

impl Context {
    pub(super) fn six_moves_score(
        &mut self,
        board: Board,
        alpha: i32,
        mut moves: u64,
        parity: u8,
    ) -> Result<i32, ()> {
        debug_assert_eq!(board.empty_count(), 6);
        let child_alpha = -alpha - 1;
        if moves == 0 {
            let legal = board.get_opponent_moves();
            if legal == 0 {
                self.stats.leaves += 1;
                return Ok(board.final_score());
            }
            self.enter()?;
            return self
                .endgame_score_entered(
                    board.board_after_pass(),
                    child_alpha,
                    -alpha,
                    Some(legal),
                    None,
                )
                .map(|score| -score);
        }
        if let Some(score) = crate::board::stability::null_window_upper(board, alpha) {
            self.stats.stability_cuts += 1;
            return Ok(score);
        }
        let mut preferred = moves & isolated_empties(board);
        let mut best = -65;
        while moves != 0 {
            let options = if preferred != 0 { preferred } else { moves };
            let bit = options.isolate_lowest_one();
            preferred &= !bit;
            moves &= !bit;
            let child = board.after_known_move(bit, board.flip_mask(bit));
            let child_parity = parity ^ SQUARE_PARITY[bit.trailing_zeros() as usize];
            self.enter()?;
            let score = -self.five_empty_score(child, child_alpha, child_parity)?;
            best = best.max(score);
            if score > alpha {
                self.stats.beta_cuts += 1;
                break;
            }
        }
        Ok(best)
    }
    pub(super) fn five_empty_score(
        &mut self,
        board: Board,
        alpha: i32,
        parity: u8,
    ) -> Result<i32, ()> {
        debug_assert_eq!(board.empty_count(), 5);
        if let Some(score) = crate::board::stability::null_window_upper(board, alpha) {
            self.stats.stability_cuts += 1;
            return Ok(score);
        }
        let mut empty = !(board.player_bits() | board.opponent_bits());
        let mut preferred = isolated_empties(board);
        let child_alpha = -alpha - 1;
        let mut best = -65;
        while empty != 0 {
            let options = if preferred != 0 { preferred } else { empty };
            let bit = options.isolate_lowest_one();
            preferred &= !bit;
            empty &= !bit;
            let square = bit.trailing_zeros() as usize;
            if board.opponent_bits() & super::last_four::NEIGHBOUR[square] == 0 {
                continue;
            }
            let flips = board.flip_mask(bit);
            if flips == 0 {
                continue;
            }
            let child = board.after_known_move(bit, flips);
            let child_parity = parity ^ SQUARE_PARITY[square];
            self.enter()?;
            let score = -self.last_four_score_with_parity(child, child_alpha, child_parity)?;
            best = best.max(score);
            if score > alpha {
                self.stats.beta_cuts += 1;
                return Ok(best);
            }
        }
        // Kernel scores/bounds are in -64..=64; -65 means no legal move.
        if best != -65 {
            return Ok(best);
        }
        if board.get_opponent_moves() == 0 {
            self.stats.leaves += 1;
            return Ok(board.final_score());
        }
        self.enter()?;
        self.five_empty_score(board.board_after_pass(), child_alpha, parity)
            .map(|score| -score)
    }
}

/// Empty squares without an empty king-move neighbour. Searching these first
/// cut more five/six-empty nodes than odd-quadrant-first ordering; the parity
/// is still carried for the four-empty kernel.
#[inline(always)]
fn isolated_empties(board: Board) -> u64 {
    let empty = !(board.player_bits() | board.opponent_bits());
    let horizontal = ((empty >> 1) & 0x7f7f7f7f7f7f7f7f) | ((empty << 1) & 0xfefefefefefefefe);
    let row = horizontal | empty;
    let around = horizontal | (row >> 8) | (row << 8);
    empty & !around
}
