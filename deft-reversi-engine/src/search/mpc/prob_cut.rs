use crate::board::board::Board;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::eval_search::nws_eval_leaf_no_mpc;
use crate::search::mpc::MpcParams;
use crate::search::search::{EvalSearch, NO_MPC_SELECTIVITY_LV};

const EVAL_SEARCH_MPC_START_DEPTH: i32 = 4;
const FINAL_SEARCH_MPC_START_EMPTIES: i32 = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Selectivity {
    z: f64,
    pub percent: i32,
}

pub const SELECTIVITY_LV_MAX: i32 = NO_MPC_SELECTIVITY_LV;

#[rustfmt::skip]
const SELECTIVITY: [Selectivity; 7] = [
    Selectivity { z: 1.0,     percent: 68 },
    Selectivity { z: 1.20,    percent: 77 },
    Selectivity { z: 1.43953, percent: 85 },
    Selectivity { z: 1.960,   percent: 95 },
    Selectivity { z: 2.326,   percent: 98 },
    Selectivity { z: 2.576,   percent: 99 },
    Selectivity { z: 0.0,     percent: 100 },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbCutResult {
    Cut(i32),
    Fail,
}

#[inline(always)]
pub fn eval_search_mpc(
    board: &Board,
    alpha: i32,
    beta: i32,
    depth: i32,
    search: &mut EvalSearch,
) -> ProbCutResult {
    if depth < EVAL_SEARCH_MPC_START_DEPTH {
        return ProbCutResult::Fail;
    }
    let empties = 64 - (board.player | board.opponent).count_ones() as i32;
    multi_prob_cut(board, alpha, beta, search.mpc.eval_search.params(depth, empties), search)
}

#[inline(always)]
pub fn final_search_mpc(
    board: &Board,
    alpha: i32,
    beta: i32,
    search: &mut EvalSearch,
) -> ProbCutResult {
    let empties = 64 - (board.player | board.opponent).count_ones() as i32;
    if empties < FINAL_SEARCH_MPC_START_EMPTIES {
        return ProbCutResult::Fail;
    }
    multi_prob_cut(board, alpha, beta, search.mpc.final_search.params(empties), search)
}

#[inline(always)]
fn multi_prob_cut(
    board: &Board,
    alpha: i32,
    beta: i32,
    params: MpcParams,
    search: &mut EvalSearch,
) -> ProbCutResult {
    if alpha >= SCORE_MAX {
        return ProbCutResult::Cut(alpha);
    }
    if beta <= -SCORE_MAX {
        return ProbCutResult::Cut(beta);
    }
    if search.selectivity_lv == NO_MPC_SELECTIVITY_LV {
        return ProbCutResult::Fail;
    }

    debug_assert!((0..=SELECTIVITY_LV_MAX).contains(&search.selectivity_lv));

    search.stats.mpc_tries += 1;

    let z = SELECTIVITY[search.selectivity_lv as usize].z;
    let error_allowance = z * params.e_std;

    let upper = ((beta as f64 - params.b + error_allowance) / params.a).ceil() as i32;
    if upper < SCORE_MAX {
        let score = nws_eval_leaf_no_mpc(board, upper - 1, params.lv, search);
        if score >= upper {
            search.stats.mpc_cuts += 1;
            search.stats.mpc_high_cuts += 1;
            return ProbCutResult::Cut(beta);
        }
    }

    let lower = ((alpha as f64 - params.b - error_allowance) / params.a).floor() as i32;
    if lower > -SCORE_MAX {
        let score = nws_eval_leaf_no_mpc(board, lower, params.lv, search);
        if score <= lower {
            search.stats.mpc_cuts += 1;
            search.stats.mpc_low_cuts += 1;
            return ProbCutResult::Cut(alpha);
        }
    }

    ProbCutResult::Fail
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::mpc::MpcConfig;
    use crate::t_table::TranspositionTable;
    use std::sync::Arc;

    #[test]
    fn eval_search_mpc_is_disabled_by_default_selectivity() {
        let board = Board::new();
        let evaluator = Arc::new(Evaluator::default());
        let mpc = Arc::new(MpcConfig::default());
        let tt = Arc::new(TranspositionTable::new());
        let mut stats = crate::search::search::SearchStats::default();
        let mut search = EvalSearch::new(evaluator, mpc, tt, &mut stats);

        assert_eq!(
            eval_search_mpc(&board, -1, 1, 10, &mut search),
            ProbCutResult::Fail
        );
        assert_eq!(search.stats.mpc_tries, 0);
    }

    #[test]
    fn multi_prob_cut_can_cut_high() {
        let board = Board::new();
        let evaluator = Arc::new(Evaluator::default());
        let mpc = Arc::new(MpcConfig::default());
        let tt = Arc::new(TranspositionTable::new());
        let mut stats = crate::search::search::SearchStats::default();
        let mut search = EvalSearch::new(evaluator, mpc, tt, &mut stats);
        search.selectivity_lv = 0;

        assert_eq!(
            multi_prob_cut(
                &board,
                -1,
                0,
                MpcParams {
                    lv: 0,
                    a: 1.0,
                    b: 0.0,
                    e_std: 0.0,
                },
                &mut search
            ),
            ProbCutResult::Cut(0)
        );
        assert_eq!(search.stats.mpc_tries, 1);
        assert_eq!(search.stats.mpc_cuts, 1);
        assert_eq!(search.stats.mpc_high_cuts, 1);
    }

    #[test]
    fn multi_prob_cut_can_cut_low() {
        let board = Board::new();
        let evaluator = Arc::new(Evaluator::default());
        let mpc = Arc::new(MpcConfig::default());
        let tt = Arc::new(TranspositionTable::new());
        let mut stats = crate::search::search::SearchStats::default();
        let mut search = EvalSearch::new(evaluator, mpc, tt, &mut stats);
        search.selectivity_lv = 0;

        assert_eq!(
            multi_prob_cut(
                &board,
                0,
                1,
                MpcParams {
                    lv: 0,
                    a: 1.0,
                    b: 0.0,
                    e_std: 0.0,
                },
                &mut search
            ),
            ProbCutResult::Cut(0)
        );
        assert_eq!(search.stats.mpc_tries, 1);
        assert_eq!(search.stats.mpc_cuts, 1);
        assert_eq!(search.stats.mpc_low_cuts, 1);
    }
}
