use std::time;

use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;


use crate::board::*;
use crate::solver::*;
use crate::eval::*;

pub fn ffo_test() -> Result<(),  std::io::Error> {

    let evaluator = Evaluator::read_file("res/eval.json").unwrap();
    let mut solver = Solver::new(evaluator);
    solver.set_ai_level(60);
    // solver.print_log = true;
    
    for i in 40..41 {
        let filename = format!("data/ffo_test/end{}.pos", i);
        let board = match read_ffo_test_files(&filename) {
            Ok(it) => it,
            Err(err) => {
                eprintln!("Error reading the file {}: {}", filename, err);
                continue;
            },
        };
    
        println!("#{} ", i);
        // board.print_board();
        println!("    num of empties: {}", board.empties_count());
        
        let now = time::Instant::now();
        let solver_result = match solver.solve(&board) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("Error occurred in perfect solver.");
                panic!();
            }
        };
        
        let end = now.elapsed();
        println!("    selectivity   : {} %", crate::mpc::SELECTIVITY[solver_result.selectivity_lv as usize].percent);
        println!("    score         : {:+}", solver_result.eval);
        println!("    best move     : {  }", position_bit_to_str(solver_result.best_move).unwrap());
        println!("    node          : {  }", solver_result.searched_nodes);
        println!("    nps [/s]      : {  }", solver_result.searched_nodes as f64 / end.as_secs_f64());
        println!("    time          : {:?}", end);
        println!();

    }

    Ok(())
}

fn read_ffo_test_files<P: AsRef<Path>>(filename: P) -> io::Result<Board> {
    let file = File::open(filename)?;
    let reader = io::BufReader::new(file);
    let mut board = Board { bit_board: [0;2], next_turn: Board::BLACK};

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
