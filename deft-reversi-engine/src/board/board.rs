use super::bit::*;
use super::flip::*;
use super::moves::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Board {
    pub player: u64,
    pub opponent: u64,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            player: 0x0000000810000000u64,
            opponent: 0x0000001008000000u64,
        }
    }
}

impl Board {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn make_pass(&mut self) {
        (self.player, self.opponent) = (self.opponent, self.player);
    }

    #[inline(always)]
    pub fn passed(&self) -> Board {
        Board {
            player: self.opponent,
            opponent: self.player,
        }
    }

    pub fn clear(&mut self) {
        self.player = 0x0000000810000000u64;
        self.opponent = 0x0000001008000000u64;
    }

    #[inline(always)]
    pub fn flip_bit(&self, move_bit: u64) -> u64 {
        debug_assert!(move_bit.count_ones() == 1);

        #[cfg(target_feature = "avx2")]
        unsafe {
            flip_avx2(
                move_bit.trailing_zeros() as usize,
                self.player,
                self.opponent,
            )
        }

        #[cfg(not(target_feature = "avx2"))]
        flip_std(move_bit, self.player, self.opponent)
    }

    #[inline(always)]
    pub fn make_move(self, move_bit: u64) -> Board {
        debug_assert!(move_bit.count_ones() == 1);
        debug_assert!(self.moves() & move_bit != 0);

        let flip_bit = self.flip_bit(move_bit);
        Board {
            player: self.opponent ^ flip_bit,
            opponent: self.player ^ (flip_bit | move_bit),
        }
    }

    #[inline(always)]
    pub fn make_move_from_flip_bit(&mut self, move_bit: u64, flip_bit: u64) {
        debug_assert!(move_bit.count_ones() == 1);
        debug_assert!(self.moves() & move_bit != 0);
        debug_assert!(self.flip_bit(move_bit) == flip_bit);

        self.player ^= flip_bit | move_bit;
        self.opponent ^= flip_bit;
        (self.player, self.opponent) = (self.opponent, self.player);
    }

    #[inline(always)]
    pub fn opponent_moves(&self) -> u64 {
        get_moves(self.opponent, self.player)
    }

    #[inline(always)]
    pub fn moves(&self) -> u64 {
        get_moves(self.player, self.opponent)
    }

    pub fn all_symmetries(&self) -> [Board; 8] {
        std::array::from_fn(|i| {
            let mut sym_board = self.clone();
            if (i & 0b0001) != 0 {
                sym_board.player = horizontal_mirror(sym_board.player);
                sym_board.opponent = horizontal_mirror(sym_board.opponent);
            }
            if (i & 0b0010) != 0 {
                sym_board.player = vertical_mirror(sym_board.player);
                sym_board.opponent = vertical_mirror(sym_board.opponent);
            }
            if (i & 0b0100) != 0 {
                sym_board.player = transpose(sym_board.player);
                sym_board.opponent = transpose(sym_board.opponent);
            }
            sym_board
        })
    }

    pub fn all_rotations(&self) -> [Board; 4] {
        let no_rotation = self.clone();

        let mut rotate_90_degrees = self.clone();
        rotate_90_degrees.player = vertical_mirror(rotate_90_degrees.player);
        rotate_90_degrees.opponent = vertical_mirror(rotate_90_degrees.opponent);
        rotate_90_degrees.player = transpose(rotate_90_degrees.player);
        rotate_90_degrees.opponent = transpose(rotate_90_degrees.opponent);

        let mut rotate_180_degrees = self.clone();
        rotate_180_degrees.player = vertical_mirror(rotate_180_degrees.player);
        rotate_180_degrees.opponent = vertical_mirror(rotate_180_degrees.opponent);
        rotate_180_degrees.player = horizontal_mirror(rotate_180_degrees.player);
        rotate_180_degrees.opponent = horizontal_mirror(rotate_180_degrees.opponent);

        let mut rotate_270_degrees = self.clone();
        rotate_270_degrees.player = horizontal_mirror(rotate_270_degrees.player);
        rotate_270_degrees.opponent = horizontal_mirror(rotate_270_degrees.opponent);
        rotate_270_degrees.player = transpose(rotate_270_degrees.player);
        rotate_270_degrees.opponent = transpose(rotate_270_degrees.opponent);

        [
            no_rotation,
            rotate_90_degrees,
            rotate_180_degrees,
            rotate_270_degrees,
        ]
    }

