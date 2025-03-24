use deft_reversi_engine::*;

use std::fs::OpenOptions;
use rand::prelude::*;
use std::io::Write;


/// 自己対戦を実行し、棋譜をファイルに保存する関数
pub fn run_self_play(n_games: usize, level: i32, start_rand: usize, eval_path: &str, out_path: &str) -> Result<(), Box<dyn std::error::Error>> {
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

            let mut rand_move_bit = 0;
            for (i, move_bit) in MoveIterator::new(legal_moves).enumerate() {
                if i == rand_move_index {rand_move_bit = move_bit; break;}
            }
            if let Ok(move_str) = position_bit_to_str(rand_move_bit) {
                let put_result = game.put(&move_str);
                #[cfg(debug_assertions)]
                if put_result.is_err() {
                    eprintln!("err: putに失敗しました。");
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

        solver.search.t_table.set_old();

        // 進捗表示（オプション）
        println!("{} / {} ゲーム完了", game_num, n_games);
    }

    println!("自己対戦完了。\n棋譜は {} に保存されました。", out_path);
    Ok(())
}
