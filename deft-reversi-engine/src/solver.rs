use crate::eval::Evaluator;
use crate::eval_search::*;
use crate::evaluator_const::SCORE_MAX;
use crate::mpc::{NO_MPC, N_SELECTIVITY_LV, SELECTIVITY, SELECTIVITY_LV_MAX};
use crate::perfect_search::*;
use crate::search::*;
use crate::{board::*, TranspositionTable};

use std::cmp;
use std::collections::VecDeque;

const AI_LEVEL_MAX: usize = 60;
pub enum SolverType {
    Eval(i32), // depth
    Perfect,
}

impl SolverType {
    /// ソルバーの説明文字列を生成
    pub fn description(&self, selectivity_lv: i32) -> String {
        match *self {
            SolverType::Perfect => format!(
                "Perfect solver ({}%)",
                SELECTIVITY[selectivity_lv as usize].percent
            ),
            SolverType::Eval(lv) => format!(
                "Eval solver (Lv.{}, {}%)",
                lv, SELECTIVITY[selectivity_lv as usize].percent
            ),
        }
    }
}

// AI_RULES: AI レベルごとの探索ルール
// - [i32; N_SELECTIVITY_LV]: Selectivity LvごとのPerfect Solver開始条件（空きマス数）
//   -> 各インデックスがSelectivity Lvに対応。
//
// 例: [30, 29, 28, 27, 26, 25, 24]
// - 空きマスが"30"個以下でPerfect Solver(Selectivity Lv.0 (68%))を開始
// - ...
// - 空きマスが"25"個以下でPerfect Solver(Selectivity Lv.5 (99%))を開始
// - 空きマスが"24"個以下でPerfect Solver(Selectivity Lv.6 (100%))を開始
pub const AI_RULES: [[i32; N_SELECTIVITY_LV]; AI_LEVEL_MAX + 1] = [
    [0, 0, 0, 0, 0, 0, 1], // Lv. 0
    [0, 0, 0, 0, 0, 0, 2], // Lv. 1
    [0, 0, 0, 0, 0, 0, 4],
    [0, 0, 0, 0, 0, 0, 6],
    [0, 0, 0, 0, 0, 0, 8],
    [0, 0, 0, 0, 0, 0, 10],
    [0, 0, 0, 0, 0, 0, 12],
    [0, 0, 0, 0, 0, 0, 14],
    [0, 0, 0, 0, 0, 0, 16],
    [0, 0, 0, 0, 0, 0, 18],
    [0, 0, 0, 0, 0, 0, 20], // 10
    [0, 0, 24, 0, 22, 0, 21],
    [0, 0, 24, 0, 22, 0, 21],
    [0, 0, 0, 24, 0, 22, 21],
    [0, 0, 0, 24, 0, 22, 21],
    [0, 0, 0, 00, 24, 22, 21], // 15
    [0, 0, 0, 00, 24, 22, 21],
    [0, 0, 26, 0, 24, 0, 23],
    [0, 0, 26, 0, 24, 0, 23],
    [0, 0, 28, 0, 26, 0, 24],
    [0, 0, 28, 0, 26, 0, 24], // 20
    [0, 0, 29, 0, 27, 0, 25],
    [0, 0, 29, 0, 27, 0, 25],
    [0, 0, 30, 0, 28, 0, 26],
    [0, 0, 30, 0, 28, 0, 26],
    [0, 0, 31, 0, 29, 0, 27], // 25
    [0, 0, 31, 0, 29, 0, 27],
    [0, 0, 32, 0, 30, 0, 28],
    [0, 0, 32, 0, 30, 0, 28],
    [0, 0, 0, 32, 0, 30, 28],
    [0, 0, 0, 32, 0, 30, 28], // 30
    [0, 0, 33, 0, 31, 0, 29],
    [0, 0, 33, 0, 31, 0, 29],
    [0, 0, 00, 33, 0, 31, 29],
    [0, 0, 00, 33, 0, 31, 29],
    [0, 0, 34, 0, 32, 0, 30], // 35
    [0, 0, 34, 0, 32, 0, 30],
    [0, 0, 0, 34, 0, 32, 30],
    [0, 0, 0, 34, 0, 32, 30],
    [0, 0, 35, 0, 33, 0, 31],
    [0, 0, 35, 0, 33, 0, 31], // 40
    [0, 0, 36, 0, 34, 0, 32],
    [0, 0, 36, 0, 34, 0, 32],
    [0, 0, 38, 0, 36, 0, 34],
    [0, 0, 38, 0, 36, 0, 34],
    [0, 0, 40, 0, 38, 0, 36], // 45
    [0, 0, 40, 0, 38, 0, 36],
    [0, 0, 42, 0, 40, 0, 38],
    [0, 0, 42, 0, 40, 0, 38],
    [0, 0, 44, 0, 42, 0, 40],
    [0, 50, 48, 46, 44, 42, 40], // 50
    [0, 52, 50, 48, 46, 44, 42],
    [0, 54, 52, 50, 48, 46, 44],
    [0, 56, 54, 52, 50, 48, 46],
    [0, 58, 56, 54, 52, 50, 48],
    [0, 60, 58, 56, 54, 52, 50], // 55
    [0, 0, 60, 58, 56, 54, 52],
    [0, 0, 0, 60, 58, 56, 54],
    [0, 0, 0, 0, 60, 58, 56],
    [0, 0, 0, 0, 0, 60, 58],
    [0, 0, 0, 0, 0, 0, 60], // 60
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
    pub search: Search,
    candidate_boards: VecDeque<PutBoard>,
    pub print_log: String,
}

