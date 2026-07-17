use crate::board::board::Board;
use crate::board::constant::NO_COORD;

use super::evaluator_const::*;

#[repr(align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureIndexes {
    pub(crate) feature_indexes: [u16; N_FEATURES],
}

impl Default for FeatureIndexes {
    fn default() -> Self {
        Self {
            feature_indexes: [0; N_FEATURES],
        }
    }
}

impl FeatureIndexes {
    /// 盤面から `FeatureIndexes` を生成する。
    pub fn from_board(board: &Board) -> Self {
        let mut state = Self::default();
        state.refresh(board);
        state
    }

    pub fn indexes(&self) -> &[u16; N_FEATURES] {
        &self.feature_indexes
    }

    /// 現在の index に対して、1 マスの三値状態の差分を反映する。
    ///
    /// `delta` は empty/opponent/player = 0/1/2 の差で、合法着手から
    /// 作る場合は -1, +1, +2 のいずれかになる。
    #[inline(always)]
    pub(crate) fn add_square_delta(&mut self, square: usize, delta: i16) {
        debug_assert!(square < N_BOARD_SQUARES);
        let square_features = unsafe { SQUARE_TO_FEATURES.get_unchecked(square) };
        for feature in &square_features.features[..square_features.len as usize] {
            let index = unsafe {
                self.feature_indexes
                    .get_unchecked_mut(feature.feature_idx as usize)
            };
            let next = *index as i32 + delta as i32 * feature.base3_weight as i32;
            debug_assert!(next >= 0);
            *index = next as u16;
        }
    }

    /// 親盤面を手番反転して作った index から、合法手後の index を作る。
    #[inline(always)]
    pub(crate) fn child_from_swapped(mut self, move_bit: u64, flip_bit: u64) -> Self {
        let mut flips = flip_bit;
        while flips != 0 {
            let square = flips.trailing_zeros() as usize;
            flips &= flips - 1;
            self.add_square_delta(square, -1);
        }
        self.add_square_delta(move_bit.trailing_zeros() as usize, 1);
        self
    }

    /// 盤面から FeatureIndexes を再計算する。
    pub fn refresh(&mut self, board: &Board) {
        self.feature_indexes = [0; N_FEATURES];
        let square_states = board_square_states(board);
        self.refresh_from_square_states(&square_states);
    }

    #[inline(always)]
    fn refresh_from_square_states(&mut self, square_states: &[u8; N_BOARD_SQUARES]) {
        unsafe { self.refresh_from_square_states_unchecked(square_states) }
    }

    #[inline(always)]
    unsafe fn refresh_from_square_states_unchecked(
        &mut self,
        square_states: &[u8; N_BOARD_SQUARES],
    ) {
        // pattern 定義は固定長で、square も盤面内に収まる前提なので
        // ここでは境界チェックを外して feature を詰める。
        let pattern_indexes = self.feature_indexes.as_mut_ptr();
        let square_states = square_states.as_ptr();

        for pattern_idx in 0..N_PATTERNS {
            let pattern = &PATTERN_DEFINITIONS[pattern_idx];
            for rotation in 0..N_ROTATIONS {
                let feature_idx = pattern_idx * N_ROTATIONS + rotation;
                let mut pattern_index = 0u16;
                for pattern_square_idx in 0..pattern.n_squares {
                    let square = pattern.squares[rotation][pattern_square_idx];
                    debug_assert_ne!(square, NO_COORD);
                    pattern_index = pattern_index * 3 + *square_states.add(square as usize) as u16;
                }
                *pattern_indexes.add(feature_idx) = pattern_index;
            }
        }
    }
}

