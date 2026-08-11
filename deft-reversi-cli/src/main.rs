mod eval_mae;
mod match_cmd;
mod mpc_collect;
mod perft;
mod play;
mod self_play;
mod solve;

use crate::eval_mae::run as run_eval_mae;
use crate::match_cmd::run as run_match;
use crate::mpc_collect::{run as run_mpc_collect, MpcCollectArgs, MpcCollectMode};
use crate::perft::*;
use crate::play::*;
use crate::self_play::*;
use crate::solve::*;
use clap::{Parser, Subcommand};

const DEFAULT_LEVEL: u8 = 10;

/// Reversi games
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Solve
    #[arg(short, long)]
    solve: Option<String>,

    /// Path to read eval weight from
    #[arg(short, long)]
    eval_path: Option<String>,

    /// Optional evaluator used only for endgame move ordering
    #[arg(long)]
    ordering_eval: Option<String>,

    /// Transposition table size in MiB
    #[arg(long)]
    hash_mb: Option<usize>,

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

#[derive(Subcommand, Debug)]
enum Command {
    EvalMae {
        #[arg(long = "eval")]
        eval_path: String,
        #[arg(long = "data")]
        data_root: String,
        #[arg(long, default_value = "valid")]
        split: String,
        #[arg(long, default_value = "12..63")]
        phase_range: String,
        #[arg(long, default_value_t = 5000)]
        limit_per_phase: usize,
    },
    Match {
        #[arg(long)]
        eval_a: String,
        #[arg(long)]
        eval_b: String,
        #[arg(long)]
        games: usize,
        #[arg(long)]
        level: u8,
        #[arg(long, default_value_t = 8)]
        start_rand: usize,
        #[arg(long)]
        seed: Option<u64>,
    },
    MpcCollect {
        #[arg(long = "eval")]
        eval_path: String,
        #[arg(long = "data")]
        data_root: String,
        #[arg(long, default_value = "valid")]
        split: String,
        #[arg(long, value_enum)]
        mode: MpcCollectMode,
        #[arg(long, default_value = "5,6,7,8,9,10,11,12,13,14")]
        levels: String,
        #[arg(long, default_value = "12..=22")]
        empties: String,
        #[arg(long)]
        probe_depths: Option<String>,
        #[arg(long)]
        probe_levels: Option<String>,
        #[arg(long, default_value_t = 200)]
        positions_per_bucket: usize,
        #[arg(long)]
        out: String,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if let Some(command) = args.command {
        return match command {
            Command::EvalMae {
                eval_path,
                data_root,
                split,
                phase_range,
                limit_per_phase,
            } => run_eval_mae(
                &eval_path,
                &data_root,
                &split,
                &phase_range,
                limit_per_phase,
            ),
            Command::Match {
                eval_a,
                eval_b,
                games,
                level,
                start_rand,
                seed,
            } => run_match(&eval_a, &eval_b, games, level as i32, start_rand, seed),
            Command::MpcCollect {
                eval_path,
                data_root,
                split,
                mode,
                levels,
                empties,
                probe_depths,
                probe_levels,
                positions_per_bucket,
                out,
                seed,
            } => run_mpc_collect(MpcCollectArgs {
                eval_path,
                data_root,
                split,
                mode,
                levels,
                empties,
                probe_depths,
                probe_levels,
                positions_per_bucket,
                out,
                seed,
            }),
        };
    }

    let level = args.level as i32;
    let eval_path = args
        .eval_path
        .as_deref()
        .unwrap_or("../data/eval/nnue-eval.bin");

    if let Some(n_games) = args.self_play {
        // 自己対戦モード
        let start_rand = args.self_play_start_rand;
        let out_path = args.self_play_out;
        run_self_play(n_games, level, start_rand, eval_path, &out_path)?;
    } else if let Some(path) = &args.solve {
        // Solveモード
        // e.g. -solve ".\problem\fforum-40-59.obf" -l 25
        println!("AI level   :  {}", args.level);
        solve(
            path,
            eval_path,
            level,
            args.ordering_eval.as_deref(),
            args.hash_mb,
        );
    } else if let Some(depth) = &args.perft {
        // Perft mode
        // e.g. --perft 11
        run_perft(*depth, args.perft_count_pass);
    } else {
        // 通常プレイモード
        let mut game = OthelloCLI::new(level, eval_path);
        game.play();
    }

    Ok(())
}
