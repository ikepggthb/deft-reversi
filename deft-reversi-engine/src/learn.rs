
use crate::search::Search;
use crate::{board_manager::*, learn, search, Evaluator};
use crate::{board::Board, perfect_search::solve_score};
use crate::t_table::*;
use serde::{Deserialize, Serialize};

use crate::eval::evaluator_const::*;
use crate::eval_for_learn::*;

use std::{env, clone};
use std::fs::File;
use std::fs;
use std::io::prelude::*;



fn read_record_file(filename: &str) -> String {
    println!("{}",filename);
    let mut f = {
        match File::open(filename) {
            Ok(f) => f,
            Err(_) => {
                eprintln!("Err: ファイルが見つかりませんでした ({})", filename);
                panic!();
            }
        }
    };
    let mut contents = String::new();
    if f.read_to_string(&mut contents).is_err() {
        eprintln!("Err: ファイルの読み込み中に問題がありました ({})", filename);
        panic!();
    }

    contents
}

fn chars_to_move_bit(s: [char; 2]) -> u64 {
    let x: u32 = {
        match s[0] {
            'a' => 0,
            'b' => 1,
            'c' => 2,
            'd' => 3,
            'e' => 4,
            'f' => 5,
            'g' => 6,
            'h' => 7,
             _  => {
                panic!("not move: {}, {}", s[0], s[1]);
            }
        }
    };
    
    let y = {
        match s[1].to_digit(10) {
            Some(i) => i - 1,
            None => panic!()
        }
    };

    1u64 << (x + y * 8)
}

#[derive(Clone)]
struct Training {
    bm: BoardManager,
    score_black: i32
}

impl Training {
    fn new() -> Self {
        Self{bm: BoardManager::new(), score_black: 0}
    }
}

fn gen_training_data(filename: &str) -> Vec<Training> {
    let record = read_record_file(filename);
    let record: Vec<char> = record.chars().collect();

    let mut training_data: Vec<Training>  = Vec::new();
    training_data.push(Training::new());
    
    let mut i = 0;
    while i < record.len() {
        if record[i] == '\r' {
            i += 1;
        }
        if record[i] == '\n'{
            let mut board = training_data.last().unwrap().bm.current_board();
            let td = training_data.last_mut().unwrap();
            board.next_turn = Board::BLACK;
            td.score_black = solve_score(&board);
            // println!("score: {}", td.score_black);

            i += 1;
            training_data.push(Training::new());
            continue;
        };
        
        let mut board = training_data.last().unwrap().bm.current_board();
        // println!("------------------------------");
        let x = [record[i], record[i+1]];
        let move_bit = chars_to_move_bit(x);

        // println!("move: {}", Board::move_bit_to_str(move_bit).unwrap());
        if board.put_piece(move_bit).is_err() {
            board.next_turn ^= 1;
            if board.put_piece(move_bit).is_err() {
                panic!();
            }
        }
        // board.print_board();
        training_data.last_mut().unwrap().bm.add(board);
        i+=2;

    }
    training_data.pop();
    println!("genarate traning data count:{}", training_data.len());

    training_data
}


fn supervised_learning(evaluator: &mut EvaluatorForLearn) {
    
    let mut learning_rate = 0.00004;
    let lambda = 0.000002;
    let mut training_data = Vec::new();
    for i in 0..200 {
        let filename = format!("data/training_data/0000_egaroucid_6_3_0_lv11/0000{i:0>3}.txt");
        training_data.append(&mut gen_training_data(&filename));
    }
    println!("train data total (Number of matches) : {}", training_data.len());


    let mut learn_count = 0;
    loop { // 学習回数分 loop
        let mut error_sum = [0.0; 61];

        for training_datum in training_data.iter() { // each 試合

            let correct_score: [i32; 2] = [training_datum.score_black, -training_datum.score_black];

            for board in training_datum.bm.board_record.iter() {
                // each 局面
                let move_count = board.move_count() as usize;
                if move_count < 2 {continue;}
                let learning_rate = if move_count < 20 {learning_rate / 3.0} else {learning_rate};
                let phase = move_count / 2;

                evaluator.clac_features(board);
                let eval = evaluator.clac_eval(board);

                let diff_eval_score = eval - correct_score[board.next_turn] as f64;

                let error = diff_eval_score.powi(2); // 平均２乗誤差

                let evaluation_scores = &mut evaluator.eval[board.next_turn][phase];
               
                for pattern in 0..N_PATTERN {
                    let e = &mut evaluation_scores.pattern_eval[pattern];
                    let f = &mut evaluator.feature_bit[pattern];
                    let n_positions = N_FEATURE_POSITIONS[pattern] as f64;

                    // each rotation boards
                    e[f[0] as usize] += -2f64 * diff_eval_score * learning_rate - 2f64 * lambda * e[f[0] as usize];
                    e[f[1] as usize] += -2f64 * diff_eval_score * learning_rate - 2f64 * lambda * e[f[1] as usize];
                    e[f[2] as usize] += -2f64 * diff_eval_score * learning_rate - 2f64 * lambda * e[f[2] as usize];
                    e[f[3] as usize] += -2f64 * diff_eval_score * learning_rate - 2f64 * lambda * e[f[3] as usize];
                }

                let mobility = 
                    N_MOBILITY_BASE 
                    + board.put_able().count_ones() as usize 
                    - board.opponent_put_able().count_ones() as usize;

                evaluation_scores.mobility_eval[mobility] +=
                    - 2f64 * diff_eval_score * learning_rate 
                    - 2f64 * lambda * evaluation_scores.mobility_eval[mobility];
                    
                evaluation_scores.const_eval +=
                    - 2f64 * diff_eval_score * learning_rate
                    - 2f64 * lambda * evaluation_scores.const_eval;

                error_sum[move_count] += error;
            }

        }
        learn_count += 1;
        for e in error_sum.iter_mut() {
            *e /= training_data.len() as f64;
        }
        let error_sum_sum = error_sum.iter().sum::<f64>()  / 40.0;
        println!("i: {learn_count:},10: {:.4} 20: {:.4}, 30: {:.4}, 40: {:.4}, 50: {:.4}, 55: {:.4}, sum: {:.4}", error_sum[10], error_sum[20], error_sum[30],error_sum[40], error_sum[50], error_sum[55], error_sum_sum);
        
        if error_sum_sum < 10.0{
            println!("exit");
            break;
        }

        // evaluator.write_file();

    }
}




