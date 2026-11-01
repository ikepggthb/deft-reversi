mod eval;
mod model;
mod mpc;
mod phase;
mod rd;
mod train;

use clap::{Parser, Subcommand};
use std::{error::Error, str::FromStr, sync::Mutex};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Trains the engine's pattern evaluation from `.rd` datasets.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Train every phase and write the model.
    Train(train::TrainArgs),
    /// Write the model from the checkpoints of a training run.
    Export(train::ExportArgs),
    /// Compare models by their error on a dataset split.
    Eval(eval::EvalArgs),
    /// Calibrate ProbCut (MPC) for a model.
    Mpc(mpc::MpcArgs),
    /// Write a model with a ProbCut calibration in one file.
    Pack(mpc::PackArgs),
}

fn main() {
    let result = match Cli::parse().command {
        Command::Train(args) => train::run(args),
        Command::Export(args) => train::export(&args),
        Command::Eval(args) => eval::run(args),
        Command::Mpc(args) => mpc::run(args),
        Command::Pack(args) => mpc::pack(args),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Disc counts `a..=b`, written as `a..=b` or `a..b` (both inclusive, as the
/// dataset directories are named), or `a`.
#[derive(Clone, Copy, Debug)]
pub struct StoneRange {
    first: u8,
    last: u8,
}

impl StoneRange {
    pub fn iter(self) -> impl Iterator<Item = u8> {
        self.first..=self.last
    }
}

impl FromStr for StoneRange {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let (a, b) = match s.split_once("..") {
            Some((a, b)) => (a, b.strip_prefix('=').unwrap_or(b)),
            None => (s, s),
        };
        let parse = |x: &str| {
            x.trim()
                .parse::<u8>()
                .map_err(|_| format!("bad disc count `{x}`"))
        };
        let (first, last) = (parse(a)?, parse(b)?);
        // 64 discs would share phase 59 with 63.
        if !(4..=63).contains(&first) || !(first..=63).contains(&last) {
            return Err(format!("disc counts must be within 4..=63: {s}"));
        }
        Ok(Self { first, last })
    }
}

/// Runs `f` on every item with up to `threads` threads, taking items in
/// order. Stops at the first error and returns it.
pub fn for_each_parallel<T: Sync>(
    items: &[T],
    threads: usize,
    f: impl Fn(&T) -> Result<()> + Sync,
) -> Result<()> {
    let next = Mutex::new(0usize);
    let failure = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = {
                        let mut next = next.lock().unwrap();
                        if failure.lock().unwrap().is_some() || *next == items.len() {
                            return;
                        }
                        *next += 1;
                        *next - 1
                    };
                    if let Err(e) = f(&items[i]) {
                        failure.lock().unwrap().get_or_insert(e);
                    }
                }
            });
        }
    });
    match failure.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("deft-train-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Positions with `stones` discs from random games, labelled by a simple
/// function of the board so training has something to fit.
#[cfg(test)]
fn test_records(random: &mut impl rand::Rng, stones: u8, n: usize) -> Vec<rd::Record> {
    use deft_reversi_engine::{Action, Board};
    let mut records = Vec::new();
    while records.len() < n {
        let mut board = Board::new();
        while 64 - board.empty_count() < stones && !board.is_terminal() {
            let moves: Vec<_> = board.legal_moves().into_iter().collect();
            board = if moves.is_empty() {
                board.play(Action::Pass).unwrap()
            } else {
                board
                    .play(moves[random.gen_range(0..moves.len())].into())
                    .unwrap()
            };
        }
        if 64 - board.empty_count() == stones {
            let corners = |b: u64| (b & 0x8100_0000_0000_0081).count_ones() as i8;
            let value = 2 * (corners(board.player_bits()) - corners(board.opponent_bits()))
                + board.legal_moves().len() as i8 / 2;
            records.push(rd::Record { board, value });
        }
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stone_ranges() {
        let r: StoneRange = "12..63".parse().unwrap();
        assert_eq!(r.iter().count(), 52);
        assert_eq!("20..=21".parse::<StoneRange>().unwrap().iter().count(), 2);
        assert_eq!("30".parse::<StoneRange>().unwrap().iter().count(), 1);
        assert!("12..64".parse::<StoneRange>().is_err());
        assert!("40..30".parse::<StoneRange>().is_err());
    }

    #[test]
    fn parallel_runs_every_item_and_reports_errors() {
        let seen = Mutex::new(Vec::new());
        for_each_parallel(&[1, 2, 3, 4, 5], 3, |&i| {
            seen.lock().unwrap().push(i);
            Ok(())
        })
        .unwrap();
        let mut seen = seen.into_inner().unwrap();
        seen.sort();
        assert_eq!(seen, [1, 2, 3, 4, 5]);
        assert!(
            for_each_parallel(&[1, 2], 2, |&i| if i == 2 {
                Err("x".into())
            } else {
                Ok(())
            })
            .is_err()
        );
    }
}
