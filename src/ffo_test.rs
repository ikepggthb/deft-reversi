use std::time;

use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

use crate::{board::*, t_table::*};
use crate::solver::*;
use crate::eval::*;

pub fn ffo_test() -> Result<(),  std::io::Error> {

    let mut evaluator = Evaluator::read_file().unwrap();
    let mut t = TranspositionTable::new();
    for i in 40..60 {
        let filename = format!("data/ffo_test/end{}.pos", i);
        let board = match read_ffo_test_files(&filename) {
            Ok(it) => it,
            Err(err) => {
                eprintln!("Error reading the file {}: {}", filename, err);
                continue;
            },
        };

        let selectivity_lv = 3;
    
        println!("#{} ", i);
        // board.print_board();
        println!("    num of empties: {}", board.empties_count());
        println!("    selectivity   : {} %", crate::mpc::SELECTIVITY[selectivity_lv as usize].percent);
        
        let now = time::Instant::now();
        let solver_result = 
            // match eval_solver(&board, 20, selectivity_lv, false, &mut t, &mut evaluator) {

            match perfect_solver(&board, false, selectivity_lv, &mut t, &mut evaluator) {
                Ok(result) => result,
                Err(e) => {
                        eprintln!("Error occurred in perfect solver.");
                        panic!();
                    }
            };
            
        let end = now.elapsed();
        println!("    score         : {:+}", solver_result.eval);
        println!("    best move     : {  }", Board::move_bit_to_str(solver_result.best_move).unwrap());
        println!("    time          : {:?}", end);
        println!("    node          : {  }", solver_result.node_count);
        println!("    nps [/s]      : {  }", solver_result.node_count as f64 / end.as_secs_f64());
        println!();

    }

    Ok(())
}

fn read_ffo_test_files<P: AsRef<Path>>(filename: P) -> io::Result<Board> {
    let file = File::open(filename)?;
    let reader = io::BufReader::new(file);
    let mut board = Board { bit_board: [0;2], next_turn: Board::BLACK };

    let mut lines = reader.lines();

    let first_line = lines.next().unwrap().unwrap();
    for (i,c) in first_line.chars().enumerate() {
        match c {
            'O' => {
                board.bit_board[Board::WHITE] |= 1 << i;
            },
            'X' => {
                board.bit_board[Board::BLACK] |= 1 << i;
            }
            _ => ()
        }
    }
    
    let second_line = lines.next().unwrap().unwrap();
    // println!("{}",first_line);
    // println!("{}",second_line);
    if second_line.contains("Black") {
        board.next_turn = Board::BLACK;
    }else {
        board.next_turn = Board::WHITE;
    }

    Ok(board)
}
