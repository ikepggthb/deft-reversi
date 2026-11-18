mod book;
mod display;
mod game;
mod gtp;
mod perft;
mod play;
mod review;
mod self_play;
mod settings;
mod solve;
mod stop;

use clap::{Args, Parser, Subcommand};
use deft_reversi_engine::{Engine, EngineOptions, Goal, Model, SearchRequest};
use std::{error::Error, path::PathBuf, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Play, analyze and review Reversi from the terminal.
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
    /// Generate, resume, export or inspect an opening book.
    #[command(disable_help_flag = true)]
    Book(book::BookArgs),
    /// Search every position of an OBF file and print a table.
    Solve(solve::SolveArgs),
    /// Review records and compare played moves with the best moves.
    Review(review::ReviewArgs),
    /// Play the engine against itself and write the game records.
    SelfPlay(self_play::SelfPlayArgs),
    /// Count leaf positions from the initial position.
    Perft(perft::PerftArgs),
    /// Run the Go Text Protocol interface for automated matches.
    Gtp(gtp::GtpArgs),
}

/// Model and engine settings shared by the commands that search.
#[derive(Args, Debug, Clone)]
struct EngineArgs {
    /// Evaluation model in DPR2 format.
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
        let model =
            Model::from_file(&self.model).map_err(|e| format!("{}: {e}", self.model.display()))?;
        self.with_model(model)
    }

    fn with_model(&self, model: Model) -> Result<Engine> {
        let threads = self
            .threads
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
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
    /// Compact eval search settings: DEPTH[@PERCENT], e.g. 12@83.
    #[arg(long, conflicts_with_all = ["depth", "exact", "eval_depth", "eval_mpc"], value_parser = settings::eval_spec)]
    eval: Option<settings::EvalSpec>,
    /// Override the eval search depth. Without --level, custom plans start at depth 10, MPC 100%, endgame off.
    #[arg(long, conflicts_with_all = ["depth", "exact"], value_parser = clap::value_parser!(u8).range(0..=60))]
    eval_depth: Option<u8>,
    /// Eval search MPC: 83, 85, 95, 98, 99 or 100 (no pruning).
    #[arg(long, conflicts_with_all = ["depth", "exact"], value_parser = settings::selectivity)]
    eval_mpc: Option<deft_reversi_engine::Selectivity>,
    /// Endgame stage PERCENT:EMPTIES. Repeat or separate by commas. Zero disables a stage.
    #[arg(long, conflicts_with_all = ["depth", "exact"], value_delimiter = ',', value_parser = settings::endgame_stage)]
    endgame: Vec<settings::EndgameStage>,
    /// Time limit per search in milliseconds; 0 means unlimited.
    #[arg(long)]
    time_ms: Option<u64>,
}

impl GoalArgs {
    fn goal(&self, default: Goal) -> Goal {
        if self.eval.is_some()
            || self.eval_depth.is_some()
            || self.eval_mpc.is_some()
            || !self.endgame.is_empty()
        {
            let mut plan = self
                .level
                .map(|n| settings::plan(Goal::Level(n)))
                .unwrap_or_else(|| {
                    deft_reversi_engine::SearchPlan::midgame(
                        10,
                        deft_reversi_engine::Selectivity::Full,
                    )
                });
            plan = plan.with_midgame(
                self.eval
                    .as_ref()
                    .map(|e| e.depth)
                    .or(self.eval_depth)
                    .unwrap_or(plan.midgame_depth()),
                self.eval
                    .as_ref()
                    .and_then(|e| e.mpc)
                    .or(self.eval_mpc)
                    .unwrap_or(plan.midgame_selectivity()),
            );
            for stage in &self.endgame {
                plan = plan.endgame(stage.selectivity, stage.empties);
            }
            return Goal::Plan(plan);
        }
        match (self.level, self.depth, self.exact) {
            (Some(level), _, _) => Goal::Level(level),
            (_, Some(depth), _) => Goal::Depth(depth),
            (_, _, true) => Goal::Endgame,
            _ => default,
        }
    }

    fn time_limit(&self) -> Option<Duration> {
        self.time_ms.filter(|&ms| ms > 0).map(Duration::from_millis)
    }

    fn request(&self, goal: Goal) -> SearchRequest {
        let request = SearchRequest::new(goal);
        match self.time_ms {
            Some(ms) if ms > 0 => request.time_limit(Duration::from_millis(ms)),
            _ => request,
        }
    }
}

#[derive(Parser)]
struct SearchCommand {
    #[command(flatten)]
    goal: GoalArgs,
}

