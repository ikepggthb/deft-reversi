use crate::eval::Evaluator;
use crate::{eval_search::*, perfect_search};
use crate::evaluator_const::SCORE_MAX;
use crate::mpc::{Selectivity, NO_MPC, N_SELECTIVITY_LV, SELECTIVITY, SELECTIVITY_LV_MAX};
use crate::perfect_search::*;
use crate::{board::*, TranspositionTable};
use crate::move_list::*;

use std::cmp;
use std::collections::VecDeque;

const AI_LEVEL_MAX: usize = 60;

#[derive(Clone, Copy)]
pub enum SolverType {
    Eval(i32, i32), // depth, selectivity_lv
    Perfect(i32),  // selectivity_lv
}

impl SolverType {
    /// ソルバーの説明文字列を生成
    pub fn description(&self) -> String {
        match *self {
            SolverType::Perfect(selectivity_lv) => format!(
                "Perfect solver ({}%)",
                SELECTIVITY[selectivity_lv as usize].percent
            ),
            SolverType::Eval(lv, selectivity_lv) => format!(
                "Eval solver (Lv.{}, {}%)",
                lv, SELECTIVITY[selectivity_lv as usize].percent
            ),
        }
    }
}

const EVAL_SOLVER_SELECTIVITY: i32 = 1;

pub struct SearchEngine {
    pub t_table: TranspositionTable,
    pub origin_board: Board,
    pub eval_func: Evaluator,
    pub selectivity_lv: i32,
    pub status: SearchStats
}

#[derive(Default)]
pub struct SearchStats {
    pub eval_search_node_count: u64,
    pub eval_search_leaf_node_count: u64,
    pub perfect_search_node_count: u64,
    pub perfect_search_leaf_node_count: u64,
    pub start_empty_count: i32,
    pub start_depth: i32
}

impl SearchStats {
    pub fn clear(&mut self) {
        self.eval_search_node_count = 0;
        self.eval_search_leaf_node_count = 0;
        self.perfect_search_node_count = 0;
        self.perfect_search_leaf_node_count = 0;
    }
}

impl SearchEngine {
    pub fn new(evaluator: Evaluator) -> SearchEngine {
        SearchEngine {
            t_table: TranspositionTable::new(),
            origin_board: Board::new(),
            eval_func: evaluator,
            selectivity_lv: NO_MPC,
            status: SearchStats::default()
        }
    }
    pub fn clear_node_count(&mut self) {
        self.status.clear();
    }

    pub fn set_board(&mut self, board: &Board) {
        self.origin_board = board.clone();
        self.clear_node_count();
    }
}

pub struct PutBoard {
    pub board: Board,
    pub put_place: u8,
}

#[inline(always)]
pub fn get_put_boards(board: &Board, legal_moves: u64) -> Vec<PutBoard> {
    let mut put_boards: Vec<PutBoard> = Vec::with_capacity(legal_moves.count_ones() as usize);
    for pos in MoveIterator::new(legal_moves) {
        let mut board = board.clone();
        board.put_piece_fast(pos);
        put_boards.push(PutBoard {
            board,
            put_place: pos.trailing_zeros() as u8,
        })
    }
    put_boards
}

pub struct SolverResult {
    pub best_move: u64,
    pub eval: i32,
    pub solver_type: SolverType,
    pub searched_nodes: u64,
    pub searched_leaf_nodes: u64,
}


pub struct Solver {
    pub search: SearchEngine,
    candidate_boards: VecDeque<PutBoard>,
    pub print_log: String,
}