pub fn learning() {

    // let mut eval = EvaluatorForLearn::new();
    let mut eval = EvaluatorForLearn::read_file().unwrap();
    supervised_learning(&mut eval);
    // eval.write_file();
}

use crate::{
    eval_search::*,
    perfect_search::*
};

pub fn npc_learn(lv: i32) {

    let mut tt = TranspositionTable::new();
    let mut evaluator = Evaluator::read_file().unwrap();

    let mut training_data = Vec::new();
    for i in 0..1 {
        let filename = format!("data/training_data/0000_egaroucid_6_3_0_lv11/0000{i:0>3}.txt");
        training_data.append(&mut gen_training_data(&filename));
    }

    const MPC_SEARCH_DEPTHS: [i32; 15] = [
        0,
        -1, -1, 1, 2, 1, 2, 3, 4, 3, 4,
         3,  4, 5, 6];

    for (i, training_datum) in training_data.iter().enumerate() {
        for board in training_datum.bm.board_record.iter(){

            let mut search = Search::new(board, 0, &mut tt, &mut evaluator);
            let n_empties = board.empties_count();
            let move_count = board.move_count();
            if move_count <= 20 || MPC_SEARCH_DEPTHS[lv as usize] == 0 || n_empties <= lv { continue; }
            let s = pvs_eval(board, -SCORE_INF, SCORE_INF, lv, &mut search);
            let ps = pvs_eval(board, -SCORE_INF, SCORE_INF, MPC_SEARCH_DEPTHS[lv as usize], &mut search);
            println!("{}, {}, {}, {}, {}", n_empties, lv, MPC_SEARCH_DEPTHS[lv as usize], s, ps);
        }
        if i > 1000 {break;}
    }
}

pub fn npc_perfect_learn() {


    let mut tt = TranspositionTable::new();
    let mut evaluator = Evaluator::read_file().unwrap();

    let mut training_data = Vec::new();
    for i in 0..1 {
        let filename = format!("data/training_data/0000_egaroucid_6_3_0_lv11/0000{i:0>3}.txt");
        training_data.append(&mut gen_training_data(&filename));
    }

    use crate::mpc::PERFECT_SEARCH_MPC_SEARCH_PARAMS;
    use crate::mpc::MpcParams;

    println!("n_empties, Search depth for prob cut, Search score, Search score for prob cut ");

    let evals: Vec<Vec<(i32, i32, i32)>> = vec![Vec::new();31];


    for training_datum in training_data.iter() {
        let s = [training_datum.score_black, -training_datum.score_black];
        for board in training_datum.bm.board_record.iter(){
            let n_empties = board.empties_count();
            let mpc_search_lv = 
                match &PERFECT_SEARCH_MPC_SEARCH_PARAMS[n_empties as usize] {
                    Some(mpc_params) => {
                        mpc_params.lv
                    },
                    None => {continue}
                };
            

            if n_empties > 20 { continue; }
            let mut search = Search::new(board, 0, &mut tt, &mut evaluator);

            let s = s[board.next_turn];
            let ps = pvs_eval(board, -SCORE_INF, SCORE_INF, mpc_search_lv, &mut search);
            println!("{}, {}, {}, {}", n_empties,mpc_search_lv, s, ps);
        }
    }
}
