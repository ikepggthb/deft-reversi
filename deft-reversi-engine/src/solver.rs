use crate::board::*;
use crate::perfect_search::*;
use crate::eval_search::*;
use crate::search::*;
use crate::t_table::*;
use crate::eval::*;

pub struct SolverResult {
    pub best_move: u64,
    pub eval: i32,
    pub node_count: u64,
    pub leaf_node_count: u64
}

pub enum SolverErr {
    NoMove,
}

const SCORE_INF: i32 = i8::MAX as i32;
const MOVE_ORDERING_EVAL_LEVEL: i32 = 8;

/// オセロの盤面に対する完全な探索を行い、最適な手とその評価値を求める。
///
/// 終盤の探索で使用される。
/// この関数は、Principal Variation Search (PVS)とNull Window Search (NWS)のアルゴリズムを使用して、
/// 与えられたオセロの盤面に対して最も有利な手を決定します。
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `print_log` - trueの場合、探索の進行状況と結果をコンソールに出力します。
///
/// # 戻り値
/// `Result<SolverResult, SolverErr>` 型。成功した場合、`SolverResult`オブジェクトが含まれ、
/// 最適な手とその評価値、探索したノード数、葉ノード数を含みます。
/// 合法手が存在しない場合は、`SolverErr::NoMove`エラーが返されます。
///
/// # 例
/// ```
/// let board = Board::new(); // 初期盤面の生成
/// match perfect_solver(&board, true) {
///     Ok(result) => println!("Best move: {}, Score: {}", result.best_move, result.eval),
///     Err(SolverErr::NoMove) => println!("No legal moves available."),
///     _ => println!("An error occurred during the search.")
/// }
/// ```
///
pub fn perfect_solver(board: &Board, print_log: bool, selectivity_lv: i32, t_table: &mut TranspositionTable, evaluator: &mut Evaluator) -> Result<SolverResult, SolverErr>
{
    let legal_moves = board.put_able();
    if legal_moves == 0 {
        return Err(SolverErr::NoMove)
    }
    let mut search = Search::new(board, selectivity_lv, t_table, evaluator);
    
    if print_log {
        println!("my_turn: {}", if board.next_turn == Board::BLACK {"Black"} else {"White"});
        println!("depth: {}", board.empties_count());
        board.print_board();
    };


    if print_log {print!("move_ordering....");};

    if board.empties_count() > 8 {
        pvs_eval(board, -SCORE_INF ,SCORE_INF, 6, &mut search);
    }
    if board.empties_count() > 10 {
        pvs_eval(board, -SCORE_INF ,SCORE_INF, 8, &mut search);
    }
    if board.empties_count() > 12 {
        pvs_eval(board, -SCORE_INF ,SCORE_INF, 10, &mut search);
    }


    if board.empties_count() > 20 {
        let main_selectivity_lv = selectivity_lv;
        search.selectivity_lv = 5;
        pvs_eval(board, -SCORE_INF, SCORE_INF,16, &mut search);
        search.selectivity_lv = main_selectivity_lv;
    }
    if search.selectivity_lv < 3 {
        let main_selectivity_lv = selectivity_lv;
        search.selectivity_lv = 5;
        pvs_perfect(board, -SCORE_INF, SCORE_INF, &mut search);
        search.selectivity_lv = main_selectivity_lv;
    }

    let mut put_boards = 
        if board.empties_count() < MOVE_ORDERING_EVAL_LEVEL + 2 {
            get_put_boards(board, legal_moves)
        } else {
            move_ordering_eval(board, legal_moves, 8,  &mut search)
        };
    if print_log {println!("OK");};

    let mut alpha = -SCORE_INF;
    let beta = SCORE_INF;
    let mut put_place_best_score ;
    
    let mut put_boards_iter = put_boards.iter_mut();
    let first_child_board = put_boards_iter.next().unwrap();
    alpha = -pvs_perfect(&first_child_board.board, -beta, -alpha, &mut search);
    put_place_best_score = first_child_board.put_place;
    if print_log { 
        println!("put: {}, nega scout score: {}",Board::move_bit_to_str(1 << put_place_best_score).unwrap(), alpha);
    };

    for put_board in put_boards_iter {
        let current_put_board = &put_board.board;
        let put_place = put_board.put_place;
        let mut score = -nws_perfect(current_put_board, -alpha - 1, &mut search);
        if score > alpha {
            if print_log { 
                println!(" put: {}, null window score: {} => reserch [{},{}]",Board::move_bit_to_str(1 << put_place).unwrap(), score, alpha, beta);
            }
            score = -pvs_perfect(current_put_board, -beta, -alpha, &mut search);
            if score > alpha {
                alpha = score;
                put_place_best_score = put_place;
            }
        }
        if print_log { 
            println!("put: {}, nega scout score: {}",Board::move_bit_to_str(1 << put_place).unwrap(), score);
        }
    }

    if print_log { 
        println!("best move: {}, score: {}{}",Board::move_bit_to_str(1 << put_place_best_score).unwrap(), if alpha > 0 {"+"} else {""},alpha);
        println!("searched nodes: {}\nsearched leaf nodes: {}", search.perfect_search_node_count, search.perfect_search_leaf_node_count);
    }

    Ok(SolverResult{
        best_move: 1 << put_place_best_score,
        eval: alpha,
        node_count: search.perfect_search_node_count,
        leaf_node_count: search.perfect_search_leaf_node_count
    })
}