impl Solver {
    pub fn new(evaluator: Evaluator) -> Self {
        Self {
            search: SearchEngine::new(evaluator),
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

            // println!("{}: i = {}, [{}, {}]", solver.description(self.search.selectivity_lv),n , alpha, beta);
            predict_score = self.search_root(alpha, beta, &solver);            

            if (predict_score <= -SCORE_MAX && alpha <= -SCORE_MAX)
                || (predict_score >= SCORE_MAX && beta >= SCORE_MAX)
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

    fn get_config(&self, n_empties: i32, level: i32) -> SolverType {
        use SolverType::*;

        {
            if level == 0 {
                Eval(0, SELECTIVITY_LV_MAX)
            } else if level <= 10 {
                if n_empties <= 2 * level {
                    Perfect(SELECTIVITY_LV_MAX)
                } else {
                    Eval(level, SELECTIVITY_LV_MAX)
                }
            } else if level <= 12 {
                if n_empties <= 21 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= 24 {
                    Perfect(4)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else if level <= 18 {
                if n_empties <= 21 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= 24 {
                    Perfect(4)
                } else if n_empties <= 27 {
                    Perfect(2)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else if level <= 21 {
                if n_empties <= 24 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= 27 {
                    Perfect(4)
                } else if n_empties <= 30 {
                    Perfect(2)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else if level <= 24 {
                if n_empties <= 24 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= 27 {
                    Perfect(5)
                } else if n_empties <= 30 {
                    Perfect(3)
                } else if n_empties <= 33 {
                    Perfect(1)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            }
            else if level <= 27 {
                if n_empties <= 27 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= 30 {
                    Perfect(4) 
                } else if n_empties <= 33 {
                    Perfect(2)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else if level < 30 {
                if n_empties <= 27 {
                    Perfect(6)
                } else if n_empties <= 30 {
                    Perfect(5)
                } else if n_empties <= 33 {
                    Perfect(3)
                } else if n_empties <= 36 {
                    Perfect(1)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else if level <= 31 {
                if n_empties <= 30 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= 33 {
                    Perfect(4)
                } else if n_empties <= 36 {
                    Perfect(2)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else if level <= 33 {
                if n_empties <= 30 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= 33 {
                    Perfect(5)
                } else if n_empties <= 36 {
                    Perfect(3)
                } else if n_empties <= 39 {
                    Perfect(1)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else if level <= 35 {
                if n_empties <= 30 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= 33 {
                    Perfect(5)
                } else if n_empties <= 36 {
                    Perfect(4)
                } else if n_empties <= 39 {
                    Perfect(2)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else if level < 60 {
                if n_empties <= (level) - 6 {
                    Perfect(SELECTIVITY_LV_MAX)
                } else if n_empties <= (level) - 3 {
                    Perfect(5)
                } else if n_empties <= level {
                    Perfect(4)
                } else if n_empties <= (level) + 3 {
                    Perfect(3)
                } else if n_empties <= (level) + 6 {
                    Perfect(2)
                } else if n_empties <= (level) + 9 {
                    Perfect(1)
                } else {
                    Eval(level, EVAL_SOLVER_SELECTIVITY)
                }
            } else {
                Perfect(SELECTIVITY_LV_MAX)
            }
        }
    }

    pub fn solve(&mut self, board: &Board, lv: i32) -> SolverResult {
        let lv = lv.clamp(1, 60);
        
        self.search.origin_board = board.clone();
        self.search.status.clear();
        self.search.t_table.set_old();

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
                    solver_type: SolverType::Perfect(NO_MPC),
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

        let mut solver_type = self.get_config(board.empties_count(), lv);

        let mut predict_score = self.search.eval_func.clac_features_eval(board);

        // Eval Solver
        self.search.selectivity_lv = if lv > 10 { 1 } else { SELECTIVITY_LV_MAX };
        
        match &mut solver_type {
            SolverType::Eval(lv,selectivity ) => {
                
                // 序盤の評価関数の学習データが良くないので
                if board.move_count() < 20 && *lv > 14 {
                    *lv -= 4;
                    if *selectivity != NO_MPC {
                        *selectivity = 3;
                    }
                }
                let step = 4;
                let start = lv.rem_euclid(step);
                for depth in (start..=*lv).step_by(step as usize) {
                    let init_width: i32 = if depth > 16 { 2 } else { 6 };
                    
                    predict_score =
                        self.aspiration_search(init_width, predict_score, SolverType::Eval(depth, *selectivity));
                }
            },
            SolverType::Perfect(selectivity) => {        
                let selectivity = *selectivity;
                let eval_solver_lv = std::cmp::min(// perfect solver を使用する際は、反復深化でのEvalSolverレベルを制限
                    (board.empties_count() - 7 - (2 - selectivity/2 )).clamp(2, 24),
                    lv,
                );
                let step = 4; let start = eval_solver_lv.rem_euclid(step);
                for depth in (start..=eval_solver_lv).step_by(step as usize) {
                    let init_width: i32 = if depth > 16 { 2 } else { 6 };
                    predict_score =
                        self.aspiration_search(init_width, predict_score, SolverType::Eval(depth, EVAL_SOLVER_SELECTIVITY));
                }

                if eval_solver_lv >= 18 && selectivity > 5 {
                    let init_width = cmp::max(10 - board.empties_count(), 2 + predict_score.rem_euclid(2));
                    predict_score = self.aspiration_search(init_width, predict_score, SolverType::Perfect(selectivity - 4));
                }

                let init_width = cmp::max(10 - board.empties_count(), 2 + predict_score.rem_euclid(2));
                predict_score = self.aspiration_search(init_width, predict_score, SolverType::Perfect(selectivity));
            }
        }

        // Perfect solver

        let best_cand = self.candidate_boards.front().unwrap();
        SolverResult {
            best_move: position_num_to_bit(best_cand.put_place as i32).unwrap(),
            eval: predict_score,
            solver_type,
            searched_nodes: self.search.status.eval_search_node_count
                + self.search.status.perfect_search_node_count,
            searched_leaf_nodes: self.search.status.eval_search_leaf_node_count
                + self.search.status.perfect_search_leaf_node_count,
        }
    }
    
    fn search_root(&mut self, alpha: i32, beta: i32, solver_type: &SolverType) -> i32 {
        match *solver_type {
            SolverType::Eval(_, selectivity) => self.search.selectivity_lv = selectivity,
            SolverType::Perfect(selectivity) => self.search.selectivity_lv = selectivity
        }
        
        fn pvs_search(board: &Board, alpha: i32, beta: i32, search: &mut SearchEngine, solver_type: &SolverType) -> i32{
            match solver_type {
                SolverType::Eval(lv, _) => pvs_eval(board, alpha, beta, *lv - 1, search),
                SolverType::Perfect(_) => pvs_perfect(board, alpha, beta, search)
            }
        }

        fn nws_search(board: &Board, alpha: i32, search: &mut SearchEngine, solver_type: &SolverType) -> i32{
            match solver_type {
                SolverType::Eval(lv, _) => nws_eval(board, alpha, *lv - 1, search),
                SolverType::Perfect(_) => nws_perfect(board, alpha, search)
            }
        }

        let mut alpha = alpha;
        let mut best_cand_index = 0;
        let mut candidate_iter = self.candidate_boards.iter();

        // first move
        let primary_board = candidate_iter.next().unwrap();
        let mut best_score = -pvs_search(&primary_board.board, -beta, -alpha, &mut self.search, solver_type);
        
        alpha = cmp::max(alpha, best_score);
        if best_score >= beta {
            return best_score;
        }

        // other move
        for (i, candidate) in candidate_iter.enumerate() {
            let candidate_index = i + 1;
            let mut score = -nws_search(&candidate.board, -alpha - 1, &mut self.search, solver_type);
            
            if score >= beta {
                self.candidate_boards.swap(0, candidate_index);
                return score;
            }
            if score > alpha {
                score = -pvs_search(&candidate.board, -beta, -alpha, &mut self.search, solver_type);
                
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
