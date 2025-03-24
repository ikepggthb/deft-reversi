mod solve;
mod self_play;
mod play;
mod perft;

use crate::play::*;
use crate::solve::*;
use crate::self_play::*;
use crate::perft::*;
use clap::Parser;

const DEFAULT_LEVEL: u8 = 10;


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
    /// (e.g. --self-play 10 --level 16 --self-play-out "./self-play.txt" --self-play-start-rand 45)
    #[arg(long, id = "Number of games")]
    self_play: Option<usize>,

    /// Output file path for self-play records
    #[arg(long, id = "PATH", default_value = "./self-play.txt")]
    self_play_out: String,

    /// Number of starting random moves in self-play
    #[arg(long, default_value_t = 20)]
    self_play_start_rand: usize,

    #[arg(long, id = "DEPTH")]
    perft: Option<u64>,

    #[arg(long)]
    perft_count_pass: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let level = args.level as i32;
    let eval_path = args.eval_path.as_deref().unwrap_or("../data/eval/eval.json");

    if let Some(n_games) = args.self_play {
        // 自己対戦モード
        let start_rand = args.self_play_start_rand;
        let out_path = args.self_play_out;
        run_self_play(n_games, level, start_rand, eval_path, &out_path)?;
    } else if let Some(path) = &args.solve {
        // Solveモード
        // e.g. -solve ".\problem\fforum-40-59.obf" -l 25
        println!("AI level   :  {}", args.level);
        solve(path, eval_path, level);
    } else if let Some(depth) = &args.perft {
        // Perft mode
        // e.g. --perft 11
        run_perft(*depth, args.perft_count_pass);
    }else {
        // 通常プレイモード
        let mut game = OthelloCLI::new(
            level,
            eval_path
        );
        game.play();
    }

    Ok(())
}