/// オセロの盤面に対する勝利可能性を評価し、最適な手を決定する。
///
/// この関数は、Null Window Search (NWS) アルゴリズムを使用して、
/// 現在の盤面から勝ち、引き分け、負けの結果をもたらす最適な手を判断します。
/// 特に終盤の局面で有効です。
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `print_log` - 真の場合、探索の進行状況と結果をコンソールに出力します。
///
/// # 戻り値
/// `Result<SolverResult, SolverErr>` 型。成功した場合、`SolverResult`オブジェクトが含まれ、
/// 最適な手とその評価値（勝ち:1、引き分け:0、負け:-1）、探索したノード数、葉ノード数を含みます。
/// 合法手が存在しない場合は、`SolverErr::NoMove`エラーが返されます。
///
/// # 注記
/// 探索過程の進行状況や結果の詳細な出力が必要な場合は、print_logパラメータをtrueに設定してください。これにより、
/// 各手の評価値や探索したノードの数など、探索に関する詳細な情報が出力されます。
pub fn winning_solver(board: &Board, print_log: bool, t_table: &mut TranspositionTable, evaluator : &mut Evaluator) -> Result<SolverResult, SolverErr>
{
    let legal_moves = board.put_able();
    if legal_moves == 0 {
        return Err(SolverErr::NoMove)
    }

    let mut search = Search::new(board, 0, t_table, evaluator);
    
    if print_log {
        println!("my_turn: {}", if board.next_turn == Board::BLACK {"Black"} else {"White"});
        println!("depth: {}", board.empties_count());
        board.print_board();
    };

    if print_log {print!("move_ordering....");};
    let mut put_boards = 
        if board.empties_count() < MOVE_ORDERING_EVAL_LEVEL + 2 {
            get_put_boards(board, legal_moves)
        } else {
            move_ordering_eval(board, legal_moves, MOVE_ORDERING_EVAL_LEVEL,  &mut search)
        };
    if print_log {println!("OK");};

    // [alpha, beta] = [0, 1]
    let mut put_place_best_score = 0;
    let mut eval = -1;
    let beta = 1;
    let mut draw_or_lose_board_index: Vec<usize> = Vec::new();

    for (i, put_board) in  put_boards.iter_mut().enumerate() {
        let current_put_board = &mut put_board.board;
        let put_place = put_board.put_place;
        let score = -nws_perfect(current_put_board, -beta,&mut search);
        if score > 0 {
            if print_log { 
                println!(" put: {}, Win",Board::move_bit_to_str(1 << put_place).unwrap());
            }
            if eval <= 0 {
                put_place_best_score = put_place;
                eval = 1
            };
            break;
        } else if score < 0 {
            if print_log { 
                println!(" put: {}, Lose",Board::move_bit_to_str(1 << put_place).unwrap());
            }
        } else {
            draw_or_lose_board_index.push(i);
            if eval < 0 {
                put_place_best_score = put_place;
                eval = 0
            };
            if print_log { 
                println!(" put: {}, Draw or Lose", Board::move_bit_to_str(1 << put_place).unwrap());
            }
        }
    }

    if eval == 0 {
        // [alpha, beta] = [-1, 0]
        let beta = 0;

        for &i in draw_or_lose_board_index.iter(){
            let put_board = &mut put_boards[i];
            let current_put_board = &mut put_board.board;
            let put_place = put_board.put_place;
            let score = -nws_perfect(current_put_board, -beta,&mut search);
            if score == 0 {
                if print_log { 
                    println!(" put: {}, Draw", Board::move_bit_to_str(1 << put_place).unwrap());
                }
                if eval < 0 {
                    put_place_best_score = put_place;
                    eval = 0
                };
                break;

            } else if score < 0 {
                if print_log { 
                    println!(" put: {}, Lose",Board::move_bit_to_str(1 << put_place).unwrap());
                }
                eval = -1;
            } else {
                eprintln!("Error ocurred in winning_solver");
                panic!()
            }
        }   
    }
    if eval == -1 {
        put_place_best_score = put_boards[0].put_place;
    }

    if print_log { 
        println!("best move: {}, score: {}",Board::move_bit_to_str(1 << put_place_best_score).unwrap(), if eval > 0 {"Win"} else if eval < 0 {"Lose"} else {"Draw"});
        println!("searched nodes: {}\nsearched leaf nodes: {}", search.perfect_search_node_count, search.perfect_search_leaf_node_count);
    }

    Ok(SolverResult{
        best_move: 1 << put_place_best_score,
        eval,
        node_count: search.perfect_search_node_count,
        leaf_node_count: search.perfect_search_leaf_node_count
    })
}

