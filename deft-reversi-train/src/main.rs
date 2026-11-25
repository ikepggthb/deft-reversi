//! リバーシエンジンの評価関数モデルおよび ProbCut（MPC）パラメータを学習・評価・変換する CLI バイナリ。
//!
//! 棋譜や局面テキストから学習用データセット（`.rd`）を作成する `convert`、
//! 盤面パターン重みや補助ニューラルネットを学習する `train`、
//! チェックポイントからモデルを書き出す `export`、モデルの精度を比較する `eval`、
//! 枝刈り用の ProbCut パラメータを調整する `mpc`、
//! モデルと校正パラメータを 1 ファイルにまとめる `pack` の各サブコマンドを提供します。

mod convert;
mod eval;
mod model;
mod mpc;
mod pen;
mod phase;
mod rd;
mod train;

use clap::{Parser, Subcommand};
use std::{error::Error, str::FromStr, sync::Mutex};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const VERSION: &str = env!("CARGO_PKG_VERSION");
static BUILD_VERSION: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("{VERSION} (git {})", deft_reversi_engine::GIT_REVISION));

/// 学習用データセット（`.rd`）からエンジンの評価関数を学習・評価・校正する CLI。
#[derive(Parser, Debug)]
#[command(author, version, long_version = BUILD_VERSION.as_str(), about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 棋譜または局面テキストから `.rd` データセットを作成する。
    Convert(convert::ConvertArgs),
    /// 全フェーズを学習してモデルファイルを書き出す。
    Train(train::TrainArgs),
    /// 学習途中のチェックポイントからモデルファイルを書き出す。
    Export(train::ExportArgs),
    /// データセットの評価誤差（MAE）を計測してモデル同士を比較する。
    Eval(eval::EvalArgs),
    /// モデルの枝刈り用 ProbCut（MPC）パラメータを調整（校正）する。
    Mpc(mpc::MpcArgs),
    /// モデルと ProbCut 校正パラメータを 1 ファイルにまとめて書き出す。
    Pack(mpc::PackArgs),
}

fn main() {
    let result = match Cli::parse().command {
        Command::Convert(args) => convert::run(args),
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

/// 対象とする石数の範囲（`a..=b`、データセットディレクトリ名に合わせた両端含む `a..b`、または単一値 `a`）。
#[derive(Clone, Copy, Debug)]
pub struct StoneRange {
    first: u8,
    last: u8,
}

impl StoneRange {
    /// 範囲内の石数を昇順に反復するイテレータを返す。
    pub fn iter(self) -> impl Iterator<Item = u8> {
        self.first..=self.last
    }
    /// 指定した石数 `stones` が範囲に含まれるか判定する。
    pub fn contains(self, stones: u8) -> bool {
        (self.first..=self.last).contains(&stones)
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
        // 64 石の盤面は空きマス 0 の終局であり、63 石と同じフェーズ 59 に属するため除外する。
        if !(4..=63).contains(&first) || !(first..=63).contains(&last) {
            return Err(format!("disc counts must be within 4..=63: {s}"));
        }
        Ok(Self { first, last })
    }
}

/// `items` の各要素に対して最大 `threads` スレッドで `f` を並列実行する。
/// いずれかの要素でエラーが起きた場合は最初のエラーを返し、残りの要素は処理しない。
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