// The old --solve FILE spelling remains an entry point to the same solve command.
fn normalize_args(mut args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let known = [
        "play",
        "book",
        "solve",
        "review",
        "self-play",
        "perft",
        "gtp",
        "help",
    ];
    if args
        .get(1)
        .and_then(|s| s.to_str())
        .is_some_and(|s| known.contains(&s))
    {
        return args;
    }
    if let Some(i) = args.iter().position(|s| s == "--solve" || s == "-s") {
        if i + 1 < args.len() {
            let file = args.remove(i + 1);
            args.remove(i);
            args.insert(1, "solve".into());
            args.insert(2, file);
        }
    } else if let Some((i, file)) = args.iter().enumerate().find_map(|(i, s)| {
        s.to_str()?
            .strip_prefix("--solve=")
            .map(|f| (i, f.to_string()))
    }) {
        args.remove(i);
        args.insert(1, "solve".into());
        args.insert(2, file.into());
    }
    args
}

fn main() {
    stop::install();
    let result = match Cli::parse_from(normalize_args(std::env::args_os().collect())).command {
        Command::Play(args) => play::run(args),
        Command::Book(args) => book::run(args),
        Command::Solve(args) => solve::run(args),
        Command::Review(args) => review::run(args),
        Command::SelfPlay(args) => self_play::run(args),
        Command::Gtp(args) => gtp::run(args),
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

    #[test]
    fn custom_search_plans_parse_and_override_levels() {
        let cli = Cli::try_parse_from([
            "cli",
            "solve",
            "a.obf",
            "-m",
            "m",
            "--eval-depth",
            "12",
            "--eval-mpc",
            "83",
            "--endgame",
            "95:24,99:22,100:18",
        ])
        .unwrap();
        let Command::Solve(args) = cli.command else {
            unreachable!()
        };
        let p = settings::plan(args.goal.goal(Goal::Endgame));
        assert_eq!(p.midgame_depth(), 12);
        assert_eq!(p.midgame_selectivity().percent(), 83);
        assert_eq!(
            p.endgame_limit(deft_reversi_engine::Selectivity::Percent95),
            24
        );
        assert_eq!(p.endgame_limit(deft_reversi_engine::Selectivity::Full), 18);
        let cli = Cli::try_parse_from([
            "cli",
            "play",
            "-m",
            "m",
            "--level",
            "24",
            "--eval-depth",
            "7",
            "--endgame",
            "100:0",
        ])
        .unwrap();
        let Command::Play(args) = cli.command else {
            unreachable!()
        };
        let p = settings::plan(args.goal.goal(Goal::Level(10)));
        assert_eq!(p.midgame_depth(), 7);
        assert_eq!(p.endgame_limit(deft_reversi_engine::Selectivity::Full), 0);
        assert_eq!(
            p.endgame_limit(deft_reversi_engine::Selectivity::Percent95),
            settings::plan(Goal::Level(24))
                .endgame_limit(deft_reversi_engine::Selectivity::Percent95)
        );
        for flags in [
            vec!["--eval-mpc", "94"],
            vec!["--endgame", "99:61"],
            vec!["--endgame", "95"],
            vec!["--exact", "--eval-depth", "1"],
            vec!["--depth", "1", "--endgame", "100:4"],
        ] {
            let mut argv = vec!["cli", "play", "-m", "m"];
            argv.extend(flags);
            assert!(Cli::try_parse_from(argv).is_err());
        }
    }

    #[test]
    fn compact_and_legacy_solve_settings_match() {
        let argv = normalize_args(
            [
                "cli",
                "--model",
                "m",
                "--solve",
                "a.obf",
                "--eval",
                "12@83",
                "--endgame",
                "95:24,100:18",
                "--time-ms",
                "0",
            ]
            .into_iter()
            .map(Into::into)
            .collect(),
        );
        let Command::Solve(args) = Cli::try_parse_from(argv).unwrap().command else {
            panic!()
        };
        let p = settings::plan(args.goal.goal(Goal::Endgame));
        assert_eq!(p.midgame_depth(), 12);
        assert_eq!(p.midgame_selectivity().percent(), 83);
        assert_eq!(p.endgame_limit(deft_reversi_engine::Selectivity::Full), 18);
        assert_eq!(args.goal.time_limit(), None);
        for flags in [
            ["--eval", "12@94"],
            ["--eval", "61@100"],
            ["--eval", "12@83@95"],
        ] {
            assert!(
                Cli::try_parse_from(["cli", "play", "-m", "m"].into_iter().chain(flags)).is_err()
            );
        }
        let argv = normalize_args(
            ["cli", "--solve=a.obf", "-m", "m"]
                .into_iter()
                .map(Into::into)
                .collect(),
        );
        assert!(matches!(
            Cli::try_parse_from(argv).unwrap().command,
            Command::Solve(_)
        ));
    }

    impl Cli {
        fn command_check() {
            use clap::CommandFactory;
            Self::command().debug_assert();
        }
    }
}
