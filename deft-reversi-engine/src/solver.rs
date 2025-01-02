use std::cmp;

use crate::eval::evaluator_const::SCORE_INF;

use crate::board::*;
use crate::mpc::NO_MPC;
use crate::mpc::N_SELECTIVITY_LV;
use crate::mpc::SELECTIVITY;
use crate::mpc::SELECTIVITY_LV_MAX;
use crate::perfect_search::*;
use crate::eval_search::*;
use crate::search::*;
use crate::eval::Evaluator;


pub enum SolverType {
    Eval(i32), // depth
    Perfect
}

const AI_LEVEL_MAX: usize = 25;


// AI_RULES: AI レベルごとの探索ルール
// - [usize; N_SELECTIVITY_LV]: Selectivity LvごとのPerfect Solver開始条件（空きマス数）
//   -> 各インデックスがSelectivity Lvに対応。
// - usize: 反復深化で使用するEval Solverの最大深さ
//
// 例: ([30, 29, 28, 27, 26, 25, 24], 18)
// - Selectivity Lv = 0: 空きマスが"24"個以下でPerfect Solverを開始
// - Selectivity Lv = 1: 空きマスが"25"個以下でPerfect Solverを開始
// - ...
// - Selectivity Lv = 6: 空きマスが"30"個以下でPerfect Solverを開始
// - Perfect Solverを使用する際、Eval Solverを使った反復深化は深さ"18"まで。
pub const AI_RULES: [([i32; N_SELECTIVITY_LV], usize); AI_LEVEL_MAX + 1] = [
    ([ 0,  0,  0,  0,  0,  0,  1], 0), // lv. 1
    ([ 0,  0,  0,  0,  0,  0,  2], 0),
    ([ 0,  0,  0,  0,  0,  0,  4], 0),
    ([ 0,  0,  0,  0,  0,  0,  6], 0),
    ([ 0,  0,  0,  0,  0,  0,  8], 0),
    ([ 0,  0,  0,  0,  0,  0, 10], 0),
    ([ 0,  0,  0,  0,  0,  0, 12], 0),
    ([ 0,  0,  0,  0,  0,  0, 14], 0),
    ([ 0,  0,  0,  0,  0,  0, 16], 0),
    ([ 0,  0,  0,  0,  0,  0, 18], 0),
    ([ 0,  0,  0,  0,  0,  0, 20], 8),
    ([ 0, 24,  0, 22,  0,  0, 21], 8),
    ([ 0, 24,  0, 22,  0,  0, 21], 10),
    ([ 0,  0, 24,  0, 22,  0, 21], 10),
    ([ 0,  0, 24,  0, 22,  0, 21], 10),
    ([ 0, 26,  0, 24,  0,  0, 22], 10),
    ([ 0, 26,  0, 24,  0,  0, 22], 12),
    ([ 0,  0, 26,  0, 24,  0, 22], 12),
    ([ 0,  0, 26,  0, 24,  0, 22], 12),
    ([ 0, 28,  0, 26,  0, 24, 23], 12),
    ([ 0, 28,  0, 26,  0, 24, 23], 14),
    ([29, 28, 27, 26, 25, 24, 23], 14),
    ([29, 28, 27, 26, 25, 24, 23], 14),
    ([ 0,  0, 28,  0, 26,  0, 24], 14),
    ([30, 29, 28, 27, 26, 25, 24], 16),
    ([ 0,  0,  0,  0,  0,  0, 60], 16)
];

pub struct SolverResult {
    pub best_move: u64,
    pub eval: i32,
    pub solver_type: SolverType,
    pub selectivity_lv: i32,
    pub searched_nodes: u64,
    pub searched_leaf_nodes: u64,
}

pub struct Solver {
    search: Search,
    candidate_boards:  Vec<PutBoard>,
    pub print_log: String,
}

impl Solver {
    pub fn new(evaluator: Evaluator) -> Self {
        Solver {
            search: Search::new(evaluator),
            candidate_boards: Vec::new(),
            print_log: String::new(),
        }
    }

    fn aspiration_search(&mut self, init_width: i32, predict_score: i32, solver: SolverType) -> i32 {
        let mut left_width = init_width;
        let mut right_width = init_width;
        let mut predict_score = predict_score;

        loop {
            let alpha = cmp::min(predict_score - left_width, -SCORE_INF); 
            let beta = cmp::min(predict_score + right_width, SCORE_INF);
            
            
            #[cfg(debug_assertions)]
            assert!(alpha <= beta);
            predict_score = match solver {
                SolverType::Eval(depth) => {
                    self.eval_solver(depth, alpha, beta)
                },
                SolverType::Perfect => {
                    self.perfect_solver(alpha, beta)
                }
            };
            if predict_score >= beta {
                right_width *= 2;
            } else if  predict_score <= alpha {
                left_width *= 2;
            } else {
                break;
            }
        }

        predict_score

    }

    fn get_ai_rules(&self, board: &Board, lv: i32) -> (bool, i32, i32)
    {
        let n_empties = board.empties_count();
        let (thresholds, max_depth) = AI_RULES[lv as usize];
        let mut perfect_solver_use = false;
        let mut selectivity_lv_perfect_search = 0;
        for (i, &e) in thresholds.iter().enumerate() {
            if 0 != e && n_empties <= e {
                selectivity_lv_perfect_search = i as i32;
                perfect_solver_use = true;
            }
        }

        (
            perfect_solver_use,
            selectivity_lv_perfect_search,
            if perfect_solver_use {max_depth as i32} else {lv}
        )
    }

