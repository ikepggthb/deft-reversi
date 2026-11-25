//! データセットの指定分割（`test` など）を使って、複数モデルの評価誤差（MAE）を計測・比較するサブコマンド。
//!
//! モデルの予測スコアと実際の終局石差が平均して何石ずれているかを石数ごとに表示する。

use crate::{Result, StoneRange, for_each_parallel, rd};
use clap::Args;
use deft_reversi_engine::{Model, PatternModel};
use std::{path::PathBuf, sync::Mutex};

/// `eval` サブコマンドの CLI 引数。
#[derive(Args, Debug)]
pub struct EvalArgs {
    /// 比較対象のモデルファイル（エンジンが読み込める任意の形式）。
    #[arg(short, long, required = true)]
    model: Vec<PathBuf>,
    /// `.rd` データセットのルートディレクトリ。
    #[arg(long)]
    data: PathBuf,
    /// 評価に使うデータ分割名（`train` / `valid` / `test`）。
    #[arg(long, default_value = "test")]
    split: String,
    /// 評価対象とする石数の範囲（両端含む）。
    #[arg(long, default_value = "12..=63")]
    stones: StoneRange,
    /// 各石数ファイルから先頭最大この件数だけを評価する。
    #[arg(long)]
    limit: Option<usize>,
    /// 並列実行するスレッド数。
    #[arg(short, long)]
    threads: Option<usize>,
}

/// 各石数と各モデルについて、予測スコアと正解ラベルの平均絶対誤差（MAE、石単位）を計算して表示する。
pub fn run(args: EvalArgs) -> Result<()> {
    let models = args
        .model
        .iter()
        .map(|path| {
            Model::weights_from_file(path)
                .map(|m| m.pattern().clone())
                .map_err(|e| format!("{}: {e}", path.display()).into())
        })
        .collect::<Result<Vec<PatternModel>>>()?;
    let stones: Vec<u8> = args.stones.iter().collect();
    let rows = Mutex::new(Vec::new());
    let threads = args
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    for_each_parallel(&stones, threads, |&s| {
        let mut records = rd::read(&rd::path(&args.data, s, &args.split), s)?;
        records.truncate(args.limit.unwrap_or(usize::MAX));
        let errors = models.iter().map(|m| mae(m, &records)).collect::<Vec<_>>();
        rows.lock().unwrap().push((s, records.len(), errors));
        Ok(())
    })?;
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by_key(|r| r.0);

    print!("{:>6} {:>10}", "stones", "positions");
    for i in 0..models.len() {
        print!(" {:>9}", format!("model {}", i + 1));
    }
    println!();
    for (s, n, errors) in &rows {
        print!("{s:>6} {n:>10}");
        for e in errors {
            print!(" {e:>9.3}");
        }
        println!();
    }
    let total: usize = rows.iter().map(|r| r.1).sum();
    print!("{:>6} {total:>10}", "mean");
    for i in 0..models.len() {
        let mean = rows.iter().map(|r| r.2[i]).sum::<f64>() / rows.len().max(1) as f64;
        print!(" {mean:>9.3}");
    }
    println!();
    for (i, path) in args.model.iter().enumerate() {
        println!("model {}: {}", i + 1, path.display());
    }
    Ok(())
}

fn mae(model: &PatternModel, records: &[rd::Record]) -> f64 {
    records
        .iter()
        .map(|r| f64::from((model.evaluate(&r.board) - i32::from(r.value)).abs()))
        .sum::<f64>()
        / records.len().max(1) as f64
}