impl Solver {
    pub fn new(evaluator: Evaluator) -> Self {
        Self {
            search: Search::new(evaluator),
            candidate_boards: VecDeque::new(),
            print_log: String::new(),
        }
    }

    fn aspiration_search(
        &mut self,
        init_width: i32,
        predict_score: i32,
        solver: SolverType,
    ) -> i32 {
        let mut left_width = init_width;
        let mut right_width = init_width;
        let mut predict_score = predict_score;

        let mut n = 0;
        loop {
            n += 1;
            let alpha = cmp::max(predict_score - left_width, -SCORE_MAX);
            let beta = cmp::min(predict_score + right_width, SCORE_MAX);

            #[cfg(debug_assertions)]
            assert!(alpha <= beta);
            predict_score = match solver {
                SolverType::Eval(depth) => self.eval_solver(depth, alpha, beta),
                SolverType::Perfect => self.perfect_solver(alpha, beta),
            };

            if (predict_score == -SCORE_MAX && alpha == -SCORE_MAX)
                || (predict_score == SCORE_MAX && beta == SCORE_MAX)
            {
                break;
            }

            if predict_score >= beta {
                if n % 2 == 1 {
                    right_width += 2;
                    left_width += 0;
                } else {
                    right_width += n * ((n as f64).log2()) as i32 + 2; // right_width += nlog(n) + 2
                    left_width += 2;
                }
            } else if predict_score <= alpha {
                if n % 2 == 1 {
                    left_width += 2;
                    right_width += 0;
                } else {
                    left_width += n * ((n as f64).log2()) as i32 + 2; // left_width += nlog(n) + 2
                    right_width += 2;
                }
            } else {
                break;
            }
        }

        predict_score
    }

