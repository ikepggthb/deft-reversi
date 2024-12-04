use crate::board::*;
use crate::perfect_search::*;
use crate::eval_search::*;
use crate::search::*;
use crate::t_table::*;
use crate::eval::*;

pub struct SolverResult {
    pub best_move: u64,
    pub eval: i32,
    pub solver_type: SolverType,
    pub selectivity_lv: i32,
    pub searched_nodes: u64,
    pub searched_leaf_nodes: u64
}

pub struct Solver {
    search: Search,
    pub print_log: bool,
    pub ai_level: i32,
    child_boards:  Vec<PutBoard>,
}
pub enum SolverErr {
    NoMove,
    FailHi(i32)
}

#[derive(PartialEq)]

#[derive(Debug)]
pub enum SolverType {
    PerfectSolver,
    EvalSolver,
    WinningSolver
}

use SolverType::*;

#[derive(PartialEq, Debug)]
pub struct SolverConfig {
    solver_type: SolverType,
    selectivity_lv: i32,
    eval_solver_level: i32,
}

const SCORE_INF: i32 = i8::MAX as i32;
const MOVE_ORDERING_EVAL_LEVEL: i32 = 8;

impl Solver {
    pub fn new(evaluator: Evaluator) -> Self {
        Solver {
            search: Search::new(evaluator),
            child_boards: Vec::new(),
            print_log: false,
            ai_level: 0
        }        
    }

    pub fn set_ai_level(&mut self, lv: i32) {
        self.ai_level = lv;
    }
    pub fn get_solve_config(&self, level: i32, n_empties: i32) -> SolverConfig
    {
        let (solver_type, selectivity_lv, eval_solver_level) =
        match level {
            1..=10 => {
                if n_empties <= level*2 {
                    (PerfectSolver, 0, 0)
                } else {
                    (EvalSolver, 0, level)
                }
            },
            11..=12 => {
                match n_empties {
                    44..=60 => (EvalSolver, 0, 10),

                    21..=22 => (PerfectSolver, 3, 0),
                    0..=20  => (PerfectSolver, 0, 0),
                    _ => (EvalSolver, 4, level),
                }
            },
            13..=14=> {
                match n_empties {
                    44..=60 => (EvalSolver, 1, 12),

                    21..=22 => (PerfectSolver, 2, 0),
                    0..=20 => (PerfectSolver, 0, 0),
                    _ => (EvalSolver, 4, level),
                }        
            },
            15..=16 => {
                match n_empties {
                    55..=60 => (EvalSolver, 1, level - 4), 
                    44..=54 => (EvalSolver, 1, level - 3),
                    
                    24 => (PerfectSolver, 3, 0),
                    23 => (PerfectSolver, 3, 0),
                    22 => (PerfectSolver, 1, 0),
                    21 => (PerfectSolver, 1, 0),
                    0..=20 => (PerfectSolver, 0, 0),
                    _ => (EvalSolver, 4, level),
                }
            },
            17..=18 => {
                match n_empties {
                    55..=60 => (EvalSolver, 1, level - 4), 
                    44..=54 => (EvalSolver, 1, level - 3),
                    
                    // 25..=26 => (PerfectSolver, 3, 0),
                    23..=24 => (PerfectSolver, 2, 0),
                    0..=22 => (PerfectSolver, 0, 0),
                    _ => (EvalSolver, 4, level),
                }
            },
            19..=24 => {
                match n_empties {
                    55..=60 => (EvalSolver, 1, level - 5),                    
                    44..=54 => (EvalSolver, 1, level - 4),
                    
                    25..=26 => (PerfectSolver, 3, 0),
                    23..=24 => (PerfectSolver, 1, 0),
                    0..=22 => (PerfectSolver, 0, 0),
                    _ => (EvalSolver, 4, level),
                }
            },
            25..=30 => {
                match n_empties {
                    55..=60 => (EvalSolver, 1, level - 5),                    
                    44..=54 => (EvalSolver, 1, level - 4),

                    27..=28 => (PerfectSolver, 3, 0),                    
                    25..=26 => (PerfectSolver, 2, 0),
                    23..=24 => (PerfectSolver, 1, 0),
                    0..=22 => (PerfectSolver, 0, 0),
                    _ => (EvalSolver, 4, level),
                }
            },
            31.. => {
                 (PerfectSolver, 0, 0)
            },
            _ => panic!()
        };

        SolverConfig{
            solver_type,
            selectivity_lv,
            eval_solver_level
        }
    }

