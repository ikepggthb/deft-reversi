//! Reference edge-pattern stability, full axes and fixed-point contact.
use crate::Board;

/// Reference NWS policy: expensive stability detection only near a fail-low.
/// The threshold test is inlined; most callers fail it.
#[inline(always)]
pub(crate) fn null_window_upper(board: Board, alpha: i32) -> Option<i32> {
    const THRESHOLD: [i32; 64] = [
        99, 99, 99, 99, 6, 8, 10, 12, 14, 16, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38, 40, 42, 44,
        46, 48, 48, 50, 50, 52, 52, 54, 54, 56, 56, 58, 58, 60, 60, 62, 62, 64, 64, 64, 64, 64, 64,
        64, 64, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    ];
    if alpha < THRESHOLD[usize::from(board.empty_count())] {
        return None;
    }
    stability_upper(board, alpha)
}
#[inline(never)]
fn stability_upper(board: Board, alpha: i32) -> Option<i32> {
    let full = full_axes(board.player_bits() | board.opponent_bits());
    let upper =
        64 - 2 * stable(board.opponent_bits(), board.player_bits(), &full).count_ones() as i32;
    (upper <= alpha).then_some(upper)
}
#[cfg(test)]
pub(crate) fn score_bounds(board: Board) -> (i32, i32) {
    let full = full_axes(board.player_bits() | board.opponent_bits());
    let player = stable(board.player_bits(), board.opponent_bits(), &full).count_ones() as i32;
    let opponent = stable(board.opponent_bits(), board.player_bits(), &full).count_ones() as i32;
    (2 * player - 64, 64 - 2 * opponent)
}
fn stable(bits: u64, opponent: u64, full: &[u64; 4]) -> u64 {
    let mut result = super::edge_stability::get_stable_edge(bits, opponent);
    let central = bits & 0x007e7e7e7e7e7e00;
    result |= full[0] & full[1] & full[2] & full[3] & central;
    loop {
        let horizontal = (result >> 1) | (result << 1) | full[0];
        let vertical = (result >> 8) | (result << 8) | full[1];
        let diagonal7 = (result >> 7) | (result << 7) | full[2];
        let diagonal9 = (result >> 9) | (result << 9) | full[3];
        let next = result | (horizontal & vertical & diagonal7 & diagonal9 & central);
        if next == result {
            return result;
        }
        result = next;
    }
}
// Full-line propagation follows the reference engine's board/stability module.
fn full_axes(disc: u64) -> [u64; 4] {
    let mut horizontal = disc;
    horizontal &= horizontal >> 1;
    horizontal &= horizontal >> 2;
    horizontal &= horizontal >> 4;
    let mut vertical = disc;
    vertical &= vertical.rotate_right(8);
    vertical &= vertical.rotate_right(16);
    vertical &= vertical.rotate_right(32);
    let (mut left7, mut right7, mut left9, mut right9) = (disc, disc, disc, disc);
    left7 &= 0xff01010101010101 | (left7 >> 7);
    right7 &= 0x80808080808080ff | (right7 << 7);
    left7 &= 0xffff030303030303 | (left7 >> 14);
    right7 &= 0xc0c0c0c0c0c0ffff | (right7 << 14);
    left7 &= 0xffffffff0f0f0f0f | (left7 >> 28);
    right7 &= 0xf0f0f0f0ffffffff | (right7 << 28);
    left9 &= 0xff80808080808080 | (left9 >> 9);
    right9 &= 0x01010101010101ff | (right9 << 9);
    left9 &= 0xffffc0c0c0c0c0c0 | (left9 >> 18);
    right9 &= 0x030303030303ffff | (right9 << 18);
    [
        (horizontal & 0x0101010101010101) * 0xff,
        vertical,
        left7 & right7,
        left9 & right9 & (0x0f0f0f0ff0f0f0f0 | (left9 >> 36) | (right9 << 36)),
    ]
}

#[cfg(test)]
mod tests {
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
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            best = best.max(-brute(board.board_after_move(bit)));
        }
        best
    }
    #[test]
    fn stability_bounds_contain_exhaustive_result() {
        let mut random = 0x12345678abcdef00u64;
        for _ in 0..128 {
            let mut empty = 0u64;
            while empty.count_ones() < 4 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                empty |= 1u64 << (random % 64);
            }
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let player = random & !empty;
            let board = Board::from_bits(player, !player & !empty).unwrap();
            let (lower, upper) = score_bounds(board);
            let expected = brute(board);
            assert!(
                lower <= expected && expected <= upper,
                "lower={lower} score={expected} upper={upper} board={board:?}"
            );
        }
    }
}
