use deft_reversi_engine::{
    check_record, default_evaluator, position_num_to_str, Game, Solver, SolverOptions,
};

use rand::prelude::*;
use std::fs::OpenOptions;
use std::io::Write;

/// 自己対戦を実行し、棋譜をファイルに保存する関数
pub fn run_self_play(
    n_games: usize,
    level: i32,
    start_rand: usize,
    eval_path: &str,
    out_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = thread_rng();
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true) // ファイルを上書き
        .open(out_path)?;

    // Evaluatorを一度だけ読み込む
    let solver = match Solver::from_file(eval_path, SolverOptions::default()) {
        Ok(solver) => solver,
        Err(e) => {
            eprintln!(
                "Evaluatorの読み込みに失敗しました（{}）。正しい評価を計算できません。",
                e
            );
            Solver::new(default_evaluator())
        }
    };

    for game_num in 1..=n_games {
        let mut game = Game::new();

        // 最初のstart_rand手をランダムに打つ
        for _ in 0..start_rand {
            let legal_moves = game.current.board.moves();
            if legal_moves == 0 {
                if game.current.board.opponent_moves() != 0 {
                    game.pass();
                    continue;
                } else {
                    break;
                }
            }

            let n_moves = legal_moves.count_ones();
            // ランダムに手を選択
            let rand_move_index = rng.gen_range(0..n_moves) as usize;

            let mut bits = legal_moves;
            let mut rand_move_bit = 0;
            for i in 0..=rand_move_index {
                rand_move_bit = bits & bits.wrapping_neg();
                bits &= bits - 1;
                if i == rand_move_index {
                    break;
                }
            }
            if let Ok(move_str) = position_num_to_str(rand_move_bit.trailing_zeros() as u8) {
                if let Err(e) = game.put(&move_str) {
                    eprintln!("err: putに失敗しました: {e}");
                }
            }
        }

        while !game.is_end() {
            let legal_moves = game.current.board.moves();
            if legal_moves == 0 {
                if game.current.board.opponent_moves() == 0 {
                    break;
                }
                game.pass();
                continue;
            }
            let solver_result = solver.solve(&game.current.board, level);

            let Some(best_move) = solver_result.best_move else {
                #[cfg(debug_assertions)]
                eprintln!("err: 最善手を計算できません。");
                break;
            };

            let move_str = match position_num_to_str(best_move) {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("err: position_num_to_str");
                    break;
                }
            };

            if let Err(e) = game.put(&move_str) {
                eprintln!("err: putに失敗しました: {e}");
            }
        }

        if !game.is_end() {
            eprintln!("err: ゲームが終局ではありません。");
            continue;
        }

        // 棋譜を取得してファイルに書き込む
        let record = game.record();
        check_record(&record)?;
        writeln!(file, "{}", record)?;

        // 進捗表示（オプション）
        println!("{} / {} ゲーム完了", game_num, n_games);
    }

    println!("自己対戦完了。\n棋譜は {} に保存されました。", out_path);
    Ok(())
}