    fn select_and_run_solver(&mut self, solve_config: &SolverConfig, alpha: i32, beta: i32) -> Result<SolverResult, SolverErr> {
        println!("solver config: {:?}", solve_config);
        self.search.selectivity_lv = solve_config.selectivity_lv;
        match solve_config.solver_type {
            PerfectSolver => self.perfect_solver(solve_config.selectivity_lv, alpha, beta),
            EvalSolver => self.eval_solver(solve_config.eval_solver_level, solve_config.selectivity_lv, alpha, beta),
            WinningSolver => self.winning_solver(),
        }
    }

    fn aspiration_search(&mut self, predict_score: i32, solve_config: &SolverConfig) -> Result<SolverResult, SolverErr> {

        if solve_config.solver_type == EvalSolver && solve_config.eval_solver_level < 8 {
            return self.select_and_run_solver(solve_config, -SCORE_INF, SCORE_INF);
        }

        let init_width: i32 = if solve_config.solver_type == EvalSolver {
            let tmp =  16 - solve_config.eval_solver_level;
            if tmp < 2 {
                2
            } else {
                tmp
            }
        } else {
            let mut tmp = 10 - self.search.origin_board.empties_count();
            if tmp < 1 { tmp = 1;}
            if predict_score % 2 == 1 {tmp += 1;}
            tmp
        };

        let mut score = predict_score;

        let mut left_width = init_width;
        let mut right_width = init_width;
        loop {
            let alpha = score - left_width; 
            let beta = score + right_width;
            println!("aspiration window [{}, {}]", alpha, beta);
            let result = 
            match self.select_and_run_solver(solve_config, alpha, beta) {
                Ok(result) => {
                    if result.eval <= alpha {
                        left_width *= 2;
                        // if left_width < 4 {left_width = 4};
                        // right_width = 0;
                        result.eval - 2
                    } else {
                        return Ok(result);
                    }
                },
                Err(e) => {
                    match e {
                        SolverErr::NoMove => {
                            return Err(SolverErr::NoMove);
                        },
                        SolverErr::FailHi(score) => {
                            right_width *= 2;
                            score + 2
                        }
                    }
                }
            };
            score = result;
        }

    }

    pub fn solve_no_iter(&mut self, board: &Board) -> Result<SolverResult, SolverErr> {
        let legal_moves = board.put_able();
        if legal_moves == 0 {
            let mut pass_board = board.clone();
            pass_board.swap();
            if pass_board.put_able() == 0 {
                return Ok(SolverResult { 
                    best_move: 0,
                    eval: solve_score(board),
                    solver_type: PerfectSolver,
                    selectivity_lv: 0,
                    searched_nodes: 0,
                    searched_leaf_nodes: 0
                })
            } else {
                let result = self.solve_no_iter(&pass_board);
                if let Ok(mut r) = result {
                    r.best_move = 0;
                    r.eval = -r.eval;
                    return Ok(r);
                }else {
                    return Err(SolverErr::NoMove);
                }
            }
        }

        self.search.set_board(board);
        self.search.clear_node_count();
        
        if self.print_log {
            println!("depth: {}", board.empties_count());
            board.print_board();
            println!("move_ordering....");
        };

        self.child_boards =
            if board.empties_count() < 12 || self.ai_level < 6{
                move_ordering_ffs(board, legal_moves,  &mut self.search)
            } else {
                move_ordering_eval(board, legal_moves, 3,  &mut self.search)
            };

          
        let solve_config = self.get_solve_config(self.ai_level, board.empties_count());
        let result = self.select_and_run_solver(&solve_config, -SCORE_INF, SCORE_INF)?;
        
        if self.print_log {
            let searched_nodes = self.search.perfect_search_node_count + self.search.eval_search_node_count;
            let searched_leaf_nodes = self.search.perfect_search_leaf_node_count + self.search.eval_search_leaf_node_count;
            println!("best move: {}, score: {}{}", position_bit_to_str(result.best_move).unwrap(), if result.eval > 0 {"+"} else {""}, result.eval);
            println!("searched nodes: {}\nsearched leaf nodes: {}", searched_nodes, searched_leaf_nodes);
        }

        Ok(result)
    }

