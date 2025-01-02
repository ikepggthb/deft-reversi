

use deft_reversi_engine::*;
use std::io::{self, Write};

#[test] 
fn stackoverflow_test(){
    let mut game = CUIOthello::new(10); // AIレベル10
    game.play();
}

struct CUIOthello {
    game: Game,
    solver: Solver,
    ai_level: i32,
    hints_enabled: bool,
}

impl CUIOthello {
    fn new(ai_level: i32) -> Self {
        CUIOthello {
            game: Game::new(),
            solver: Solver::new(Evaluator::default()),
            ai_level,
            hints_enabled: false,
        }
    }

    fn display_board(&self) {
        println!("{}", self.game.current.board.print_board_string(self.game.current.turn));
    }

    fn display_score(&self) {
        let black_count = self.game.current.board.player.count_ones();
        let white_count = self.game.current.board.opponent.count_ones();
        println!("スコア: プレイヤー (X): {}, コンピュータ (O): {}", black_count, white_count);
    }

    fn display_hints(&self) {
        if self.hints_enabled {
            let legal_moves = self.game.current.board.put_able();
            println!("ヒント: 打てる場所: {:?}", self.get_legal_moves(legal_moves));
        }
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
        loop {
            print!("入力（例: E6）: ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            let input = input.trim();

            if input == "undo" {
                if self.game.undo().is_err() {
                    println!("これ以上戻れません。");
                } else {
                    println!("1手戻しました。");
                }
                return;
            } else if input == "redo" {
                if self.game.redo().is_err() {
                    println!("これ以上やり直せません。");
                } else {
                    println!("1手やり直しました。");
                }
                return;
            } else if let Err(e) = self.game.put(input) {
                println!("無効な入力です: {}", e);
            } else {
                break;
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

    fn play(&mut self) {
        println!("オセロゲーム開始！");
        println!("プレイヤー: X, コンピュータ: O");
        println!("コマンド: 'undo' で1手戻る, 'redo' でやり直し, 'quit' で終了");

        loop {
            self.display_board();
            self.display_hints();

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

            if self.game.current.turn == Board::BLACK {
                self.player_turn();
            } else {
                self.computer_turn();
            }
        }
    }
}

fn main() {
    let mut game = CUIOthello::new(10); // AIレベル10
    game.play();
}