    pub fn solve(&mut self, board: &Board, lv: i32) -> SolverResult {
        let lv = if lv > AI_LEVEL_MAX as i32{ AI_LEVEL_MAX as i32 } else {lv};
        self.search.set_board(board);
        self.search.clear_node_count();
        let legal_moves = board.put_able();

        if legal_moves == 0 {
            let passed_board = {
                let mut new_board = board.clone();
                new_board.swap();
                new_board
            };
            if passed_board.put_able() == 0 {
                return SolverResult { 
                    best_move: 0,
                    eval: solve_score(board),
                    solver_type: SolverType::Perfect,
                    selectivity_lv: 0,
                    searched_nodes: 0,
                    searched_leaf_nodes: 0 
                }
            } else {
                let mut r = self.solve(&passed_board, lv);
                r.eval = -r.eval;
                return  r;
            }
        }
    
        self.candidate_boards =
            if board.empties_count() < 12 || lv < 6{
                move_ordering_ffs(board, legal_moves,  &mut self.search)
            } else {
                move_ordering_eval(board, legal_moves, 3,  &mut self.search)
            };

        let (
            perfect_solver_use,
            selectivity_lv_perfect_search,
            mut max_depth_eval_solver
        ) = self.get_ai_rules(board, lv);
            
        
        let mut predict_score = self.search.eval_func.clac_features_eval(board);    
        
        // Eval Solver
        self.search.selectivity_lv =  if lv > 10 { 0 } else { SELECTIVITY_LV_MAX };

        // 序盤の評価関数の学習データが良くないので
        if board.move_count() < 20 && max_depth_eval_solver > 14 {
            max_depth_eval_solver -= 4;
            if self.search.selectivity_lv != NO_MPC {
                self.search.selectivity_lv = 3;
            }
        }
        let step = 2;
        let start = max_depth_eval_solver.rem_euclid(step);
        for depth in (start..=max_depth_eval_solver).step_by(step as usize) {
            let init_width: i32 = if depth > 16 {4} else {64};
            predict_score = self.aspiration_search(init_width, predict_score,SolverType::Eval(depth) );        
        }

        // Perfect solver
        if perfect_solver_use {
            for selectivity_lv in 1..=selectivity_lv_perfect_search {
                self.search.selectivity_lv = selectivity_lv;
                let init_width = cmp::max(
                    10 - board.empties_count(), 
                    1 + predict_score.rem_euclid(2) );                        
                predict_score = self.aspiration_search(init_width, predict_score,SolverType::Perfect );            
            }
        }
        
        let best_cand = self.candidate_boards.first().unwrap();
        SolverResult { 
            best_move: position_num_to_bit(best_cand.put_place as i32).unwrap(),
            eval: predict_score,
            solver_type: if perfect_solver_use {SolverType::Perfect} else { SolverType::Eval(lv)},
            selectivity_lv: self.search.selectivity_lv,
            searched_nodes: self.search.eval_search_node_count + self.search.perfect_search_node_count,
            searched_leaf_nodes: self.search.eval_search_leaf_node_count + self.search.perfect_search_leaf_node_count 
        }
    }


    fn eval_solver(&mut self, depth: i32, alpha: i32, beta: i32) -> i32
    {
        let mut alpha = alpha;

        let mut best_cand_index = 0;
        let mut candidate_iter = self.candidate_boards.iter();

        // first move
        let primary_board = candidate_iter.next().unwrap();
        let mut best_score = -pvs_eval(&primary_board.board, -beta, -alpha, depth, &mut self.search);
        alpha = cmp::max(alpha, best_score);
        if best_score >= beta {
            return best_score;
        }

        // other move
        for (i, candidate) in candidate_iter.enumerate() {
            let candidate_index = i + 1;
            let mut score = -nws_eval(&candidate.board, -alpha - 1, depth, &mut self.search);
            if score >= beta {
                self.candidate_boards.swap(0, candidate_index);
                 return score;
            }
            if score > alpha {
                score = -pvs_eval(&candidate.board, -beta, -alpha, depth, &mut self.search);
                if score >= beta {
                    self.candidate_boards.swap(0, candidate_index);
                    return score;
                }
                if score > alpha {
                    best_score = score;
                    alpha = score;
                    best_cand_index = candidate_index;
                }
            }
        }

        if best_cand_index > 0 {
            self.candidate_boards.swap(0, best_cand_index);
        }

        best_score
    }

    fn perfect_solver(&mut self, alpha: i32, beta: i32) -> i32 {
        let mut alpha = alpha;
        let mut best_cand_index = 0;
        let mut candidate_iter = self.candidate_boards.iter();

        // first move
        let primary_board = candidate_iter.next().unwrap();
        let mut best_score = -pvs_perfect(&primary_board.board, -beta, -alpha, &mut self.search);
        alpha = cmp::max(alpha, best_score);
        if best_score >= beta {
            return best_score;
        }

        // other move
        for (i, candidate) in candidate_iter.enumerate() {
            let candidate_index = i + 1;
            let mut score = -nws_perfect(&candidate.board, -alpha - 1, &mut self.search);
            if score >= beta {
                self.candidate_boards.swap(0, candidate_index);
                 return score;
            }
            if score > alpha {
                score = -pvs_perfect(&candidate.board, -beta, -alpha, &mut self.search);
                if score >= beta {
                    self.candidate_boards.swap(0, candidate_index);
                    return score;
                }
                if score > alpha {
                    best_score = score;
                    alpha = score;
                    best_cand_index = candidate_index;
                }
            }
        }

        if best_cand_index > 0 {
            self.candidate_boards.swap(0, best_cand_index);
        }

        best_score
    }

}