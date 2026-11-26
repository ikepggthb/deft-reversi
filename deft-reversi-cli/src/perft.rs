//! 初期局面から指定手数までの合法手をすべて展開し、末端局面数を数える `perft` コマンド。
//!
//! エンジンの着手生成と局面遷移が正しいかを検証したり、探索速度の目安を測ったりするために使う。

use crate::display;
use clap::Args;
use deft_reversi_engine::{Action, Position};
use std::time::Instant;

/// `perft` サブコマンドの引数。
#[derive(Args, Debug)]
pub struct PerftArgs {
    /// 展開する深さ（手数＝ply）。
    depth: u8,
    /// 強制パスも 1 手および 1 末端として数える（エンジンの perft と同じ数え方）。
    #[arg(long)]
    count_pass: bool,
}

/// `perft` の計測結果。
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Counts {
    /// 指定深さに到達した局面数と、その前に終局した局面数の合計。
    pub leaves: u64,
    /// 展開中に発生した強制パスの回数。
    pub passes: u64,
    /// 指定深さに達する前に終局した局面数（それぞれ `leaves` にも含まれる）。
    pub ends: u64,
}

/// `position` から残り `depth` 手まで合法手を展開し、結果を `counts` に加算する。
pub fn count(position: Position, depth: u8, count_pass: bool, counts: &mut Counts) {
    if depth == 0 {
        counts.leaves += 1;
    } else if position.is_terminal() {
        counts.leaves += 1;
        counts.ends += 1;
    } else if position.must_pass() {
        counts.passes += 1;
        let next = position.play(Action::Pass).unwrap();
        // `count_pass` が真のときだけ強制パスで残り手数を 1 減らす。
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

/// 深さ 1 から `args.depth` まで順に `perft` を実行し、結果を表形式で標準出力へ表示する。
pub fn run(args: PerftArgs) {
    eprintln!("{}", crate::settings::versions());
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
