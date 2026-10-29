use crate::board::{flip, moves};
use crate::{Action, Moves, Square};

/// A rejected action leaves the board unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveError {
    IllegalMove,
    IllegalPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Board {
    pub(crate) player: u64,
    pub(crate) opponent: u64,
}

impl std::fmt::Display for MoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::IllegalMove => "illegal move",
            Self::IllegalPass => "pass is not permitted",
        })
    }
}
impl std::error::Error for MoveError {}
impl Default for Board {
    fn default() -> Self {
        Board {
            player: 0x0000000810000000u64,
            opponent: 0x0000001008000000u64,
        }
    }
}

impl Board {
    /// Constructs a position with disjoint player and opponent stones.
    pub fn from_bits(player: u64, opponent: u64) -> Option<Self> {
        (player & opponent == 0).then_some(Self { player, opponent })
    }

    /// Validates a single-square move and advances to the opponent's turn.
    fn try_play(&mut self, move_mask: u64) -> Result<(), MoveError> {
        if move_mask.count_ones() != 1 || self.get_moves() & move_mask == 0 {
            return Err(MoveError::IllegalMove);
        }
        self.make_move(move_mask);
        Ok(())
    }

    /// Passes only when this player has no move and the opponent does.
    fn try_pass(&mut self) -> Result<(), MoveError> {
        if self.get_moves() != 0 || self.get_opponent_moves() == 0 {
            return Err(MoveError::IllegalPass);
        }
        self.make_pass();
        Ok(())
    }

    pub fn is_terminal(&self) -> bool {
        self.get_moves() == 0 && self.get_opponent_moves() == 0
    }

