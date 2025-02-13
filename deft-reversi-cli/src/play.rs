use deft_reversi_engine::*;
use std::{
    backtrace,
    io::{self, Write},
    process::exit,
};
pub struct OthelloCLI {
    game: Game,
    solver: Solver,
    ai_level: i32,
    setting_turn: SettingTurn,
}

enum Turn {
    Computer,
    Player,
}

struct SettingTurn {
    black: Turn,
    white: Turn,
}

impl SettingTurn {
    fn get(&self, c: Color) -> &Turn {
        match c {
            Color::Black => &self.black,
            Color::White => &self.white,
        }
    }
}

impl OthelloCLI {
    pub fn new(ai_level: i32, eval_path: &str) -> Self {
        let eval = match Evaluator::read_file(eval_path) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Evaluator: {}", e);
                Evaluator::default()
            }
        };

        OthelloCLI {
            game: Game::new(),
            solver: Solver::new(eval),
            ai_level,
            setting_turn: SettingTurn {
                black: Turn::Player,
                white: Turn::Player,
            },
        }
    }

    fn display_board(&self) {
        let (black_score, white_score) = {
            match self.game.current.turn {
                Color::Black => (
                    self.game.current.board.player.count_ones(),
                    self.game.current.board.opponent.count_ones(),
                ),
                Color::White => (
                    self.game.current.board.opponent.count_ones(),
                    self.game.current.board.player.count_ones(),
                ),
            }
        };
        println!("  A B C D E F G H");
        for i in 0..8 {
            print!("{}", i + 1);
            for j in 0..8 {
                print!(" ");
                let mask = 1u64 << (i * 8 + j);
                if self.game.current.board.player & mask != 0 {
                    print!("{}", self.game.current.turn.get_char());
                } else if self.game.current.board.opponent & mask != 0 {
                    print!("{}", self.game.current.turn.opponent().get_char());
                } else {
                    print!(".");
                }
            }

            // print!("   ");
            // match i {
            //     1 => {
            //         print!("Black: {}", black_score);
            //     }
            //     2 => {
            //         print!("White: {}", white_score);
            //     }
            //     _ => (),
            // }
            println!();
        }

        println!("-----------------------------------------------------------");
        println!("                           Score                           ");
        println!(
            "    Black  {:2}                           {:>2}   White    ",
            black_score, white_score
        );
        println!("-----------------------------------------------------------");
        println!("{:<17} | {:<17} | {:<17}", "Turn", "Move", "Empties");
        println!(
            "{:>17} | {:>17} | {:>17}",
            self.game.current.turn.get_str(),
            self.game.current.board.move_count() + 1,
            self.game.current.board.empties_count()
        );
        println!("-----------------------------------------------------------");
        println!("                          Record                           ");
        let record: String = self.game.record();

        // 各手は2文字なので、2文字ずつに分割する
        let moves: Vec<&str> = record
            .as_bytes()
            .chunks(2)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect();

        // 20手ごとに改行して表示する
        for line in moves.chunks(20) {
            println!("{}", line.join(" "));
        }
        println!("-----------------------------------------------------------");
    }

    fn execute_player_prompt(&mut self) {
        loop {
            print!("> ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            let input = input.trim();

            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "new" | "init" => {
                    self.game = Game::new();
                    break;
                }
                "level" | "l" => {
                    if parts.len() != 2 {
                        println!("Invalid level command. Usage: level <number> (1-60)");
                        continue;
                    }
                    match parts[1].parse::<i32>() {
                        Ok(new_level) => {
                            if new_level < 1 || new_level > 60 {
                                println!("Invalid level. Level must be between 1 and 60.");
                                continue;
                            }
                            self.ai_level = new_level;
                            println!("Level set to: {}", new_level);
                            break; // レベル変更後はループから抜け、ボード表示を更新する
                        }
                        Err(_) => {
                            println!("Invalid level command. Usage: level <number> (1-60)");
                            continue;
                        }
                    }
                }
                "play" => {
                    if parts.len() != 2 {
                        eprintln!("Invalid mode command. Usage: play <record of game (f5f6...)>");
                        continue;
                    }
                    let all_record = self.game.record() + parts[1];
                    if check_record(&all_record).is_err() {
                        eprintln!("Invalid mode command. Usage: play <record of game (f5f6...)>");
                    }
                    for position in parts[1].as_bytes().chunks_exact(2) {
                        self.game.put(std::str::from_utf8(position).unwrap());
                        if self.game.is_pass() {
                            self.game.pass();
                        }
                    }
                    break;
                }
                "undo" => {
                    if let Err(e) = self.game.undo() {
                        println!("{}", e);
                    } else {
                        break;
                    }
                }
                "mode" => {
                    let print_err = || {
                        println!("Invalid mode command. Usage: mode <number>");
                        println!("Available modes:");
                        println!("  0 : Black - Player, White - Computer");
                        println!("  1 : Black - Computer, White - Player");
                        println!("  2 : Computer vs Computer");
                        println!("  3 : Player vs Player");
                    };
                    if parts.len() != 2 {
                        print_err();
                        continue;
                    }
                    match parts[1].parse::<u32>() {
                        Ok(mode) => {
                            match mode {
                                0 => {
                                    self.setting_turn = SettingTurn {
                                        black: Turn::Player,
                                        white: Turn::Computer,
                                    };
                                    println!("Mode set: Black - Player, White - Computer");
                                    break; // モード変更後はループから抜け、ボード表示を更新する
                                }
                                1 => {
                                    self.setting_turn = SettingTurn {
                                        black: Turn::Computer,
                                        white: Turn::Player,
                                    };
                                    println!("Mode set: Black - Computer, White - Player");
                                    break; // モード変更後はループから抜け、ボード表示を更新する
                                }
                                2 => {
                                    self.setting_turn = SettingTurn {
                                        black: Turn::Computer,
                                        white: Turn::Computer,
                                    };
                                    println!("Mode set: Computer vs Computer");
                                    break; // モード変更後はループから抜け、ボード表示を更新する
                                }
                                3 => {
                                    self.setting_turn = SettingTurn {
                                        black: Turn::Player,
                                        white: Turn::Player,
                                    };
                                    println!("Mode set: Player vs Player");
                                    break; // モード変更後はループから抜け、ボード表示を更新する
                                }
                                _ => {
                                    print_err();
                                }
                            }
                        }
                        Err(_) => {
                            print_err();
                        }
                    }
                }
                "redo" => {
                    if self.game.redo().is_err() {
                        println!("Cannot redo any further.");
                    } else {
                        println!("One move has been redone.");
                    }
                    break;
                }
                "go" => {
                    self.computer_turn();
                    break;
                }
                "help" => {
                    self.display_help();
                }
                "quit" | "exit" => {
                    exit(0);
                }
                _ => {
                    if self.game.put(input).is_err() {
                        println!("Invalid input");
                    } else {
                        break;
                    }
                }
            }
        }
    }

    fn computer_turn(&mut self) {
        let result = self.solver.solve(&self.game.current.board, self.ai_level);
        if let Ok(move_str) = position_bit_to_str(result.best_move) {
            println!("move: {}", move_str);
            self.game.put(&move_str).unwrap();
        }
    }

    /// Displays the final game score in a refined format.
    fn display_score(&self) {
        let (black_score, white_score) = {
            match self.game.current.turn {
                Color::Black => (
                    self.game.current.board.player.count_ones(),
                    self.game.current.board.opponent.count_ones(),
                ),
                Color::White => (
                    self.game.current.board.opponent.count_ones(),
                    self.game.current.board.player.count_ones(),
                ),
            }
        };

        println!("\n==============================");
        println!("         Final Score          ");
        println!("------------------------------");
        println!("Black (X): {:>3}", black_score);
        println!("White (O): {:>3}", white_score);
        println!("==============================\n");
    }
    /// Displays a help message showing the available commands.
    fn display_help(&self) {
        println!("Available commands:");
        println!(
            "  new | init                - Start a new game with the standard initial position."
        );
        println!("  level | l <number>        - Set AI level (1-60).");
        println!("  play <record>             - Play a game record (e.g., f5f6...).");
        println!("  undo                      - Undo the last move.");
        println!("  redo                      - Redo a move.");
        println!("  mode <number>             - Change game mode. Available modes:");
        println!("                                0 : Black - Player, White - Computer");
        println!("                                1 : Black - Computer, White - Player");
        println!("                                2 : Computer vs Computer");
        println!("                                3 : Player vs Player");
        println!("  go                        - Let the computer make a move.");
        println!("  help                      - Show this help message.");
        println!("  quit | exit               - Exit the game.");
        println!();
        println!("Alternatively, enter a coordinate (e.g., D3) to place your piece.");
    }
    pub fn play(&mut self) {
        loop {
            self.display_board();

            if self.game.is_end() {
                println!("Game Over!");

                self.display_score();
                let (black_score, white_score) = {
                    match self.game.current.turn {
                        Color::Black => (
                            self.game.current.board.player.count_ones(),
                            self.game.current.board.opponent.count_ones(),
                        ),
                        Color::White => (
                            self.game.current.board.opponent.count_ones(),
                            self.game.current.board.player.count_ones(),
                        ),
                    }
                };
                match black_score.cmp(&white_score) {
                    std::cmp::Ordering::Equal => {
                        println!("Draw!");
                    }
                    std::cmp::Ordering::Greater => {
                        println!("Black wins!");
                    }
                    std::cmp::Ordering::Less => {
                        println!("White wins!");
                    }
                }
            }

            if self.game.is_pass() {
                // println!("{}はパスです。", if self.game.current.turn == Board::BLACK { "プレイヤー" } else { "コンピュータ" });
                self.game.pass();
                continue;
            }

            if let Turn::Computer = self.setting_turn.get(self.game.current.turn) {
                if !self.game.is_end() {
                    self.computer_turn();
                } else {
                    self.execute_player_prompt();
                }
            } else {
                self.execute_player_prompt();
            }
        }
    }
}