pub(crate) fn board_square_states(board: &Board) -> [u8; N_BOARD_SQUARES] {
    let mut square_states = [0; N_BOARD_SQUARES];
    let mut opponent_bits = board.opponent;

    // 0 = empty, 1 = opponent, 2 = player
    while opponent_bits != 0 {
        let square = opponent_bits.trailing_zeros() as usize;
        square_states[square] = 1;
        opponent_bits &= opponent_bits - 1;
    }

    let mut player_bits = board.player;
    while player_bits != 0 {
        let square = player_bits.trailing_zeros() as usize;
        square_states[square] = 2;
        player_bits &= player_bits - 1;
    }

    square_states
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    const D3: u64 = 1u64 << 19;

    fn played_board() -> Board {
        let board = Board::new();
        let flip = board.flip_bit(D3);
        board.make_move_from_flip_bit(D3, flip);
        board
    }

    #[test]
    fn refresh_matches_recreated_state() {
        let initial = Board::new();
        let played = played_board();

        let mut state = FeatureIndexes::from_board(&initial);
        state.refresh(&played);

        assert_eq!(state, FeatureIndexes::from_board(&played));
    }

    #[test]
    fn square_delta_matches_recreated_state_after_move() {
        let board = Board::new();
        let move_bit = 1u64 << 19;
        let flip = board.flip_bit(move_bit);
        let child = board.make_move_from_flip_bit(move_bit, flip);

        // child の手番から見ると、親の opponent/player index を交換した状態が基準。
        let child_indexes =
            FeatureIndexes::from_board(&board.passed()).child_from_swapped(move_bit, flip);

        assert_eq!(child_indexes, FeatureIndexes::from_board(&child));
    }

    #[test]
    fn square_delta_matches_recreated_state_during_game() {
        let mut board = Board::new();
        let mut choice = 0usize;

        for _ in 0..60 {
            let mut moves = board.moves();
            if moves == 0 {
                board = board.passed();
                moves = board.moves();
                if moves == 0 {
                    break;
                }
            }

            let swapped = FeatureIndexes::from_board(&board.passed());
            let mut candidates = moves;
            while candidates != 0 {
                let move_bit = candidates & candidates.wrapping_neg();
                candidates &= candidates - 1;
                let flip = board.flip_bit(move_bit);
                let child = board.make_move_from_flip_bit(move_bit, flip);
                assert_eq!(
                    swapped.child_from_swapped(move_bit, flip),
                    FeatureIndexes::from_board(&child)
                );
            }

            let selected = choice % moves.count_ones() as usize;
            let mut move_bit = moves & moves.wrapping_neg();
            for _ in 0..selected {
                moves &= moves - 1;
                move_bit = moves & moves.wrapping_neg();
            }
            board = board.make_move(move_bit);
            choice = choice.wrapping_mul(5).wrapping_add(3);
        }
    }

    #[test]
    fn square_to_features_matches_pattern_definitions() {
        let mut expected = [[0u16; N_FEATURES]; N_BOARD_SQUARES];
        for pattern_idx in 0..N_PATTERNS {
            let pattern = &PATTERN_DEFINITIONS[pattern_idx];
            for rotation in 0..N_ROTATIONS {
                let feature_idx = pattern_idx * N_ROTATIONS + rotation;
                for pattern_square_idx in 0..pattern.n_squares {
                    let square = pattern.squares[rotation][pattern_square_idx] as usize;
                    expected[square][feature_idx] =
                        POW3[pattern.n_squares - 1 - pattern_square_idx] as u16;
                }
            }
        }

        for square in 0..N_BOARD_SQUARES {
            let square_features = &SQUARE_TO_FEATURES[square];
            assert!(square_features.len as usize <= MAX_SQUARE_FEATURES);

            let mut actual = [0u16; N_FEATURES];
            for feature in &square_features.features[..square_features.len as usize] {
                actual[feature.feature_idx as usize] = feature.base3_weight;
            }

            assert_eq!(actual, expected[square]);
        }
    }

    #[test]
    fn pattern_table_sizes_match_pattern_definitions() {
        for pattern_idx in 0..N_PATTERNS {
            assert_eq!(
                PATTERN_TABLE_SIZES[pattern_idx],
                POW3[PATTERN_DEFINITIONS[pattern_idx].n_squares]
            );
        }
    }

    fn naive_pattern_indexes(board: &Board) -> [u16; N_FEATURES] {
        let mut pattern_indexes = [0; N_FEATURES];
        let player_bits = board.player;
        let opponent_bits = board.opponent;

        for pattern_idx in 0..N_PATTERNS {
            let pattern = &PATTERN_DEFINITIONS[pattern_idx];
            for rotation in 0..N_ROTATIONS {
                let feature_idx = pattern_idx * N_ROTATIONS + rotation;
                let mut pattern_index = 0u16;
                for pattern_square_idx in 0..pattern.n_squares {
                    let square = pattern.squares[rotation][pattern_square_idx];
                    let square_state =
                        2 * ((player_bits >> square) & 1) + ((opponent_bits >> square) & 1);
                    pattern_index = pattern_index * 3 + square_state as u16;
                }
                pattern_indexes[feature_idx] = pattern_index;
            }
        }

        pattern_indexes
    }

    fn sample_boards(limit: usize) -> Vec<Board> {
        let mut boards = Vec::with_capacity(limit);
        let mut frontier = vec![Board::new()];

        while let Some(board) = frontier.pop() {
            boards.push(board);
            if boards.len() >= limit {
                break;
            }

            let mut moves = board.moves();
            if moves == 0 {
                if board.opponent_moves() != 0 {
                    frontier.push(board.passed());
                }
                continue;
            }

            while moves != 0 {
                let move_bit = 1u64 << moves.trailing_zeros();
                moves &= moves - 1;

                frontier.push(board.make_move(move_bit));
            }
        }

        boards
    }

    #[test]
    #[ignore]
    fn benchmark_refresh_implementations() {
        let boards = sample_boards(256);
        let iterations = 2_000;

        let start = Instant::now();
        let mut naive_checksum = 0u64;
        for _ in 0..iterations {
            for board in &boards {
                let pattern_indexes = naive_pattern_indexes(black_box(board));
                naive_checksum ^= black_box(pattern_indexes[0] as u64);
            }
        }
        let naive_elapsed = start.elapsed();

        let start = Instant::now();
        let mut optimized_checksum = 0u64;
        for _ in 0..iterations {
            for board in &boards {
                let mut state = FeatureIndexes::default();
                state.refresh(black_box(board));
                optimized_checksum ^= black_box(state.feature_indexes[0] as u64);
            }
        }
        let optimized_elapsed = start.elapsed();

        eprintln!(
            "refresh naive: {:?}, square_states: {:?}, checksums: {} / {}",
            naive_elapsed, optimized_elapsed, naive_checksum, optimized_checksum
        );
    }
}