/// オセロの盤面に対する評価関数を用いた探索を行い、最適な手を決定する。
///
/// この関数は、評価関数に基づいて盤面のスコアを計算し、
/// Principal Variation Search (PVS) および Null Window Search (NWS) アルゴリズムを使用して
/// 最適な手を探索します。探索深度は引数 `lv` で指定されます。
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `lv` - 探索の深さを表す整数値。
/// * `print_log` - 真の場合、探索の進行状況と結果をコンソールに出力します。
///
/// # 戻り値
/// `Result<SolverResult, SolverErr>` 型。成功した場合、`SolverResult`オブジェクトが含まれ、
/// 最適な手とその評価値、探索したノード数、葉ノード数を含みます。
/// 合法手が存在しない場合は、`SolverErr::NoMove`エラーが返されます。
///
/// # 注記
/// この関数は複雑なアルゴリズムを用いて盤面の探索を行うため、計算に時間がかかる可能性があります。
/// 探索の深さ (lv) は、盤面の複雑さや求める精度に応じて適切に設定する必要があります。
/// また、print_logパラメータをtrueに設定することで、探索の進行状況や結果の詳細がコンソールに出力されます。
pub fn eval_solver(board: &Board, lv: i32, selectivity_lv: i32, print_log: bool, t_table: &mut TranspositionTable, evaluator : &mut Evaluator) -> Result<SolverResult, SolverErr>
{
    let legal_moves = board.put_able();
    if legal_moves == 0 {
        return Err(SolverErr::NoMove)
    }

    let mut search = Search::new(board, selectivity_lv, t_table, evaluator);
    
    if print_log {
        println!("my_turn: {}", if board.next_turn == Board::BLACK {"Black"} else {"White"});
        println!("depth: {}", board.empties_count());
        // board.print_board();
    };
    
    
    if print_log {println!("move_ordering....");};
    
    
    if lv > 6 {
        pvs_eval(board, -SCORE_INF, SCORE_INF, lv - 3, &mut search);
    }

    let put_boards = 
        if lv - 3 <= 0 {
            get_put_boards(board, legal_moves)
        } else {
            move_ordering_eval(board, legal_moves, MOVE_ORDERING_EVAL_LEVEL.min(lv - 4),  &mut search)
        };
    if print_log {println!("OK");};

    let mut alpha = -SCORE_INF;
    let beta = SCORE_INF;
    let mut put_place_best_score ;
    
    let mut put_boards_iter = put_boards.iter();
    let first_child_board = put_boards_iter.next().unwrap();
    alpha = -pvs_eval(&first_child_board.board, -beta, -alpha, lv - 1, &mut search);
    put_place_best_score = first_child_board.put_place;
    if print_log { 
        println!("put: {}, nega scout score: {}",Board::move_bit_to_str(1 << put_place_best_score).unwrap(), alpha);
    };

    for put_board in put_boards_iter {
        let current_put_board = &put_board.board;
        let put_place = put_board.put_place;
        let mut score = -nws_eval(current_put_board, -alpha - 1, lv - 1, &mut search);
        if score > alpha {
            if print_log { 
                println!(" put: {}, null window score: {} => reserch [{},{}]",Board::move_bit_to_str(1 << put_place).unwrap(), score, alpha, beta);
            }
            score = -pvs_eval(current_put_board, -beta, -alpha, lv - 1, &mut search);
            if score > alpha {
                alpha = score;
                put_place_best_score = put_place;
            }
        }
        if print_log { 
            println!("put: {}, nega scout score: {}",Board::move_bit_to_str(1 << put_place).unwrap(), score);
        }
    }

    if print_log { 
        println!("best move: {}, score: {}{}",Board::move_bit_to_str(1 << put_place_best_score).unwrap(), if alpha > 0 {"+"} else {""},alpha);
        println!("searched nodes: {}\nsearched leaf nodes: {}", search.eval_search_node_count, search.eval_search_leaf_node_count);
    }
    
    Ok(SolverResult{
        best_move: 1 << put_place_best_score,
        eval: alpha,
        node_count: search.eval_search_node_count,
        leaf_node_count: search.eval_search_leaf_node_count
    })
}
