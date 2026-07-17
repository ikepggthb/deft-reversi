use crate::board::{board::Board, stability::get_stability};
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::SearchContext;

const NWS_STABILITY_THRESHOLD: [i32; 64] = [
    99, 99, 99, 99, 6, 8, 10, 12, 14, 16, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38, 40, 42, 44, 46,
    48, 48, 50, 50, 52, 52, 54, 54, 56, 56, 58, 58, 60, 60, 62, 62, 64, 64, 64, 64, 64, 64, 64, 64,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

const PVS_STABILITY_THRESHOLD: [i32; 64] = [
    99, 99, 99, 99, -2, 0, 2, 4, 6, 8, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38, 40,
    40, 42, 42, 44, 44, 46, 46, 48, 48, 50, 50, 52, 52, 54, 54, 56, 56, 58, 58, 60, 60, 62, 62, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

pub fn stability_cut_nws(
    board: &Board,
    alpha: i32,
    n_empties: i32,
    search: &mut SearchContext,
) -> Option<i32> {
    if alpha >= NWS_STABILITY_THRESHOLD[n_empties as usize] {
        search.stats.stability_tries += 1;
        let score = SCORE_MAX - 2 * get_stability(board.opponent, board.player);
        if score <= alpha {
            search.stats.stability_cuts += 1;
            return Some(score);
        }
    }
    None
}

pub fn stability_cut_pvs(
    board: &Board,
    alpha: i32,
    beta: &mut i32,
    n_empties: i32,
    search: &mut SearchContext,
) -> Option<i32> {
    if *beta >= PVS_STABILITY_THRESHOLD[n_empties as usize] {
        search.stats.stability_tries += 1;
        let upper = SCORE_MAX - 2 * get_stability(board.opponent, board.player);
        if upper <= alpha {
            search.stats.stability_cuts += 1;
            return Some(upper);
        }
        if upper < *beta {
            *beta = upper;
        }
    }
    None
}
