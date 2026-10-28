use super::tree::odd_regions;
use super::*;
pub(super) const TAIL_EMPTIES: u8 = 6;
pub(super) const LOCAL_END_MAX: u8 = 13;
const LOCAL_SLOTS: usize = 2048;
const QUADRANT_MASKS: [u64; 4] = [
    0x000000000f0f0f0f,
    0x00000000f0f0f0f0,
    0x0f0f0f0f00000000,
    0xf0f0f0f000000000,
];
const PARITY_REGIONS: [u64; 16] = {
    let mut masks = [0; 16];
    let mut parity = 0;
    while parity < 16 {
        let mut region = 0;
        while region < 4 {
            if parity & (1 << region) != 0 {
                masks[parity] |= QUADRANT_MASKS[region];
            }
            region += 1;
        }
        parity += 1;
    }
    masks
};
const SQUARE_PARITY: [u8; 64] = {
    let mut ids = [0; 64];
    let mut square = 0;
    while square < 64 {
        ids[square] = 1 << (((square >> 2) & 1) | ((square >> 4) & 2));
        square += 1;
    }
    ids
};
fn tail_parity(board: Board) -> u8 {
    let empty = !(board.player_bits() | board.opponent_bits());
    QUADRANT_MASKS
        .iter()
        .enumerate()
        .fold(0, |parity, (i, mask)| {
            parity | (((empty & mask).count_ones() as u8 & 1) << i)
        })
}

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
        self.endgame_score_entered(board, alpha, beta, None, None)
    }
    fn endgame_score_entered(
        &mut self,
        board: Board,
        alpha: i32,
        beta: i32,
        known_moves: Option<u64>,
        known_probe: Option<EndProbe>,
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
                    best: 64,
                },
            );
        }
        let quality = self.runtime.quality.load(Ordering::Relaxed);
        // A parent's snapshot remains a valid bound even if a sibling has since
        // replaced the slot. This cache is owned by this Context, and quality is
        // fixed until the search stage has joined all workers.
        let probe = known_probe.unwrap_or_else(|| self.local_end_probe(board, quality));
        if probe.matched {
            if known_probe.is_none() {
                self.stats.tt_hits += 1;
            }
            if i32::from(probe.lower) >= beta {
                self.stats.tt_cuts += 1;
                return Ok(i32::from(probe.lower));
            }
            if i32::from(probe.upper) <= alpha {
                self.stats.tt_cuts += 1;
                return Ok(i32::from(probe.upper));
            }
        }
        let mut best_move = probe.best;
        let score = if empties > TAIL_EMPTIES {
            self.ordered_endgame(board, alpha, beta, known_moves, &mut best_move)?
        } else {
            self.six_moves_score(
                board,
                alpha,
                known_moves.unwrap_or_else(|| board.get_moves()),
                tail_parity(board),
            )?
        };
        let (lower, upper) = if score >= beta {
            (score as i8, probe.upper)
        } else {
            (probe.lower, score as i8)
        };
        self.end_cache[layer][usize::from(probe.index)] = EndEntry {
            quality,
            player: board.player_bits(),
            opponent: board.opponent_bits(),
            lower,
            upper,
            best: best_move,
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
            let bit = moves & moves.wrapping_neg();
            moves &= moves - 1;
            let flips = board.flip_mask(bit);
            if flips == board.opponent_bits() {
                *best_move = bit.trailing_zeros() as u8;
                self.stats.leaves += 1;
                return Ok(64);
            }
            let child = board.after_known_move(bit, flips);
            let probe = self.local_end_probe(child, quality);
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
            let ordering = super::tree::endgame_ordering(legal, bit, odd);
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
    fn local_end_probe(&self, board: Board, quality: u8) -> EndProbe {
        let empties = board.empty_count();
        debug_assert!((TAIL_EMPTIES..=LOCAL_END_MAX).contains(&empties));
        let hash = (board.player_bits() ^ board.opponent_bits().rotate_left(32))
            .wrapping_mul(0x9e3779b97f4a7c15);
        let index = (hash >> 53) as u16;
        let mut probe = EndProbe {
            index,
            ..EndProbe::EMPTY
        };
        if let Some(entry) = self
            .end_cache
            .get(usize::from(empties - TAIL_EMPTIES))
            .and_then(|layer| layer.get(usize::from(index)))
            && entry.player == board.player_bits()
            && entry.opponent == board.opponent_bits()
            && entry.quality == quality
        {
            probe.lower = entry.lower;
            probe.upper = entry.upper;
            probe.best = entry.best;
            probe.matched = true;
        }
        probe
    }
    fn tail_score_impl(&mut self, board: Board, alpha: i32, beta: i32) -> Result<i32, ()> {
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
        debug_assert_eq!(count, 5);
        debug_assert_eq!(beta, alpha + 1);
        self.five_empty_score(board, alpha, tail_parity(board))
    }
    // Six empties still use the local TT and often arrive with known legal moves.
    fn six_moves_score(
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
        let mut preferred = moves & PARITY_REGIONS[usize::from(parity)];
        let mut best = -65;
        while moves != 0 {
            let options = if preferred != 0 { preferred } else { moves };
            let bit = options & options.wrapping_neg();
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
    // At five empties, discover legal moves lazily in the same parity/square
    // order as the bitboard path. A cutoff need not test all remaining squares.
    fn five_empty_score(&mut self, board: Board, alpha: i32, parity: u8) -> Result<i32, ()> {
        debug_assert_eq!(board.empty_count(), 5);
        let mut empty = !(board.player_bits() | board.opponent_bits());
        let mut preferred = empty & PARITY_REGIONS[usize::from(parity)];
        let child_alpha = -alpha - 1;
        let mut best = -65;
        while empty != 0 {
            let options = if preferred != 0 { preferred } else { empty };
            let bit = options & options.wrapping_neg();
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
    square: u8,
    flips: u64,
    legal: u64,
    ordering: i32,
    probe: EndProbe,
}

// Six bytes: no board copy, pointer, or evaluation state in the candidate.
#[derive(Clone, Copy)]
struct EndProbe {
    index: u16,
    lower: i8,
    upper: i8,
    best: u8,
    matched: bool,
}
impl EndProbe {
    const EMPTY: Self = Self {
        index: 0,
        lower: -64,
        upper: 64,
        best: 64,
        matched: false,
    };
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
    fn seven_empty_board() -> Board {
        let mut board = Board::new();
        let mut random = 1u64;
        while board.empty_count() > 7 {
            let mut moves = board.get_moves();
            if moves == 0 {
                assert!(!board.is_terminal());
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
        board
    }
    #[test]
    fn carried_parity_matches_board_regions_through_moves_and_passes() {
        for seed in 1..=32u64 {
            let mut random = seed;
            let mut board = Board::new();
            while !board.is_terminal() {
                let parity = tail_parity(board);
                assert_eq!(PARITY_REGIONS[usize::from(parity)], odd_regions(board));
                let mut moves = board.get_moves();
                if moves == 0 {
                    board = board.board_after_pass();
                    assert_eq!(tail_parity(board), parity);
                    continue;
                }
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                for _ in 0..random % u64::from(moves.count_ones()) {
                    moves &= moves - 1;
                }
                let bit = moves & moves.wrapping_neg();
                board = board.board_after_move(bit);
                assert_eq!(
                    tail_parity(board),
                    parity ^ SQUARE_PARITY[bit.trailing_zeros() as usize],
                );
            }
        }
    }
    #[test]
    fn five_empty_kernel_matches_exhaustive_for_both_turns_and_boundary_windows() {
        let empty = 0x1fu64;
        let mut boards = vec![
            Board::from_bits(!empty, 0).unwrap(),
            Board::from_bits(0, !empty).unwrap(),
        ];
        for seed in 1..=64u64 {
            let mut random = seed;
            let mut board = Board::new();
            while board.empty_count() > 5 && !board.is_terminal() {
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
            if board.empty_count() == 5 {
                boards.push(board);
                boards.push(board.board_after_pass());
            }
        }
        let mut pass_positions = 0;
        for board in boards {
            if board.get_moves() == 0 && board.get_opponent_moves() != 0 {
                pass_positions += 1;
            }
            let expected = brute(board);
            for alpha in [expected - 1, expected] {
                let mut context = context();
                let actual = context
                    .five_empty_score(board, alpha, tail_parity(board))
                    .unwrap();
                assert_eq!(actual, expected, "board={board:?} alpha={alpha}");
            }
        }
        assert!(pass_positions > 0);
    }
    #[test]
    fn probe_snapshot_survives_replacement_and_rejects_other_quality() {
        assert_eq!(std::mem::size_of::<EndProbe>(), 6);
        assert_eq!(std::mem::size_of::<EndMove>(), 32);
        let board = seven_empty_board();
        let expected = brute(board);
        let mut context = context();
        context
            .tail_score_entered(board, expected, expected + 1)
            .unwrap();
        let probe = context.local_end_probe(board, 0);
        let layer = usize::from(board.empty_count() - TAIL_EMPTIES);
        let slot = &mut context.end_cache[layer][usize::from(probe.index)];
        *slot = EndEntry {
            player: board.player_bits(),
            opponent: board.opponent_bits(),
            quality: 0,
            lower: expected as i8,
            upper: expected as i8,
            best: 64,
        };
        let probe = context.local_end_probe(board, 0);
        assert!(probe.matched);
        assert!(!context.local_end_probe(board, 1).matched);
        context.end_cache[layer][usize::from(probe.index)].quality = 255;
        assert!(!context.local_end_probe(board, 0).matched);
        assert_eq!(
            context
                .endgame_score_entered(board, expected, expected + 1, None, Some(probe))
                .unwrap(),
            expected
        );
    }
    #[test]
    fn child_tt_cut_saves_the_cutting_move() {
        let board = seven_empty_board();
        let expected = brute(board);
        let mut context = context();
        context
            .tail_score_entered(board, expected, expected + 1)
            .unwrap();
        for layer in &mut context.end_cache {
            for entry in layer {
                entry.quality = 255;
            }
        }
        let mut moves = board.get_moves();
        let winning = loop {
            assert_ne!(moves, 0);
            let bit = moves & moves.wrapping_neg();
            moves &= moves - 1;
            if -brute(board.board_after_move(bit)) == expected {
                break bit;
            }
        };
        let child = board.board_after_move(winning);
        let probe = context.local_end_probe(child, 0);
        context.end_cache[0].resize(
            LOCAL_SLOTS,
            EndEntry {
                player: 0,
                opponent: 0,
                lower: -64,
                upper: 64,
                best: 64,
                quality: 255,
            },
        );
        context.end_cache[0][usize::from(probe.index)] = EndEntry {
            player: child.player_bits(),
            opponent: child.opponent_bits(),
            lower: -expected as i8,
            upper: -expected as i8,
            best: 64,
            quality: 0,
        };
        let mut best = 64;
        assert_eq!(
            context
                .ordered_endgame(board, expected - 1, expected, None, &mut best)
                .unwrap(),
            expected
        );
        assert!(best < 64);
        assert_ne!(board.get_moves() & (1u64 << best), 0);
        assert!(-brute(board.board_after_move(1u64 << best)) >= expected);
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