    fn get_ai_rules(&self, board: &Board, lv: i32) -> (bool, i32, i32) {
        let n_empties = board.empties_count();
        let thresholds = AI_RULES[lv as usize];
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
            if perfect_solver_use {
                std::cmp::min(// perfect solver を使用する際は、反復深化でのEvalSolverレベルを制限
                    (n_empties - 7 - (2 - selectivity_lv_perfect_search * 2)).max(2),
                    lv,
                )
            } else {
                lv
            }, 
        )
    }

    pub fn solve(&mut self, board: &Board, lv: i32) -> SolverResult {
        let lv = if lv > AI_LEVEL_MAX as i32 {
            AI_LEVEL_MAX as i32
        } else {
            lv
        };
        self.search.set_board(board);
        self.search.clear_node_count();
        let legal_moves = board.moves();

        if legal_moves == 0 {
            let passed_board = {
                let mut new_board = board.clone();
                new_board.swap();
                new_board
            };
            if passed_board.moves() == 0 {
                return SolverResult {
                    best_move: 0,
                    eval: solve_score(board),
                    solver_type: SolverType::Perfect,
                    selectivity_lv: NO_MPC,
                    searched_nodes: 1,
                    searched_leaf_nodes: 1,
                };
            } else {
                let mut r = self.solve(&passed_board, lv);
                r.eval = -r.eval;
                return r;
            }
        }

        self.candidate_boards = get_put_boards(board, legal_moves).into_iter().collect();

        let (perfect_solver_use, selectivity_lv_perfect_search, mut max_depth_eval_solver) =
            self.get_ai_rules(board, lv);

        let mut predict_score = self.search.eval_func.clac_features_eval(board);

        // Eval Solver
        self.search.selectivity_lv = if lv > 10 { 1 } else { SELECTIVITY_LV_MAX };

        // 序盤の評価関数の学習データが良くないので
        if board.move_count() < 20 && max_depth_eval_solver > 14 {
            max_depth_eval_solver -= 4;
            if self.search.selectivity_lv != NO_MPC {
                self.search.selectivity_lv = 3;
            }
        }
        let step = 4;
        let start = max_depth_eval_solver.rem_euclid(step);
        for depth in (start..=max_depth_eval_solver).step_by(step as usize) {
            let init_width: i32 = if depth > 16 { 2 } else { 6 };
            predict_score =
                self.aspiration_search(init_width, predict_score, SolverType::Eval(depth));
        }

        // Perfect solver
        if perfect_solver_use {
            self.search.selectivity_lv = selectivity_lv_perfect_search;
            let init_width = cmp::max(10 - board.empties_count(), 2 + predict_score.rem_euclid(2));
            predict_score = self.aspiration_search(init_width, predict_score, SolverType::Perfect);
        }

        let best_cand = self.candidate_boards.front().unwrap();//first().unwrap();
        self.search.t_table.set_old();
        SolverResult {
            best_move: position_num_to_bit(best_cand.put_place as i32).unwrap(),
            eval: predict_score,
            solver_type: if perfect_solver_use {
                SolverType::Perfect
            } else {
                SolverType::Eval(lv)
            },
            selectivity_lv: self.search.selectivity_lv,
            searched_nodes: self.search.eval_search_node_count
                + self.search.perfect_search_node_count,
            searched_leaf_nodes: self.search.eval_search_leaf_node_count
                + self.search.perfect_search_leaf_node_count,
        }
    }

    fn eval_solver(&mut self, depth: i32, alpha: i32, beta: i32) -> i32 {
        let mut alpha = alpha;

        let mut best_cand_index = 0;
        let mut candidate_iter = self.candidate_boards.iter();

        // first move
        let primary_board = candidate_iter.next().unwrap();
        let mut best_score = -pvs_eval(
            &primary_board.board,
            -beta,
            -alpha,
            depth - 1,
            &mut self.search,
        );
        alpha = cmp::max(alpha, best_score);
        if best_score >= beta {
            return best_score;
        }

        // other move
        for (i, candidate) in candidate_iter.enumerate() {
            let candidate_index = i + 1;
            let mut score = -nws_eval(&candidate.board, -alpha - 1, depth - 1, &mut self.search);
            if score >= beta {
                self.candidate_boards.swap(0, candidate_index);
                return score;
            }
            if score > alpha {
                score = -pvs_eval(&candidate.board, -beta, -alpha, depth - 1, &mut self.search);
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
            let best_cand = self.candidate_boards.remove(best_cand_index).unwrap();
            self.candidate_boards.push_front(best_cand);
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
            let best_cand = self.candidate_boards.remove(best_cand_index).unwrap();
            self.candidate_boards.push_front(best_cand);
        }

        best_score
    }
}
