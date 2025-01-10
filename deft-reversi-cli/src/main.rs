

use deft_reversi_engine::*;
use std::{io::{self, Write}, process::exit};

struct CUIOthello {
    game: Game,
    solver: Solver,
    ai_level: i32,
}

impl CUIOthello {
    fn new(ai_level: i32, eval_path: &str) -> Self {
        let eval  = match Evaluator::read_file(eval_path) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Evaluator: {}", e);
                Evaluator::default()
            }
        };
        CUIOthello {
            game: Game::new(),
            solver: Solver::new(Evaluator::default()),
            ai_level,
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
            let legal_moves = self.game.current.board.put_able();
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
        loop {
            print!("入力（例: E6）: ");
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
                    }
                    return;
                },
                "redo" => {
                    if self.game.redo().is_err() {
                        println!("これ以上やり直せません。");
                    } else {
                        println!("1手やり直しました。");
                    }
                    return;
                },
                "hint" => {
                    self.display_hints();
                },
                "quit" => {
                    exit(0);
                }
                _ => {
                    if let Err(e) = self.game.put(input) {
                        println!("無効な入力です: {}", e);
                    }
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

    fn play(&mut self) {
        println!("オセロゲーム開始！");
        println!("プレイヤー: X, コンピュータ: O");
        println!("コマンド: 'undo' で1手戻る, 'redo' でやり直し, 'quit' で終了");

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

            if self.game.current.turn == Board::BLACK {
                self.player_turn();
            } else {
                self.computer_turn();
            }
        }
    }
}


const DEFAULT_LEVEL: u8 = 10;

use clap::Parser;
use rand::prelude::*;
use std::fs::OpenOptions;

/// Reversi games
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Solve
    #[arg(short, long)]
    solve: Option<String>,

 /// Path to read eval weight from
    #[arg(short, long)]
    eval_path: Option<String>,

    /// AI level
    #[arg(short, long, default_value_t = DEFAULT_LEVEL)]
    level: u8,

    /// Number of self-play games to run
    #[arg(long)]
    self_play: Option<usize>,

    /// Output file path for self-play records (default: "./self-play.txt")
    #[arg(long, default_value = "./self-play.txt")]
    self_play_out: String,

    /// Number of starting random moves in self-play (default: 20)
    #[arg(long, default_value_t = 20)]
    self_play_start_rand: usize,

}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if let Some(n_games) = args.self_play {
        // 自己対戦モード
        let level = args.level as i32;
        let start_rand = args.self_play_start_rand;
        let out_path = args.self_play_out;
        let eval_path = args.eval_path.as_deref().unwrap_or("../deft-reversi-engine/res/eval.json");
        run_self_play(n_games, level, start_rand, eval_path, &out_path)?;
    } else if let Some(path) = &args.solve {
        // Solveモード
        println!("Solve File :  {}", path);
        println!("AI level   :  {}", args.level);
    } else {
        // 通常プレイモード
        let eval_path = args.eval_path
            .unwrap_or_else(|| "../deft-reversi-engine/res/eval.json".to_string());
        let mut game = CUIOthello::new(
            args.level as i32,
            eval_path.as_str()
        );
        game.play();
    }

    Ok(())
}


/// 自己対戦を実行し、棋譜をファイルに保存する関数
fn run_self_play(n_games: usize, level: i32, start_rand: usize, eval_path: &str, out_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = thread_rng();
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)  // ファイルを上書き
        .open(out_path)?;

    // Evaluatorを一度だけ読み込む
    let evaluator = match Evaluator::read_file(eval_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Evaluatorの読み込みに失敗しました（{}）。正しい評価を計算できません。", e);
            Evaluator::default()
        }
    };

    let mut solver = Solver::new(evaluator);

    for game_num in 1..=n_games {
        let mut game = Game::new();

        // 最初のstart_rand手をランダムに打つ
        for _ in 0..start_rand {
            let legal_moves = game.current.board.put_able();
            let board = &game.current.board;
            if legal_moves == 0 {
                if game.current.board.opponent_put_able() != 0 {
                    game.pass();
                    continue;
                } else {
                    break;
                }
            }

            let (available_moves, n_moves) = get_put_boards_fast(board, legal_moves);
            // ランダムに手を選択
            let n = rng.gen_range(0..n_moves);

            let chosen_move = available_moves[n];
            if let Ok(move_str) = position_bit_to_str(available_moves[n].legal_move) {
                let put_result = game.put(&move_str);
                #[cfg(debug_assertions)]
                if put_result.is_err() {
                    eprintln!("err: putに失敗しました。");
                }
            }
        }

        while !game.is_end() {
            let legal_moves = game.current.board.put_able();
            if legal_moves == 0 {
                if game.current.board.opponent_put_able() == 0 {
                    break;
                }
                game.pass();
                continue;
            }
            let solver_result = solver.solve(&game.current.board, level);

            if solver_result.best_move == 0 {
                #[cfg(debug_assertions)]
                eprintln!("err: 最善手を計算できません。");
                break;
            }

            let move_str = match position_bit_to_str(solver_result.best_move) {
                Ok(s) => s,
                Err(_) => { 
                    eprintln!("err: position_bit_to_str");
                    break; 
                }
            };

            

            let put_result = game.put(&move_str);
            #[cfg(debug_assertions)]
            if put_result.is_err() {
                eprintln!("err: putに失敗しました。");
            }
        }

        if !game.is_end() {
            eprintln!("err: ゲームが終局ではありません。");
            continue;
        }

        // 棋譜を取得してファイルに書き込む
        let record = game.record();
        writeln!(file, "{}", record)?;

        // 進捗表示（オプション）
        println!("{} / {} ゲーム完了", game_num, n_games);
    }

    println!("自己対戦完了。棋譜は {} に保存されました。", out_path);
    Ok(())
}