    /// Returns the current move count (number of moves played so far).
    /// The count starts at 0 and ends at 60 (excluding passes).
    #[inline(always)]
    pub fn move_count(&self) -> i32 {
        (self.player | self.opponent).count_ones() as i32 - 4
    }

    pub fn unique_board(&self) -> Board {
        self.all_symmetries().into_iter().min().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D3: u64 = 1u64 << 19;
    const C4: u64 = 1u64 << 26;
    const D4: u64 = 1u64 << 27;
    const E4: u64 = 1u64 << 28;
    const D5: u64 = 1u64 << 35;
    const E5: u64 = 1u64 << 36;
    const F5: u64 = 1u64 << 37;
    const E6: u64 = 1u64 << 44;
    const H8: u64 = 1u64 << 63;

    #[test]
    fn new_board_has_initial_position() {
        let board = Board::new();

        assert_eq!(board.player, D5 | E4);
        assert_eq!(board.opponent, D4 | E5);
        assert_eq!(board.move_count(), 0);
    }

    #[test]
    fn initial_board_has_four_legal_moves() {
        let board = Board::new();

        assert_eq!(board.moves(), D3 | C4 | F5 | E6);
    }

    #[test]
    fn flip_bit_returns_flipped_discs_for_legal_move() {
        let board = Board::new();

        assert_eq!(board.flip_bit(D3), D4);
    }

    #[test]
    fn make_move_applies_move_and_changes_turn() {
        let board = Board::new();

        let board = board.make_move(D3);

        assert_eq!(board.player, E5);
        assert_eq!(board.opponent, D3 | D4 | D5 | E4);
        assert_eq!(board.move_count(), 1);
    }

    #[test]
    fn make_move_from_flip_bit_matches_make_move() {
        let board_from_move = Board::new();
        let mut board_from_flip = Board::new();
        let flip_bit = board_from_flip.flip_bit(D3);

        let board_from_move = board_from_move.make_move(D3);
        board_from_flip.make_move_from_flip_bit(D3, flip_bit);

        assert_eq!(board_from_flip.player, board_from_move.player);
        assert_eq!(board_from_flip.opponent, board_from_move.opponent);
    }

    #[test]
    fn make_pass_swaps_player_and_opponent() {
        let mut board = Board::new();
        let player = board.player;
        let opponent = board.opponent;

        board.make_pass();

        assert_eq!(board.player, opponent);
        assert_eq!(board.opponent, player);
    }

    #[test]
    fn passed_returns_swapped_board_without_mutating_original() {
        let board = Board::new();
        let passed = board.passed();

        assert_eq!(passed.player, board.opponent);
        assert_eq!(passed.opponent, board.player);
        assert_eq!(board.player, D5 | E4);
        assert_eq!(board.opponent, D4 | E5);
    }

    #[test]
    fn symmetries_and_rotations_include_original_first() {
        let board = Board::new();
        let symmetries = board.all_symmetries();
        let rotations = board.all_rotations();

        assert_eq!(symmetries.len(), 8);
        assert_eq!(symmetries[0].player, board.player);
        assert_eq!(symmetries[0].opponent, board.opponent);
        assert_eq!(rotations.len(), 4);
        assert_eq!(rotations[0].player, board.player);
        assert_eq!(rotations[0].opponent, board.opponent);
    }

    #[test]
    fn unique_board_is_minimum_symmetry() {
        let board = Board {
            player: D3 | E4 | H8,
            opponent: C4 | D4 | E5,
        };
        let unique_board = board.unique_board();
        let expected = board.all_symmetries().into_iter().min().unwrap();

        assert_eq!(unique_board.player, expected.player);
        assert_eq!(unique_board.opponent, expected.opponent);
    }
}