    pub fn solve(&mut self, board: &Board) -> Result<SolverResult, SolverErr> {
        let legal_moves = board.put_able();
        if legal_moves == 0 {
            let mut pass_board = board.clone();
            pass_board.swap();
            if pass_board.put_able() == 0 {
                return Ok(SolverResult { 
                    best_move: 0,
                    eval: solve_score(board),
                    solver_type: PerfectSolver,
                    selectivity_lv: 0,
                    searched_nodes: 0,
                    searched_leaf_nodes: 0
                })
            } else {
                let result = self.solve(&pass_board);
                if let Ok(mut r) = result {
                    r.best_move = 0;
                    r.eval = -r.eval;
                    return Ok(r);
                }else {
                    return Err(SolverErr::NoMove);
                }
            }
            return Err(SolverErr::NoMove)
        }

        self.search.set_board(board);
        self.search.clear_node_count();
        
        if self.print_log {
            println!("depth: {}", board.empties_count());
            board.print_board();
            println!("move_ordering....");
        };

        self.child_boards =
            if board.empties_count() < 12 || self.ai_level < 6{
                move_ordering_ffs(board, legal_moves,  &mut self.search)
            } else {
                move_ordering_eval(board, legal_moves, 3,  &mut self.search)
            };
          
        let original_solve_config = self.get_solve_config(self.ai_level, board.empties_count());
        let mut prev_solve_config = SolverConfig{solver_type: EvalSolver, selectivity_lv: 0, eval_solver_level: 0}; // dummy

        let mut predict_score: Option<i32> = None;
        for i in (8..=self.ai_level).step_by(2) {
            let solve_config = self.get_solve_config(i, board.empties_count());
            if original_solve_config == solve_config {
                break;
            }
            if solve_config == prev_solve_config {
                continue;
            }

            let result = if solve_config.solver_type == PerfectSolver {
                match predict_score {
                Some(score) => self.aspiration_search(score, &solve_config),
                None => self.select_and_run_solver(&solve_config, -SCORE_INF, SCORE_INF)
                }
            }  else {
                self.select_and_run_solver(&solve_config, -SCORE_INF, SCORE_INF)
            }?;
            predict_score = Some(result.eval);

            // PerfectSolverは1度しか実行しない。
            if solve_config.solver_type == PerfectSolver {
                break;
            }

            prev_solve_config = solve_config;
        }

        let result = match predict_score {
            Some(score) => self.aspiration_search(score, &original_solve_config),
            None => self.select_and_run_solver(&original_solve_config, -SCORE_INF, SCORE_INF)
        } ?;
        

        if self.print_log {
            let searched_nodes = self.search.perfect_search_node_count + self.search.eval_search_node_count;
            let searched_leaf_nodes = self.search.perfect_search_leaf_node_count + self.search.eval_search_leaf_node_count;
            println!("best move: {}, score: {}{}", position_bit_to_str(result.best_move).unwrap(), if result.eval > 0 {"+"} else {""}, result.eval);
            println!("searched nodes: {}\nsearched leaf nodes: {}", searched_nodes, searched_leaf_nodes);
        }

        Ok(result)
    }

    pub fn perfect_solver(&mut self, selectivity_lv: i32, alpha: i32, beta: i32) -> Result<SolverResult, SolverErr>
    {
        // [alpha, beta]
        let mut alpha = alpha;

        let mut put_place_best_score: u8 ;
        
        let mut index_put_place_best_score = 0;
        let mut put_boards_iter = self.child_boards.iter_mut();
        let first_child_board = put_boards_iter.next().unwrap();
        alpha = -pvs_perfect(&first_child_board.board, -beta, -alpha, &mut self.search);
        put_place_best_score = first_child_board.put_place;
        if self.print_log { 
            println!("put: {}, nega scout score: {}",position_bit_to_str(1 << put_place_best_score).unwrap(), alpha);
        };

        for (i, put_board) in put_boards_iter.enumerate() {
            let current_put_board = &put_board.board;
            let put_place = put_board.put_place;
            let mut score = -nws_perfect(current_put_board, -alpha - 1, &mut self.search);
            if score >= beta {
                return Err(SolverErr::FailHi(score));
            }
            if score > alpha {
                if self.print_log { 
                    println!(" put: {}, null window score: {} => reserch [{},{}]",position_bit_to_str(1 << put_place).unwrap(), score, alpha, beta);
                }
                if score >= beta {
                    return Err(SolverErr::FailHi(score));
                }
                score = -pvs_perfect(current_put_board, -beta, -alpha, &mut self.search);
                if score > alpha {
                    alpha = score;
                    put_place_best_score = put_place;
                    index_put_place_best_score = i;
                }
            }
            if self.print_log { 
                println!("put: {}, nega scout score: {}",position_bit_to_str(1 << put_place).unwrap(), score);
            }
        }

        if index_put_place_best_score > 0 {
            self.child_boards.swap(0, index_put_place_best_score);
        }

        let result = SolverResult{
            best_move: 1 << put_place_best_score,
            eval: alpha,
            solver_type: PerfectSolver,
            selectivity_lv: selectivity_lv,
            searched_nodes: self.search.perfect_search_node_count + self.search.eval_search_node_count,
            searched_leaf_nodes : self.search.perfect_search_leaf_node_count + self.search.eval_search_leaf_node_count
        };

        Ok(result)
    }

