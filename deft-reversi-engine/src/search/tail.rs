use super::tree::odd_regions;
use super::*;
pub(super) const TAIL_EMPTIES: u8 = 6;
pub(super) const LOCAL_END_MAX: u8 = 13;
const LOCAL_SLOTS: usize = 2048;

impl Context {
    /// Exact tail: features and TT are unnecessary once no heuristic is consulted.
    pub(super) fn tail_entered(
        &mut self,
        board: Board,
        mut alpha: i32,
        beta: i32,
    ) -> Result<Line, ()> {
        if beta - alpha == 1 {
            return self.tail_score_entered(board, alpha, beta).map(Line::leaf);
        }
        let mut moves = board.get_moves();
        if moves == 0 {
            if board.get_opponent_moves() == 0 {
                self.stats.leaves += 1;
                return Ok(Line::leaf(board.final_score()));
            }
            self.enter()?;
            let child = self.tail_entered(board.board_after_pass(), -beta, -alpha)?;
            return Ok(Line::prepend(Action::Pass, &child));
        }
        if board.empty_count() == 1 {
            self.stats.leaves += 1;
            let bit = moves & moves.wrapping_neg();
            let flips = board.flip_mask(bit);
            let mut line = Line::leaf(
                2 * (board.player_bits().count_ones() + flips.count_ones() + 1) as i32 - 64,
            );
            line.actions[0] = Some(Action::Place(
                Square::new(bit.trailing_zeros() as u8).unwrap(),
            ));
            line.len = 1;
            return Ok(line);
        }
        let mut preferred = moves & odd_regions(board);
        let mut best = Line::leaf(-65);
        while moves != 0 {
            let options = if preferred != 0 { preferred } else { moves };
            let bit = options & options.wrapping_neg();
            preferred &= !bit;
            moves &= !bit;
            let child_board = board.after_known_move(bit, board.flip_mask(bit));
            self.enter()?;
            let child = self.tail_entered(child_board, -beta, -alpha)?;
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

impl Context {
    /// Null-window tail has no PV to preserve. Avoid copying a PV at every leaf.
    pub(super) fn tail_score_entered(
        &mut self,
        board: Board,
        alpha: i32,
        beta: i32,
    ) -> Result<i32, ()> {
        self.endgame_score_entered(board, alpha, beta, None)
    }
    fn endgame_score_entered(
        &mut self,
        board: Board,
        alpha: i32,
        beta: i32,
        known_moves: Option<u64>,
    ) -> Result<i32, ()> {
        let empties = board.empty_count();
        if empties < TAIL_EMPTIES {
            return self.tail_score_impl(board, alpha, beta);
        }
        debug_assert!(empties <= LOCAL_END_MAX);
        let layer = usize::from(empties - TAIL_EMPTIES);
        if self.end_cache.is_empty() {
            self.end_cache
                .resize_with(usize::from(LOCAL_END_MAX - TAIL_EMPTIES + 1), Vec::new);
        }
        if self.end_cache[layer].is_empty() {
            self.end_cache[layer].resize(
                LOCAL_SLOTS,
                EndEntry {
                    quality: 255,
                    player: 0,
                    opponent: 0,
                    lower: -64,
                    upper: 64,
                },
            );
        }
        let hash = (board.player_bits() ^ board.opponent_bits().rotate_left(32))
            .wrapping_mul(0x9e3779b97f4a7c15);
        let index = (hash >> 53) as usize;
        let old = self.end_cache[layer][index];
        let quality = self.runtime.quality.load(Ordering::Relaxed);
        let same = old.player == board.player_bits()
            && old.opponent == board.opponent_bits()
            && old.quality == quality;
        if same {
            self.stats.tt_hits += 1;
            if i32::from(old.lower) >= beta {
                self.stats.tt_cuts += 1;
                return Ok(i32::from(old.lower));
            }
            if i32::from(old.upper) <= alpha {
                self.stats.tt_cuts += 1;
                return Ok(i32::from(old.upper));
            }
        }
        let score = if empties > TAIL_EMPTIES {
            self.ordered_endgame(board, alpha, beta, known_moves)?
        } else {
            self.tail_score_impl(board, alpha, beta)?
        };
        let (lower, upper) = if score >= beta {
            (score as i8, if same { old.upper } else { 64 })
        } else {
            (if same { old.lower } else { -64 }, score as i8)
        };
        self.end_cache[layer][index] = EndEntry {
            quality,
            player: board.player_bits(),
            opponent: board.opponent_bits(),
            lower,
            upper,
        };
        Ok(score)
    }
    /// Compact exact NWS for the last 7..13 empties. No shared TT or PV traffic.
    fn ordered_endgame(
        &mut self,
        board: Board,
        mut alpha: i32,
        beta: i32,
        known_moves: Option<u64>,
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
                .endgame_score_entered(passed, -beta, -alpha, Some(legal))
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
            let child = board.after_known_move(moves, board.flip_mask(moves));
            self.enter()?;
            return self
                .endgame_score_entered(child, -beta, -alpha, None)
                .map(|score| -score);
        }
        let mut candidates = [EndMove {
            bit: 0,
            flips: 0,
            legal: 0,
            ordering: 0,
        }; LOCAL_END_MAX as usize];
        let odd = odd_regions(board);
        let mut count = 0;
        let mut best = -65;
        while moves != 0 {
            let bit = moves & moves.wrapping_neg();
            moves &= moves - 1;
            let flips = board.flip_mask(bit);
            if flips == board.opponent_bits() {
                self.stats.leaves += 1;
                return Ok(64);
            }
            let child = board.after_known_move(bit, flips);
            if let Some(entry) = self.local_end_entry(child) {
                self.stats.tt_hits += 1;
                let lower = -i32::from(entry.upper);
                let upper = -i32::from(entry.lower);
                if lower >= beta {
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
                let score = -self.endgame_score_entered(child, -beta, -alpha, Some(legal))?;
                best = best.max(score);
                if score >= beta {
                    return Ok(score);
                }
                continue;
            }
            let ordering = super::tree::endgame_ordering(legal, bit, odd);
            candidates[count] = EndMove {
                bit,
                flips,
                legal,
                ordering,
            };
            count += 1;
        }
        for i in 0..count {
            let index = (i..count).max_by_key(|&n| candidates[n].ordering).unwrap();
            candidates.swap(i, index);
            let action = candidates[i];
            let child = board.after_known_move(action.bit, action.flips);
            self.enter()?;
            let score = -self.endgame_score_entered(child, -beta, -alpha, Some(action.legal))?;
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                break;
            }
        }
        Ok(best)
    }
    fn local_end_entry(&self, board: Board) -> Option<EndEntry> {
        let empties = board.empty_count();
        if !(TAIL_EMPTIES..=LOCAL_END_MAX).contains(&empties) {
            return None;
        }
        let hash = (board.player_bits() ^ board.opponent_bits().rotate_left(32))
            .wrapping_mul(0x9e3779b97f4a7c15);
        let entry = *self
            .end_cache
            .get(usize::from(empties - TAIL_EMPTIES))?
            .get((hash >> 53) as usize)?;
        (entry.player == board.player_bits()
            && entry.opponent == board.opponent_bits()
            && entry.quality == self.runtime.quality.load(Ordering::Relaxed))
        .then_some(entry)
    }
    fn tail_score_impl(&mut self, board: Board, mut alpha: i32, beta: i32) -> Result<i32, ()> {
        let empty = !(board.player_bits() | board.opponent_bits());
        let count = empty.count_ones() as u8;
        if count == 4 {
            return self.last_four_score(board, alpha);
        }
        if count <= 3 {
            return self.small_score(
                super::small::SmallPosition {
                    board,
                    empty,
                    odd: odd_regions(board),
                    count,
                },
                alpha,
                beta,
                false,
            );
        }
        let mut moves = board.get_moves();
        if moves == 0 {
            if board.get_opponent_moves() == 0 {
                self.stats.leaves += 1;
                return Ok(board.final_score());
            }
            self.enter()?;
            return self
                .tail_score_entered(board.board_after_pass(), -beta, -alpha)
                .map(|score| -score);
        }
        let mut preferred = moves & odd_regions(board);
        let mut best = -65;
        while moves != 0 {
            let options = if preferred != 0 { preferred } else { moves };
            let bit = options & options.wrapping_neg();
            preferred &= !bit;
            moves &= !bit;
            let child = board.after_known_move(bit, board.flip_mask(bit));
            self.enter()?;
            let score = -self.tail_score_entered(child, -beta, -alpha)?;
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                break;
            }
        }
        Ok(best)
    }
}

#[inline(always)]
/// Exact when needed; otherwise a safe fail-low upper bound for this window.
pub(super) fn last_square_bound(board: Board, empty: u64, alpha: i32) -> i32 {
    if empty == 0 {
        return board.final_score();
    }
    let square = empty.trailing_zeros() as usize;
    let base = 2 * board.player_bits().count_ones() as i32;
    let flips = crate::board::last::doubled_flips(square, board.player_bits());
    if flips != 0 {
        return base + flips - 62;
    }
    let difference = base - 63;
    let upper = difference + difference.signum();
    if upper <= alpha {
        return upper;
    }
    let flips = crate::board::last::doubled_flips(square, board.opponent_bits());
    if flips != 0 {
        return base - flips - 64;
    }
    upper
}

#[derive(Clone, Copy)]
struct EndMove {
    bit: u64,
    flips: u64,
    legal: u64,
    ordering: i32,
}

#[cfg(test)]
mod tests {
    use super::super::session::test_context as context;
    use super::*;
    fn brute(board: Board) -> i32 {
        let mut moves = board.get_moves();
        if moves == 0 {
            return if board.is_terminal() {
                board.final_score()
            } else {
                -brute(board.board_after_pass())
            };
        }
        let mut best = -65;
        while moves != 0 {
            let bit = moves & moves.wrapping_neg();
            moves &= moves - 1;
            best = best.max(-brute(board.board_after_move(bit)));
        }
        best
    }
    #[test]
    fn score_tail_preserves_upper_and_lower_bounds() {
        let mut context = context();
        let mut boards = vec![
            Board::from_bits(1u64 << 62, !(1u64 << 62 | 1u64 << 63)).unwrap(),
            Board::from_bits(!(1u64 << 63), 0).unwrap(),
            Board::from_bits(u64::MAX, 0).unwrap(),
        ];
        for seed in 1..=8u64 {
            let mut random = seed;
            let mut board = Board::new();
            while board.empty_count() > 8 && !board.is_terminal() {
                let mut moves = board.get_moves();
                if moves == 0 {
                    board = board.board_after_pass();
                    continue;
                }
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                for _ in 0..random % u64::from(moves.count_ones()) {
                    moves &= moves - 1;
                }
                board = board.board_after_move(moves & moves.wrapping_neg());
            }
            boards.push(board);
            boards.push(board.board_after_pass());
        }
        for board in boards {
            let expected = brute(board);
            for alpha in [-65, -21, -1, 0, 20, 63, 64] {
                let actual = context.tail_score_entered(board, alpha, alpha + 1).unwrap();
                if expected <= alpha {
                    assert!(
                        expected <= actual && actual <= alpha,
                        "upper: expected={expected} actual={actual} alpha={alpha}"
                    );
                } else {
                    assert!(
                        alpha < actual && actual <= expected,
                        "lower: expected={expected} actual={actual} alpha={alpha}"
                    );
                }
            }
        }
    }
}
