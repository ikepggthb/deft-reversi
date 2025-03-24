use std::time;

use std::fs::File;
use std::io::{self, BufRead};

use deft_reversi_engine::*;

// hh:mm:ss.mmm の形式にフォーマット
fn format_duration(duration: time::Duration) -> String {
    let millis = duration.as_millis() % 1000; // ミリ秒
    let seconds = duration.as_secs() % 60; // 秒
    let minutes = duration.as_secs() / 60 % 60; // 分
    let hours = duration.as_secs() / 3600; // 時

    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

pub fn solve(path: &str, eval_path: &str, level: i32) {
    let evaluator = Evaluator::read_file(eval_path).unwrap();
    let mut solver = Solver::new(evaluator);

    let board_list: Vec<Board> = match read_solve_file(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    let total_solve_start_time = time::Instant::now();
    let mut total_nodes = 0;

    let table_header = format!(
        "{:5} | {:5} | {:4} | {:9} | {:28} | {:12} | {:15} | {:14}",
        " #", "score", "move", "n_empties", "solver", "node", "nps", "time"
    );
    let table_separator_line = "-".repeat(table_header.len());

    println!("{}", table_separator_line);
    println!("{}", table_header);
    println!("{}", table_separator_line);
    for (i, board) in board_list.iter().enumerate() {
        let solve_start_time = time::Instant::now();
        let solver_result = solver.solve(board, level);
        let solve_time = solve_start_time.elapsed();

        println!(
            "{:>5}   {:+5}   {:>4}   {:>9}   {:28}   {:>12}   {:>15.3}   {:>14} ",
            i + 1,
            solver_result.eval,
            position_bit_to_str(solver_result.best_move).unwrap(),
            board.empties_count(),
            solver_result
                .solver_type
                .description(),
            solver_result.searched_nodes,
            solver_result.searched_nodes as f64 / solve_time.as_secs_f64(),
            format_duration(solve_time)
        );
        total_nodes += solver_result.searched_nodes;
    }

    let total_solve_time = total_solve_start_time.elapsed();

    println!("{}", table_separator_line);
    println!(
        "{:>5}   {:+5}   {:>4}   {:>9}   {:28}   {:>12}   {:>15.3}   {:>14} ",
        "total",
        "",
        "",
        "",
        "",
        total_nodes,
        total_nodes as f64 / total_solve_time.as_secs_f64(),
        format_duration(total_solve_time)
    );

    println!("{}", table_separator_line);
}

fn read_solve_file(path: &str) -> Result<Vec<Board>, std::io::Error> {
    let mut boards = Vec::new();
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let trimmed_line = line.trim();

        if line.is_empty() {
            continue;
        }

        // 空白でボードと手番を分割
        let parts: Vec<&str> = trimmed_line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Line must contain a board and a turn separated by a space: {}",
                    trimmed_line
                ),
            ));
        }

        let board_data = parts[0]; // ボード部分
        let turn_char = parts[1]; // 手番部分

        if board_data.len() != 64 || turn_char.len() != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Board must be 64 characters and turn must be 1 character: {}",
                    trimmed_line
                ),
            ));
        }

        let mut board = Board {
            player: 0,
            opponent: 0,
        };
        for (i, c) in board_data.chars().enumerate() {
            let c = c.to_ascii_uppercase();
            match c {
                'X' | 'B' => board.player |= 1 << i,
                'O' | 'W' => board.opponent |= 1 << i,
                '-' | '_' => (),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Invalid character '{}' in line: {}", c, trimmed_line),
                    ));
                }
            }
        }

        // 手番を解析
        match turn_char.to_ascii_uppercase().chars().next().unwrap() {
            'X' => (),           // 手番がXならそのまま
            'O' => board.swap(), // 手番がOならプレイヤーと相手をスワップ
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Invalid turn character '{}' in line: {}",
                        turn_char, trimmed_line
                    ),
                ));
            }
        }
        boards.push(board);
    }

    Ok(boards)
}