    pub fn player_bits(&self) -> u64 {
        self.player
    }
    pub fn opponent_bits(&self) -> u64 {
        self.opponent
    }
    pub fn empty_count(&self) -> u8 {
        (!(self.player | self.opponent)).count_ones() as u8
    }
    /// Terminal disc difference, awarding unoccupied squares to the winner.
    pub fn final_score(&self) -> i32 {
        let difference = self.player.count_ones() as i32 - self.opponent.count_ones() as i32;
        difference + difference.signum() * i32::from(self.empty_count())
    }
    /// The position after `action`, with the opponent to move.
    pub fn play(&self, action: Action) -> Result<Self, MoveError> {
        let mut next = *self;
        match action {
            Action::Place(square) => next.try_play(square.bit())?,
            Action::Pass => next.try_pass()?,
        }
        Ok(next)
    }
    pub fn legal_moves(&self) -> Moves {
        Moves(self.get_moves())
    }
    /// Whether the player to move must pass: no move here, but one for the opponent.
    pub fn must_pass(&self) -> bool {
        self.get_moves() == 0 && self.get_opponent_moves() != 0
    }
    /// The discs that a move on `square` turns over; zero if it is illegal.
    pub fn flips(&self, square: Square) -> u64 {
        if self.get_moves() & square.bit() == 0 {
            0
        } else {
            self.flip_mask(square.bit())
        }
    }
    pub(crate) fn after_known_move(&self, bit: u64, flips: u64) -> Self {
        Self {
            player: self.opponent ^ flips,
            opponent: self.player ^ flips ^ bit,
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn get_moves(&self) -> u64 {
        moves::get_moves(self.player, self.opponent)
    }

    #[inline(always)]
    pub(crate) fn get_opponent_moves(&self) -> u64 {
        moves::get_moves(self.opponent, self.player)
    }

    #[inline(always)]
    pub(crate) fn flip_mask(&self, move_mask: u64) -> u64 {
        flip::flip(move_mask, self.player, self.opponent)
    }

    #[inline(always)]
    /// Advances without validation. The caller must supply one legal move bit.
    pub(crate) fn board_after_move(&self, move_mask: u64) -> Board {
        let flip_mask = self.flip_mask(move_mask);
        Board {
            player: (self.opponent ^ flip_mask),
            opponent: (self.player ^ (flip_mask | move_mask)),
        }
    }

    #[inline(always)]
    /// Advances without validation. The caller must supply one legal move bit.
    pub(crate) fn make_move(&mut self, move_mask: u64) {
        let flip_mask = self.flip_mask(move_mask);
        let player_tmp = self.player;
        self.player = self.opponent ^ flip_mask;
        self.opponent = player_tmp ^ (flip_mask | move_mask);
    }

    #[inline(always)]
    pub(crate) fn make_pass(&mut self) {
        std::mem::swap(&mut self.player, &mut self.opponent);
    }

    #[inline(always)]
    pub(crate) fn board_after_pass(&self) -> Board {
        Board {
            player: self.opponent,
            opponent: self.player,
        }
    }

    #[inline(always)]
    fn transpose(self) -> Self {
        Self {
            player: transpose(self.player),
            opponent: transpose(self.opponent),
        }
    }

    #[inline(always)]
    fn mirror_horizontal(self) -> Self {
        Self {
            player: self.player.reverse_bits().swap_bytes(),
            opponent: self.opponent.reverse_bits().swap_bytes(),
        }
    }

    #[inline(always)]
    fn mirror_vertical(self) -> Self {
        Self {
            player: self.player.swap_bytes(),
            opponent: self.opponent.swap_bytes(),
        }
    }

    #[inline(always)]
    fn rotate_180(self) -> Self {
        Self {
            player: self.player.reverse_bits(),
            opponent: self.opponent.reverse_bits(),
        }
    }

    /// The least of the eight symmetric images, for symmetry-aware lookups.
    #[inline]
    pub fn canonical(self) -> Self {
        self.symmetries()
            .into_iter()
            .min_by_key(|board| (board.player, board.opponent))
            .unwrap()
    }

    /// Eight geometric symmetries in the same order used by `canonical`.
    pub fn symmetries(self) -> [Self; 8] {
        let transposed = self.transpose();

        [
            self,
            transposed,
            self.mirror_horizontal(),
            transposed.mirror_horizontal(),
            self.mirror_vertical(),
            transposed.mirror_vertical(),
            self.rotate_180(),
            transposed.rotate_180(),
        ]
    }
}

#[inline(always)]
fn transpose(mut bits: u64) -> u64 {
    let t = (bits ^ (bits >> 7)) & 0x00aa_00aa_00aa_00aa;
    bits ^= t ^ (t << 7);

    let t = (bits ^ (bits >> 14)) & 0x0000_cccc_0000_cccc;
    bits ^= t ^ (t << 14);

    let t = (bits ^ (bits >> 28)) & 0x0000_0000_f0f0_f0f0;
    bits ^ t ^ (t << 28)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The least-significant bit is A1, and bits proceed toward H8.
    const fn square(file: u8, rank: u8) -> u64 {
        1_u64 << ((rank - 1) * 8 + (file - b'A'))
    }

    #[test]
    fn new_returns_the_initial_position() {
        let board = Board::new();

        assert_eq!(board.player, square(b'E', 4) | square(b'D', 5));
        assert_eq!(board.opponent, square(b'D', 4) | square(b'E', 5));
        assert_eq!(board.player & board.opponent, 0);
    }

    #[test]
    fn default_returns_the_same_position_as_new() {
        let new_board = Board::new();
        let default_board = Board::default();

        assert_eq!(new_board.player, default_board.player);
        assert_eq!(new_board.opponent, default_board.opponent);
    }

    #[test]
    fn initial_position_has_four_expected_moves() {
        let board = Board::new();
        let expected = square(b'D', 3) | square(b'C', 4) | square(b'F', 5) | square(b'E', 6);

        let actual = board.get_moves();

        assert_eq!(actual, expected);
        assert_eq!(actual.count_ones(), 4);
        assert_eq!(actual & (board.player | board.opponent), 0);
    }

    #[test]
    fn each_initial_move_flips_the_expected_stone() {
        let board = Board::new();
        let cases = [
            (square(b'D', 3), square(b'D', 4)),
            (square(b'C', 4), square(b'D', 4)),
            (square(b'F', 5), square(b'E', 5)),
            (square(b'E', 6), square(b'E', 5)),
        ];

        for (move_bit, expected) in cases {
            assert_eq!(board.flip_mask(move_bit), expected);
        }
    }

    #[test]
    fn empty_and_full_positions_have_no_moves() {
        let empty = Board {
            player: 0,
            opponent: 0,
        };
        let full = Board {
            player: u64::MAX,
            opponent: 0,
        };

        assert_eq!(empty.get_moves(), 0);
        assert_eq!(full.get_moves(), 0);
    }

    #[test]
    fn move_generation_does_not_wrap_across_board_edges() {
        let board = Board {
            player: square(b'H', 1),
            opponent: square(b'A', 2),
        };

        assert_eq!(board.get_moves(), 0);
    }

    #[test]
    fn pass_flips_no_stones() {
        let board = Board::new();

        assert_eq!(board.flip_mask(0), 0);
    }

    #[test]
    fn test_transpose() {
        let bit: u64 = 0b10000001_01000000_00100000_00010000_00001000_00000100_00000010_00000001; // 対角線が立っている盤面
        let expected: u64 =
            0b10000000_01000000_00100000_00010000_00001000_00000100_00000010_10000001; // 転置後
        assert_eq!(transpose(bit), expected);
    }

    #[test]
    fn test_vertical_mirror() {
        let bit: u64 = 0x00000000FFFFFFFF; // 上半分が空、下半分が埋まった盤面
        let expected: u64 = 0xFFFFFFFF00000000; // 上下反転
        let b = Board {
            player: bit,
            opponent: bit,
        };
        let b = b.mirror_vertical();
        assert_eq!(b.player, expected);
        assert_eq!(b.opponent, expected);
    }

    #[test]
    fn test_horizontal_mirror() {
        let bit: u64 = 0x8000000000000001; // 左端と右端が埋まった盤面
        let expected: u64 = 0x0100000000000080; // 左右反転
        let b = Board {
            player: bit,
            opponent: bit,
        };
        let b = b.mirror_horizontal();
        assert_eq!(b.player, expected);
        assert_eq!(b.opponent, expected);
    }

    // canonical() の実装とは独立した、座標ベースの対称変換。
    fn transform_reference(bits: u64, symmetry: usize) -> u64 {
        let mut transformed = 0;

        for rank in 0..8 {
            for file in 0..8 {
                let source = rank * 8 + file;
                if bits & (1_u64 << source) == 0 {
                    continue;
                }

                let (new_file, new_rank) = match symmetry {
                    0 => (file, rank),         // identity
                    1 => (7 - rank, file),     // rotate 90 degrees
                    2 => (7 - file, 7 - rank), // rotate 180 degrees
                    3 => (rank, 7 - file),     // rotate 270 degrees
                    4 => (7 - file, rank),     // mirror left-right
                    5 => (file, 7 - rank),     // mirror top-bottom
                    6 => (rank, file),         // main-diagonal mirror
                    7 => (7 - rank, 7 - file), // anti-diagonal mirror
                    _ => unreachable!(),
                };
                transformed |= 1_u64 << (new_rank * 8 + new_file);
            }
        }

        transformed
    }

    fn symmetries_reference(board: Board) -> [Board; 8] {
        std::array::from_fn(|symmetry| Board {
            player: transform_reference(board.player, symmetry),
            opponent: transform_reference(board.opponent, symmetry),
        })
    }

    fn canonical_reference(board: Board) -> Board {
        symmetries_reference(board)
            .into_iter()
            .min_by_key(|board| (board.player, board.opponent))
            .unwrap()
    }

    #[test]
    fn canonical_matches_independent_reference_implementation() {
        let boards = [
            Board::new(),
            Board {
                player: square(b'A', 1) | square(b'C', 2) | square(b'H', 4) | square(b'B', 7),
                opponent: square(b'G', 1) | square(b'D', 3) | square(b'A', 6) | square(b'H', 8),
            },
            Board {
                player: 0,
                opponent: square(b'B', 2) | square(b'G', 8),
            },
            Board {
                player: u64::MAX,
                opponent: 0,
            },
        ];

        for board in boards {
            assert_eq!(board.canonical(), canonical_reference(board));
        }
    }

    #[test]
    fn every_symmetry_has_the_same_canonical_board() {
        let board = Board {
            player: square(b'A', 1) | square(b'C', 2) | square(b'H', 4) | square(b'B', 7),
            opponent: square(b'G', 1) | square(b'D', 3) | square(b'A', 6) | square(b'H', 8),
        };
        let expected = board.canonical();

        for (symmetry, transformed) in symmetries_reference(board).into_iter().enumerate() {
            assert_eq!(
                transformed.canonical(),
                expected,
                "canonical board differed for symmetry {symmetry}"
            );
        }
    }

    #[test]
    fn canonical_is_idempotent() {
        let board = Board {
            player: square(b'B', 1) | square(b'F', 4) | square(b'A', 8),
            opponent: square(b'H', 1) | square(b'C', 5),
        };

        let canonical = board.canonical();

        assert_eq!(canonical.canonical(), canonical);
    }

    #[test]
    fn canonical_uses_player_then_opponent_for_ordering() {
        let board = Board {
            player: square(b'H', 8),
            opponent: square(b'G', 8),
        };

        assert_eq!(
            board.canonical(),
            Board {
                player: square(b'A', 1),
                opponent: square(b'B', 1),
            }
        );
    }
}
