
use std::{io::{self, Write}, process::exit};
use deft_reversi_engine::*;
pub struct CUIOthello {
    game: Game,
    solver: Solver,
    ai_level: i32,
}

impl CUIOthello {
    pub fn new(ai_level: i32, eval_path: &str) -> Self {
        let eval  = match Evaluator::read_file(eval_path) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Evaluator: {}", e);
                Evaluator::default()
            }
        };
 
        CUIOthello {
            game: Game::new(),
            solver: Solver::new(eval),
            ai_level,
        }
    }

    fn display_board(&self) {
        
        println!("  A B C D E F G H");
        for i in 0..8 {
            print!("{}", i + 1);
            for j in 0..8 {
                print!(" ");
                let mask = 1u64 << (i*8+j);
                if self.game.current.board.player & mask != 0 {
                    print!("{}",
                    if self.game.current.turn == Board::BLACK { "X" } else { "O" });
                } else if self.game.current.board.opponent & mask != 0 {
                    print!("{}",
                    if self.game.current.turn == Board::BLACK { "O" } else { "X" });
                } else {
                    print!(".");
                }
            }
            println!();
            
        }

        println!("------------------------------------");
        println!("{:<10} | {:<10} | {:<10}", 
            "Turn", "Move", "Empties"
        );
        println!("{:<10} | {:<10} | {:<10}", 
            if self.game.current.turn == Board::BLACK { "Black" } else { "White" },
            self.game.current.board.move_count() + 1,
            self.game.current.board.empties_count()
        );
        println!("------------------------------------");
    }

    fn display_score(&self) {
        let black_count = self.game.current.board.player.count_ones();
        let white_count = self.game.current.board.opponent.count_ones();
        println!("スコア: プレイヤー (X): {}, コンピュータ (O): {}", black_count, white_count);
    }

    fn display_hints(&self) {
            let legal_moves = self.game.current.board.moves();
            println!("ヒント: 打てる場所: {:?}", self.get_legal_moves(legal_moves));
        
    }

    fn get_legal_moves(&self, legal_moves: u64) -> Vec<String> {
        let mut moves = Vec::new();
        let mut temp_moves = legal_moves;
        while temp_moves != 0 {
            let move_bit = temp_moves & (!temp_moves + 1);
            temp_moves &= temp_moves - 1;
            if let Ok(move_str) = position_bit_to_str(move_bit) {
                moves.push(move_str);
            }
        }
        moves
    }

    fn player_turn(&mut self) {
        print!(">");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();
        
        match input {
            "undo" => {
                if self.game.undo().is_err() {
                    println!("これ以上戻れません。");
                } else {
                    println!("1手戻しました。");
                };
            },
            "redo" => {
                if self.game.redo().is_err() {
                    println!("これ以上やり直せません。");
                } else {
                    println!("1手やり直しました。");
                };
            },
            "go" => {
                self.computer_turn();
            }
            "hint" => {
                self.display_hints();
            },
            "quit" | "exit"  => {
                exit(0);
            }
            _ => {
                if let Err(e) = self.game.put(input) {
                    println!("無効な入力です: {}", e);
                }
            }
        }
    }

    fn computer_turn(&mut self) {
        println!("コンピュータが考えています...");
        let result = self.solver.solve(&self.game.current.board, self.ai_level);
        if let Ok(move_str) = position_bit_to_str(result.best_move) {
            println!("コンピュータの手: {}", move_str);
            self.game.put(&move_str).unwrap();
        }
    }

    pub fn play(&mut self) {

        loop {
            self.display_board();

            if self.game.is_end() {
                println!("ゲーム終了！");
                self.display_score();
                let black_count = self.game.current.board.player.count_ones();
                let white_count = self.game.current.board.opponent.count_ones();
                if black_count > white_count {
                    println!("プレイヤーの勝利！");
                } else if black_count < white_count {
                    println!("コンピュータの勝利！");
                } else {
                    println!("引き分け！");
                }
                break;
            }

            if self.game.is_pass() {
                println!("{}はパスです。", if self.game.current.turn == Board::BLACK { "プレイヤー" } else { "コンピュータ" });
                self.game.pass();
                continue;
            }

            self.player_turn();
        }
    }
}

