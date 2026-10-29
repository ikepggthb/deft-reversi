use crate::Board;

/// Counts leaf positions through the same unchecked transitions as the search.
/// A forced pass consumes one ply here; a terminal position counts as one leaf.
/// Search depth, unlike this diagnostic depth, counts placements only.
pub fn perft(board: Board, depth: u8) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut moves = board.get_moves();
    if moves == 0 {
        return if board.is_terminal() {
            1
        } else {
            perft(board.board_after_pass(), depth - 1)
        };
    }
    let mut leaves = 0;
    while moves != 0 {
        let bit = moves.isolate_lowest_one();
        moves &= moves - 1;
        leaves += perft(board.board_after_move(bit), depth - 1);
    }
    leaves
}
