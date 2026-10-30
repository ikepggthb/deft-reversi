//! Deterministic random positions shared by the unit tests.
use crate::Board;

/// Advances a xorshift64 generator and returns its new state.
pub(crate) fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Picks one move of a non-empty move mask.
pub(crate) fn random_move(mut moves: u64, random: &mut u64) -> u64 {
    for _ in 0..xorshift(random) % u64::from(moves.count_ones()) {
        moves &= moves - 1;
    }
    moves.isolate_lowest_one()
}

/// Plays random moves from the initial position, passing when needed, until
/// `empties` remain or the game ends.
pub(crate) fn random_board(seed: u64, empties: u8) -> Board {
    let mut board = Board::new();
    let mut random = seed;
    while board.empty_count() > empties && !board.is_terminal() {
        let moves = board.get_moves();
        if moves == 0 {
            board = board.board_after_pass();
            continue;
        }
        board = board.board_after_move(random_move(moves, &mut random));
    }
    board
}
