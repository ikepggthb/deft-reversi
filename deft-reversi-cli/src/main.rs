mod display;
mod game;
mod perft;
mod play;
mod self_play;
mod solve;
mod stop;

use clap::{Args, Parser, Subcommand};
use deft_reversi_engine::{Engine, EngineOptions, Goal, Model, SearchRequest};
use std::{error::Error, path::PathBuf, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Reversi from the terminal: play, solve positions, self-play and perft.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Play against the engine.
    Play(play::PlayArgs),
    /// Search every position of an OBF file and print a table.
    Solve(solve::SolveArgs),
    /// Play the engine against itself and write the game records.
    SelfPlay(self_play::SelfPlayArgs),
    /// Count leaf positions from the initial position.
    Perft(perft::PerftArgs),
}

/// Model and engine settings shared by the commands that search.
#[derive(Args, Debug, Clone)]
struct EngineArgs {
    /// Evaluation model (v3/v4 JSON or bincode, or DPR2).
    #[arg(short, long, env = "DEFT_MODEL")]
    model: PathBuf,
    /// Search threads, including the calling thread. Defaults to every core.
    #[arg(short, long)]
    threads: Option<usize>,
    /// Transposition table size in MiB.
    #[arg(long, default_value_t = 256)]
    tt_mib: usize,
}

impl EngineArgs {
    fn engine(&self) -> Result<Engine> {
        let threads = self
            .threads
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
        let model =
            Model::from_file(&self.model).map_err(|e| format!("{}: {e}", self.model.display()))?;
        let options = EngineOptions::default()
            .threads(threads)
            .tt_mib(self.tt_mib);
        Ok(Engine::new(model, options)?)
    }
}

/// How far each search reads.
#[derive(Args, Debug, Clone)]
struct GoalArgs {
    /// Engine level, 1-60.
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(1..=60))]
    level: Option<u8>,
    /// Fixed depth, 0-60.
    #[arg(short, long, conflicts_with = "level", value_parser = clap::value_parser!(u8).range(0..=60))]
    depth: Option<u8>,
    /// Read to the end of the game without pruning.
    #[arg(long, conflicts_with_all = ["level", "depth"])]
    exact: bool,
    /// Time limit per search, in milliseconds.
    #[arg(long)]
    time_ms: Option<u64>,
}

impl GoalArgs {
    fn goal(&self, default: Goal) -> Goal {
        match (self.level, self.depth, self.exact) {
            (Some(level), _, _) => Goal::Level(level),
            (_, Some(depth), _) => Goal::Depth(depth),
            (_, _, true) => Goal::Endgame,
            _ => default,
        }
    }

    fn request(&self, goal: Goal) -> SearchRequest {
        let request = SearchRequest::new(goal);
        match self.time_ms {
            Some(ms) => request.time_limit(Duration::from_millis(ms)),
            None => request,
        }
    }
}

fn main() {
    stop::install();
    let result = match Cli::parse().command {
        Command::Play(args) => play::run(args),
        Command::Solve(args) => solve::run(args),
        Command::SelfPlay(args) => self_play::run(args),
        Command::Perft(args) => {
            perft::run(args);
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_parse() {
        Cli::command_check();
        let cli = Cli::try_parse_from(["cli", "solve", "a.obf", "-m", "m.bin", "--exact"]).unwrap();
        assert!(matches!(cli.command, Command::Solve(_)));
        assert!(
            Cli::try_parse_from(["cli", "solve", "a.obf", "-m", "m", "-l", "3", "-d", "4"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["cli", "play", "-m", "m", "-l", "61"]).is_err());
        let cli = Cli::try_parse_from(["cli", "perft", "9", "--count-pass"]).unwrap();
        assert!(matches!(cli.command, Command::Perft(_)));
    }

    impl Cli {
        fn command_check() {
            use clap::CommandFactory;
            Self::command().debug_assert();
        }
    }
}
