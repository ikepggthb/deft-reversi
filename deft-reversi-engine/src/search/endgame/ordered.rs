//! Ordered null-window endgame search for seven to thirteen empties.
use super::local_tt::EndProbe;
use super::parity::odd_regions;
use super::{Board, Context, EvalState, LOCAL_END_MAX, Ordering};

impl Context {
    pub(super) fn ordered_endgame(
        &mut self,
        board: Board,
        mut alpha: i32,
        beta: i32,
        known_moves: Option<u64>,
        best_move: &mut u8,
    ) -> Result<i32, ()> {
        let mut moves = known_moves.unwrap_or_else(|| board.get_moves());
        if moves == 0 {
            let passed = board.board_after_pass();
            let legal = passed.get_moves();
            if legal == 0 {
                self.stats.leaves += 1;
                return Ok(board.final_score());
            }
            self.enter()?;
            return self
                .endgame_score_entered(passed, -beta, -alpha, Some(legal), None)
                .map(|score| -score);
        }
        if let Some(score) = crate::board::stability::null_window_upper(board, alpha) {
            self.stats.stability_cuts += 1;
            return Ok(score);
        }
        if board.empty_count() >= 12
            && self.runtime.sigma().is_some()
            && let Some(score) = self.probcut(
                board,
                EvalState::from_board(board),
                board.empty_count(),
                alpha,
                beta,
                true,
            )?
        {
            return Ok(score);
        }
        if moves.is_power_of_two() {
            *best_move = moves.trailing_zeros() as u8;
            let child = board.after_known_move(moves, board.flip_mask(moves));
            self.enter()?;
            return self
                .endgame_score_entered(child, -beta, -alpha, None, None)
                .map(|score| -score);
        }
        let tt_move = *best_move;
        let mut best = -65;
        if tt_move < 64 && moves & (1u64 << tt_move) != 0 {
            let bit = 1u64 << tt_move;
            moves &= !bit;
            let child = board.after_known_move(bit, board.flip_mask(bit));
            self.enter()?;
            let score = -self.endgame_score_entered(child, -beta, -alpha, None, None)?;
            best = score;
            if score >= beta {
                self.stats.beta_cuts += 1;
                return Ok(score);
            }
        }
        let mut candidates = [EndMove {
            square: 0,
            flips: 0,
            legal: 0,
            ordering: 0,
            probe: EndProbe::EMPTY,
        }; LOCAL_END_MAX as usize];
        let odd = odd_regions(board);
        let mut count = 0;
        let quality = self.runtime.quality.load(Ordering::Relaxed);
        #[cfg(feature = "endgame-pattern-ordering")]
        let mut ordering_state = None;
        while moves != 0 {
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            let flips = board.flip_mask(bit);
            if flips == board.opponent_bits() {
                *best_move = bit.trailing_zeros() as u8;
                self.stats.leaves += 1;
                return Ok(64);
            }
            let child = board.after_known_move(bit, flips);
            let probe = self.end_cache.probe(child, quality);
            if probe.matched {
                self.stats.tt_hits += 1;
                let lower = -i32::from(probe.upper);
                let upper = -i32::from(probe.lower);
                if lower >= beta {
                    *best_move = bit.trailing_zeros() as u8;
                    self.stats.tt_cuts += 1;
                    return Ok(lower);
                }
                if upper <= alpha {
                    self.stats.tt_cuts += 1;
                    best = best.max(upper);
                    continue;
                }
            }
            let legal = child.get_moves();
            let mobility = legal.count_ones() + (legal & 0x8100000000000081).count_ones();
            if mobility <= 1 {
                self.enter()?;
                let score =
                    -self.endgame_score_entered(child, -beta, -alpha, Some(legal), Some(probe))?;
                if score > best {
                    best = score;
                    *best_move = bit.trailing_zeros() as u8;
                }
                if score >= beta {
                    return Ok(score);
                }
                continue;
            }
            let ordering = endgame_ordering(child, legal, bit, odd);
            #[cfg(feature = "endgame-pattern-ordering")]
            let ordering = if board.empty_count() >= 11 {
                let state = ordering_state.get_or_insert_with(|| self.end_ordering_state(board));
                self.stats.evaluations += 1;
                let value = self
                    .runtime
                    .model
                    .end_ordering_child_value(child, state, bit, flips);
                -(mobility as i32) * 40 - value * 12
            } else {
                ordering
            };
            candidates[count] = EndMove {
                square: bit.trailing_zeros() as u8,
                flips,
                legal,
                ordering,
                probe,
            };
            count += 1;
        }
        for i in 0..count {
            let index = (i..count).max_by_key(|&n| candidates[n].ordering).unwrap();
            candidates.swap(i, index);
            let action = candidates[i];
            let child = board.after_known_move(1u64 << action.square, action.flips);
            #[cfg(feature = "endgame-pattern-ordering")]
            if child.empty_count() >= 11
                && let Some(state) = ordering_state
            {
                self.end_ordering[usize::from(child.empty_count() - 11)] =
                    Some((child, state.played(1u64 << action.square, action.flips)));
            }
            self.enter()?;
            let score = -self.endgame_score_entered(
                child,
                -beta,
                -alpha,
                Some(action.legal),
                Some(action.probe),
            )?;
            if score > best {
                best = score;
                *best_move = action.square;
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                break;
            }
        }
        Ok(best)
    }
    #[cfg(feature = "endgame-pattern-ordering")]
    fn end_ordering_state(&mut self, board: Board) -> EvalState {
        let slot = &mut self.end_ordering[usize::from(board.empty_count() - 11)];
        if let Some((cached, state)) = *slot
            && cached == board
        {
            return state;
        }
        let state = EvalState::from_board(board);
        *slot = Some((board, state));
        state
    }
}

/// Reference fast endgame ordering: the opponent's corner-weighted mobility
/// dominates, then the mover's stable corner discs; square value and quadrant
/// parity only break ties. (The reference's potential-mobility term was not
/// worth its cost here.)
pub(in crate::search) fn endgame_ordering(child: Board, legal: u64, bit: u64, odd: u64) -> i32 {
    const SQUARE_VALUE: [i32; 64] = [
        18, 4, 16, 12, 12, 16, 4, 18, 4, 2, 6, 8, 8, 6, 2, 4, 16, 6, 14, 10, 10, 14, 6, 16, 12, 8,
        10, 0, 0, 10, 8, 12, 12, 8, 10, 0, 0, 10, 8, 12, 16, 6, 14, 10, 10, 14, 6, 16, 4, 2, 6, 8,
        8, 6, 2, 4, 18, 4, 16, 12, 12, 16, 4, 18,
    ];
    const CORNERS: u64 = 0x8100000000000081;
    let mobility = (legal.count_ones() + (legal & CORNERS).count_ones()) as i32;
    // The mover's discs on corners and on edge squares next to its own corners.
    let mover = child.opponent_bits();
    let corner = mover
        & (((mover & 0x0100000000000001) << 1)
            | ((mover & 0x8000000000000080) >> 1)
            | ((mover & 0x0000000000000081) << 8)
            | ((mover & 0x8100000000000000) >> 8)
            | CORNERS);
    let parity = if child.empty_count() < 11 { 8 } else { 4 };
    (36 - mobility) * (1 << 15)
        + corner.count_ones() as i32 * (1 << 11)
        + SQUARE_VALUE[bit.trailing_zeros() as usize]
        + if bit & odd != 0 { parity } else { 0 }
}

#[derive(Clone, Copy)]
pub(super) struct EndMove {
    square: u8,
    flips: u64,
    legal: u64,
    ordering: i32,
    probe: EndProbe,
}
