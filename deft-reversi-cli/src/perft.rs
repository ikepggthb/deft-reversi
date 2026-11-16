use crate::display;
use clap::Args;
use deft_reversi_engine::{Action, Position};
use std::time::Instant;

#[derive(Args, Debug)]
pub struct PerftArgs {
    /// Depth in plies.
    depth: u8,
    /// Count a forced pass as a ply and a leaf, as the engine's perft does.
    #[arg(long)]
    count_pass: bool,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Counts {
    pub leaves: u64,
    pub passes: u64,
    /// Finished games before the last ply; each is also a leaf.
    pub ends: u64,
}

pub fn count(position: Position, depth: u8, count_pass: bool, counts: &mut Counts) {
    if depth == 0 {
        counts.leaves += 1;
    } else if position.is_terminal() {
        counts.leaves += 1;
        counts.ends += 1;
    } else if position.must_pass() {
        counts.passes += 1;
        let next = position.play(Action::Pass).unwrap();
        count(next, depth - u8::from(count_pass), count_pass, counts);
    } else {
        for square in position.legal_moves() {
            count(
                position.play(square).unwrap(),
                depth - 1,
                count_pass,
                counts,
            );
        }
    }
}

pub fn run(args: PerftArgs) {
    println!(
        "{:>5} | {:>14} | {:>10} | {:>10} | {:>12}",
        "depth", "leaves", "passes", "ends", "time"
    );
    let total = Instant::now();
    for depth in 1..=args.depth {
        let mut counts = Counts::default();
        count(Position::initial(), depth, args.count_pass, &mut counts);
        println!(
            "{depth:>5} | {:>14} | {:>10} | {:>10} | {:>12}",
            counts.leaves,
            counts.passes,
            counts.ends,
            display::duration(total.elapsed())
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Board, perft};

    fn leaves(depth: u8, count_pass: bool) -> u64 {
        let mut counts = Counts::default();
        count(Position::initial(), depth, count_pass, &mut counts);
        counts.leaves
    }

    #[test]
    fn known_counts() {
        let expected = [1, 4, 12, 56, 244, 1396, 8200, 55092, 390216];
        for (depth, n) in expected.into_iter().enumerate() {
            assert_eq!(leaves(depth as u8, false), n);
        }
    }

    #[test]
    fn counting_passes_matches_the_engine() {
        for depth in 0..=9 {
            assert_eq!(leaves(depth, true), perft(Board::new(), depth));
        }
    }
}
