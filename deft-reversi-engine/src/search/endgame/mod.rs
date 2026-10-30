//! Exact endgame dispatch; move ordering, shallow kernels and TT storage are separate.
use super::{Aborted, Action, Board, Context, EvalState, Line, Ordering, Square};
mod last_four;
mod local_tt;
mod ordered;
pub(super) mod parity;
mod shallow;
mod small;
use local_tt::EndProbe;
pub(super) use local_tt::{EndgameCache, new_cache_epoch};
#[cfg(test)]
use ordered::EndMove;
#[cfg(test)]
use parity::{PARITY_REGIONS, SQUARE_PARITY};
use parity::{odd_regions, tail_parity};
pub(in crate::search) const TAIL_EMPTIES: u8 = 6;
pub(in crate::search) const LOCAL_END_MAX: u8 = 13;
/// Selective endgame searches try ProbCut from this many empties.
pub(in crate::search) const PROBCUT_MIN_EMPTIES: u8 = 12;
/// The selective stages before an exact solve stop ProbCut one empty earlier,
/// so more of their local results are exact and survive into the final stage.
pub(in crate::search) const EXACT_PREPARATION_PROBCUT_MIN: u8 = 13;

impl Context {
    pub(super) fn tail_entered(
        &mut self,
        board: Board,
        mut alpha: i32,
        beta: i32,
    ) -> Result<Line, Aborted> {
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
            let bit = moves.isolate_lowest_one();
            let flips = board.flip_mask(bit);
            let mut line = Line::leaf(
                2 * (board.player_bits().count_ones() + flips.count_ones() + 1) as i32 - 64,
            );
            line.actions[0] = Some(Action::Place(Square::lowest(bit)));
            line.len = 1;
            return Ok(line);
        }
        let mut preferred = moves & odd_regions(board);
        let mut best = Line::leaf(-65);
        while moves != 0 {
            let options = if preferred != 0 { preferred } else { moves };
            let bit = options.isolate_lowest_one();
            preferred &= !bit;
            moves &= !bit;
            let child_board = board.after_known_move(bit, board.flip_mask(bit));
            self.enter()?;
            let child = self.tail_entered(child_board, -beta, -alpha)?;
            if -child.score > best.score {
                best = Line::prepend(Action::Place(Square::lowest(bit)), &child);
            }
            alpha = alpha.max(best.score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                break;
            }
        }
        Ok(best)
    }
    pub(super) fn tail_score_entered(
        &mut self,
        board: Board,
        alpha: i32,
        beta: i32,
    ) -> Result<i32, Aborted> {
        self.endgame_score_entered(board, alpha, beta, None, None)
    }
    fn endgame_score_entered(
        &mut self,
        board: Board,
        alpha: i32,
        beta: i32,
        known_moves: Option<u64>,
        known_probe: Option<EndProbe>,
    ) -> Result<i32, Aborted> {
        let empties = board.empty_count();
        if empties < TAIL_EMPTIES {
            return self.tail_score_impl(board, alpha, beta);
        }
        debug_assert!(empties <= LOCAL_END_MAX);
        self.end_cache.prepare(empties);
        let quality = self.end_quality(empties);
        // A parent's snapshot remains a valid bound even if a sibling has since
        // replaced the slot. This cache is owned by this Context, and quality is
        // fixed until the search stage has joined all workers.
        let probe = known_probe.unwrap_or_else(|| self.end_cache.probe(board, quality));
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
        self.end_cache
            .store(board, quality, probe, score, beta, best_move);
        Ok(score)
    }
    /// Below the ProbCut range a local result is exact even inside a
    /// selective stage, so it is cached as exact (quality 0).
    fn end_quality(&self, empties: u8) -> u8 {
        if empties < self.runtime.endgame_probcut_min {
            0
        } else {
            self.runtime.quality.load(Ordering::Relaxed)
        }
    }
    fn tail_score_impl(&mut self, board: Board, alpha: i32, beta: i32) -> Result<i32, Aborted> {
        let empty = !(board.player_bits() | board.opponent_bits());
        let count = empty.count_ones() as u8;
        if count == 4 {
            return self.last_four_score(board, alpha);
        }
        if count <= 3 {
            return self.small_score(
                small::SmallPosition {
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
}

#[inline(always)]
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

#[cfg(test)]
mod tests {
    use super::super::session::test_context as context;
    use super::*;
    use crate::test_util::{random_board, random_move};
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
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            best = best.max(-brute(board.board_after_move(bit)));
        }
        best
    }
    fn seven_empty_board() -> Board {
        let board = random_board(1, 7);
        assert_eq!(board.empty_count(), 7);
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
                let moves = board.get_moves();
                if moves == 0 {
                    board = board.board_after_pass();
                    assert_eq!(tail_parity(board), parity);
                    continue;
                }
                let bit = random_move(moves, &mut random);
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
            let board = random_board(seed, 5);
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
        let probe = context.end_cache.probe(board, 0);
        assert!(probe.matched);
        assert!(!context.end_cache.probe(board, 1).matched);
        context.end_cache.clear();
        assert!(!context.end_cache.probe(board, 0).matched);
        assert_eq!(
            context
                .endgame_score_entered(board, expected, expected + 1, None, Some(probe))
                .unwrap(),
            expected
        );
    }
    #[test]
    fn selective_stage_caches_results_below_probcut_as_exact() {
        let board = seven_empty_board();
        let expected = brute(board);
        let mut context = context();
        context.runtime.quality.store(3, Ordering::Relaxed);
        let score = context
            .tail_score_entered(board, expected - 1, expected)
            .unwrap();
        assert!(score >= expected);
        // Seven empties never use ProbCut: the bound is exact for any stage.
        assert!(context.end_cache.probe(board, 0).matched);
        assert!(!context.end_cache.probe(board, 3).matched);
        // A position inside the ProbCut range keeps the stage's tag.
        let deep = random_board(5, PROBCUT_MIN_EMPTIES);
        assert_eq!(deep.empty_count(), PROBCUT_MIN_EMPTIES);
        context.tail_score_entered(deep, 0, 1).unwrap();
        assert!(context.end_cache.probe(deep, 3).matched);
        assert!(!context.end_cache.probe(deep, 0).matched);
    }
    #[test]
    fn child_tt_cut_saves_the_cutting_move() {
        let board = seven_empty_board();
        let expected = brute(board);
        let mut context = context();
        context
            .tail_score_entered(board, expected, expected + 1)
            .unwrap();
        context.end_cache.clear();
        let mut moves = board.get_moves();
        let winning = loop {
            assert_ne!(moves, 0);
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            if -brute(board.board_after_move(bit)) == expected {
                break bit;
            }
        };
        let child = board.board_after_move(winning);
        let probe = context.end_cache.probe(child, 0);
        context.end_cache.prepare(child.empty_count());
        context
            .end_cache
            .store(child, 0, probe, -expected, -expected, 64);
        let probe = context.end_cache.probe(child, 0);
        context
            .end_cache
            .store(child, 0, probe, -expected, -expected + 1, 64);
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
            let board = random_board(seed, 8);
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