    pub fn eval_solver(&mut self, lv: i32, selectivity_lv: i32, alpha: i32, beta: i32) -> Result<SolverResult, SolverErr>
    {
        let mut alpha = alpha;
        let mut put_place_best_score ;
        
        let mut index_put_place_best_score = 0;
        let mut put_boards_iter = self.child_boards.iter();
        let first_child_board = put_boards_iter.next().unwrap();
        alpha = -pvs_eval(&first_child_board.board, -beta, -alpha, lv - 1, &mut self.search);
        put_place_best_score = first_child_board.put_place;
        if self.print_log { 
            println!("put: {}, nega scout score: {}",position_bit_to_str(1 << put_place_best_score).unwrap(), alpha);
        };

        for (i, put_board) in put_boards_iter.enumerate() {
            let current_put_board = &put_board.board;
            let put_place = put_board.put_place;
            let mut score = -nws_eval(current_put_board, -alpha - 1, lv - 1, &mut self.search);
            if score > alpha {
                if self.print_log { 
                    println!(" put: {}, null window score: {} => reserch [{},{}]",position_bit_to_str(1 << put_place).unwrap(), score, alpha, beta);
                }
                score = -pvs_eval(current_put_board, -beta, -alpha, lv - 1, &mut self.search);
                if score > alpha {
                    alpha = score;
                    put_place_best_score = put_place;
                    index_put_place_best_score = i;
                }
            }
            if self.print_log { 
                println!("put: {}, nega scout score: {}",position_bit_to_str(1 << put_place).unwrap(), score);
            }
        }

        if index_put_place_best_score > 0 {
            self.child_boards.swap(0, index_put_place_best_score);
        }

        let result = SolverResult{
            best_move: 1 << put_place_best_score,
            eval: alpha,
            solver_type: EvalSolver,
            selectivity_lv,
            searched_nodes: self.search.perfect_search_node_count + self.search.eval_search_node_count,
            searched_leaf_nodes : self.search.perfect_search_leaf_node_count + self.search.eval_search_leaf_node_count
        };

        Ok(result)
    }

    pub fn winning_solver(&mut self) -> Result<SolverResult, SolverErr>
    {
        // [alpha, beta] = [0, 1]
        let mut put_place_best_score = 0;
        let mut eval = -1;
        let beta = 1;
        let mut draw_or_lose_board_index: Vec<usize> = Vec::new();

        for (i, put_board) in  self.child_boards.iter_mut().enumerate() {
            let current_put_board = &mut put_board.board;
            let put_place = put_board.put_place;
            let score = -nws_perfect(current_put_board, -beta,&mut self.search);
            if score > 0 {
                if self.print_log { 
                    println!(" put: {}, Win",position_bit_to_str(1 << put_place).unwrap());
                }
                if eval <= 0 {
                    put_place_best_score = put_place;
                    eval = 1
                };
                break;
            } else if score < 0 {
                if self.print_log { 
                    println!(" put: {}, Lose",position_bit_to_str(1 << put_place).unwrap());
                }
            } else {
                draw_or_lose_board_index.push(i);
                if eval < 0 {
                    put_place_best_score = put_place;
                    eval = 0
                };
                if self.print_log { 
                    println!(" put: {}, Draw or Lose", position_bit_to_str(1 << put_place).unwrap());
                }
            }
        }

        if eval == 0 {
            // [alpha, beta] = [-1, 0]
            let beta = 0;

            for &i in draw_or_lose_board_index.iter(){
                let put_board = &mut self.child_boards[i];
                let current_put_board = &mut put_board.board;
                let put_place = put_board.put_place;
                let score = -nws_perfect(current_put_board, -beta,&mut self.search);
                if score == 0 {
                    if self.print_log { 
                        println!(" put: {}, Draw", position_bit_to_str(1 << put_place).unwrap());
                    }
                    if eval < 0 {
                        put_place_best_score = put_place;
                        eval = 0
                    };
                    break;

                } else if score < 0 {
                    if self.print_log { 
                        println!(" put: {}, Lose",position_bit_to_str(1 << put_place).unwrap());
                    }
                    eval = -1;
                } else {
                    eprintln!("Error ocurred in winning_solver");
                    panic!()
                }
            }   
        }
        if eval == -1 {
            put_place_best_score = self.child_boards[0].put_place;
        }

        let result = SolverResult{
            best_move: 1 << put_place_best_score,
            eval: eval,
            solver_type: WinningSolver,
            selectivity_lv: 0,
            searched_nodes: self.search.perfect_search_node_count + self.search.eval_search_node_count,
            searched_leaf_nodes : self.search.perfect_search_leaf_node_count + self.search.eval_search_leaf_node_count
        };

        Ok(result)
    }
}
