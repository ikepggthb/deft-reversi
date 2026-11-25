//! 全フェーズの学習（`train`）と、チェックポイントからのモデル書き出し（`export`）を行うモジュール。
//!
//! 石数ごとの各フェーズを並列に学習し、1 エポック終えるごとにチェックポイント（`phase_<stones>.ckpt`）と
//! 学習ログ（`metrics.jsonl`）を保存する。これにより、途中で中断しても途中から再開（resume）したり、
//! 完了後にエポック数を増やして追加学習したりできる。

use crate::{
    Result, StoneRange, for_each_parallel,
    pen::{PenSettings, PenWeights},
    phase::{Metrics, PhaseWeights, Settings},
    rd,
};
use clap::Args;
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

/// `train` サブコマンドの CLI 引数。
#[derive(Args, Debug)]
pub struct TrainArgs {
    /// `phase_<NN>/{train,valid}.rd` を含む rdg データセットディレクトリ。
    #[arg(long)]
    data: PathBuf,
    /// チェックポイントおよび `metrics.jsonl` の保存先。中断時はここから再開する。
    #[arg(long)]
    work: PathBuf,
    /// 全フェーズ完了時に書き出す出力モデルパス（DPR2）。
    #[arg(long)]
    out: PathBuf,
    /// 学習対象の石数範囲（両端含む）。
    #[arg(long, default_value = "12..=63")]
    stones: StoneRange,
    /// 各フェーズの学習エポック数。完了済みランで増やすとチェックポイントから追加学習する。
    #[arg(long, default_value_t = 4)]
    epochs: u32,
    /// 各石数あたりの学習局面数上限（ランダム抽出）。
    #[arg(long)]
    limit: Option<u64>,
    /// 線形重み（AdaGrad）の学習率。
    #[arg(long, default_value_t = 0.05)]
    lr: f32,
    /// 線形重みの L2 正則化係数。
    #[arg(long, default_value_t = 1e-4)]
    l2: f32,
    /// Huber 損失の閾値（石数）。
    #[arg(long, default_value_t = 4.0)]
    huber_delta: f32,
    /// 出現回数の少ない重みを 0 へ寄せて過学習を抑える係数（詳細は `export --shrink` を参照）。
    #[arg(long, default_value_t = 32.0)]
    shrink: f32,
    /// `shrink`（重みの縮小）を適用しない石数範囲（詳細は `export --no-shrink-stones` を参照）。
    #[arg(long)]
    no_shrink_stones: Option<StoneRange>,
    /// 乱数シード。
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// パターン状態とその鏡像での重み共有を無効化する。
    /// 既定では線形重みおよび `--pen` の埋め込みを鏡像と共有する。
    #[arg(long)]
    no_mirror: bool,
    /// 線形パターン評価を補う小型ニューラルネットを追加して同時学習する。
    #[arg(long)]
    pen: bool,
    /// `--pen` 指定時に線形重みを学習せず、小型ニューラルネット単体で局面を学習する（出力の線形テーブルは 0 になる）。
    #[arg(long, requires = "pen")]
    pen_no_linear: bool,
    /// 補助ニューラルネットのパターン埋め込み（AdaGrad）の学習率。
    #[arg(long, default_value_t = 0.01)]
    pen_lr: f32,
    /// 補助ニューラルネットの全結合層（Adam）の学習率。
    #[arg(long, default_value_t = 0.001)]
    pen_dense_lr: f32,
    /// 補助ニューラルネットの全結合層を Adam で 1 回更新する際のミニバッチサイズ。
    #[arg(long, default_value_t = 256)]
    pen_batch: u32,
    /// 補助ニューラルネットの全結合重みに掛ける Decoupled Weight Decay（重み減衰）。既定は 0
    /// （オフセット中心化後の実験で 0.01 / 0.1 はテスト誤差をそれぞれ 0.005 / 0.05 石悪化させたため）。
    #[arg(long, default_value_t = 0.0)]
    pen_decay: f32,
    /// 同時に学習するフェーズ数（スレッド数）。各フェーズは学習データをメモリに展開するため注意
    /// （1 局面 24 バイト、最大フェーズで約 1.2 GB）。
    #[arg(short, long)]
    threads: Option<usize>,
}

/// `export` サブコマンドの CLI 引数。
#[derive(Args, Debug)]
pub struct ExportArgs {
    /// チェックポイントが保存された作業ディレクトリ。
    #[arg(long)]
    work: PathBuf,
    /// 出力モデルファイルのパス（DPR2 形式）。
    #[arg(long)]
    out: PathBuf,
    /// 出現回数の少ない重みを 0（またはテーブル平均）へ寄せて過学習を防ぐ縮小係数：
    /// 出現回数 n の重みに `n / (n + shrink)` を掛ける。
    #[arg(long, default_value_t = 32.0)]
    shrink: f32,
    /// 縮小（`shrink`）を行わずに出力する石数範囲。
    /// 全局面が網羅されている序盤フェーズ（Egaroucid の列挙序盤など）では、
    /// 出現回数の少ない重みも他と同じく信頼できるため指定する。
    #[arg(long)]
    no_shrink_stones: Option<StoneRange>,
}

/// チェックポイントから再開するために一致が必須となる設定群。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Identity {
    settings: Settings,
    pen: Option<PenSettings>,
    limit: Option<u64>,
    seed: u64,
}

#[derive(Clone, Serialize, Deserialize)]
enum Weights {
    Linear(PhaseWeights),
    Pen(Box<PenWeights>),
    /// 線形重みなしの PEN（`--pen-no-linear`）。古いチェックポイントのタグ互換のため末尾に配置。
    PenOnly(Box<PenWeights>),
}

impl Weights {
    fn train(&mut self, records: &[rd::Record], identity: &Identity) -> Metrics {
        match (self, &identity.pen) {
            (Self::Linear(w), _) => w.train(records, &identity.settings),
            (Self::Pen(w), Some(pen)) => w.train(records, &identity.settings, pen, true),
            (Self::PenOnly(w), Some(pen)) => w.train(records, &identity.settings, pen, false),
            (Self::Pen(_) | Self::PenOnly(_), None) => {
                unreachable!("the identity matched the checkpoint")
            }
        }
    }
    fn validate(&self, records: &[rd::Record], huber_delta: f32) -> Metrics {
        match self {
            Self::Linear(w) => w.validate(records, huber_delta),
            Self::Pen(w) | Self::PenOnly(w) => w.validate(records, huber_delta),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    version: u32,
    stones: u8,
    identity: Identity,
    data: rd::Fingerprint,
    epochs: u32,
    weights: Weights,
}
const CHECKPOINT_VERSION: u32 = 5;

#[derive(Serialize)]
struct EpochLog {
    stones: u8,
    epoch: u32,
    train: Metrics,
    valid: Option<Metrics>,
    seconds: f64,
}

/// `train` サブコマンドを実行し、対象全フェーズを並列学習したあとモデルを書き出す。
pub fn run(args: TrainArgs) -> Result<()> {
    fs::create_dir_all(&args.work)?;
    let identity = Identity {
        settings: Settings {
            lr: args.lr,
            l2: args.l2,
            huber_delta: args.huber_delta,
            mirror: !args.no_mirror,
        },
        pen: args.pen.then_some(PenSettings {
            lr: args.pen_lr,
            dense_lr: args.pen_dense_lr,
            batch: args.pen_batch,
            decay: args.pen_decay,
        }),
        limit: args.limit,
        seed: args.seed,
    };
    identity.settings.validate()?;
    if let Some(pen) = &identity.pen {
        pen.validate()?;
    }
    if args.limit == Some(0) {
        return Err("--limit must be at least 1".into());
    }
    validate_shrink(args.shrink)?;
    let stones: Vec<u8> = args.stones.iter().collect();
    for &s in &stones {
        rd::count(&rd::path(&args.data, s, "train"))?;
    }
    // レコード数が多いフェーズから順に投入し、終盤で一部スレッドだけが大きく残らないようにする。
    let mut order = stones.clone();
    order
        .sort_by_key(|&s| std::cmp::Reverse(rd::count(&rd::path(&args.data, s, "train")).unwrap()));
    let metrics = Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(args.work.join("metrics.jsonl"))?,
    );
    let threads = args
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    for_each_parallel(&order, threads, |&s| {
        train_phase(&args, identity, s, &metrics)
    })?;
    export(&ExportArgs {
        work: args.work.clone(),
        out: args.out.clone(),
        shrink: args.shrink,
        no_shrink_stones: args.no_shrink_stones,
    })
}

fn checkpoint_path(work: &Path, stones: u8) -> PathBuf {
    work.join(format!("phase_{stones}.ckpt"))
}

fn read_checkpoint(path: &Path) -> Result<Option<Checkpoint>> {
    if !path.exists() {
        return Ok(None);
    }
    let checkpoint: Checkpoint =
        bincode::deserialize(&fs::read(path)?).map_err(|e| format!("{}: {e}", path.display()))?;
    if checkpoint.version != CHECKPOINT_VERSION {
        return Err(format!("{}: unsupported checkpoint", path.display()).into());
    }
    Ok(Some(checkpoint))
}

fn train_phase(
    args: &TrainArgs,
    identity: Identity,
    stones: u8,
    metrics: &Mutex<fs::File>,
) -> Result<()> {
    let path = checkpoint_path(&args.work, stones);
    let train_path = rd::path(&args.data, stones, "train");
    let data = rd::fingerprint(&train_path)?;
    let checkpoint = read_checkpoint(&path)?;
    if let Some(c) = &checkpoint {
        if c.identity != identity || c.stones != stones {
            return Err(format!(
                "{}: trained with other settings ({:?}); use another --work",
                path.display(),
                c.identity
            )
            .into());
        }
        if matches!(c.weights, Weights::PenOnly(_)) != args.pen_no_linear {
            return Err(format!(
                "{}: trained {} the linear weights; use another --work",
                path.display(),
                if args.pen_no_linear {
                    "with"
                } else {
                    "without"
                }
            )
            .into());
        }
        if c.data != data {
            return Err(format!(
                "{}: trained on other data than {}; use another --work",
                path.display(),
                train_path.display()
            )
            .into());
        }
        if c.epochs >= args.epochs {
            println!("stones {stones}: done ({} epochs)", c.epochs);
            return Ok(());
        }
    }
    let mut records = rd::read(&train_path, stones)?;
    let mut rng = StdRng::seed_from_u64(args.seed ^ u64::from(stones) << 32);
    if let Some(limit) = args.limit {
        records.shuffle(&mut rng);
        records.truncate(limit as usize);
    }
    if records.is_empty() {
        // チェックポイントを残さないことで、export 時に近傍フェーズがコピーされるようにする。
        println!("stones {stones}: no training positions, skipped");
        return Ok(());
    }
    let valid_path = rd::path(&args.data, stones, "valid");
    let valid = if valid_path.exists() {
        rd::read(&valid_path, stones)?
    } else {
        Vec::new()
    };
    let (mut weights, done) = match checkpoint {
        Some(c) => (c.weights, c.epochs),
        None if identity.pen.is_some() => {
            let w = Box::new(PenWeights::new(
                &records,
                args.seed ^ u64::from(stones) << 40,
                identity.settings.mirror,
            ));
            let w = if args.pen_no_linear {
                Weights::PenOnly(w)
            } else {
                Weights::Pen(w)
            };
            (w, 0)
        }
        None => (
            Weights::Linear(PhaseWeights::new(&records, identity.settings.mirror)),
            0,
        ),
    };
    // 再開時にも連続実行時と完全に同じシャッフル順になるよう、完了済みエポック分の乱数を空回しする。
    for _ in 0..done {
        records.shuffle(&mut rng);
    }
    for epoch in done..args.epochs {
        let start = Instant::now();
        records.shuffle(&mut rng);
        let train = weights.train(&records, &identity);
        let valid = (!valid.is_empty()).then(|| weights.validate(&valid, args.huber_delta));
        let log = EpochLog {
            stones,
            epoch: epoch + 1,
            train,
            valid,
            seconds: start.elapsed().as_secs_f64(),
        };
        println!(
            "stones {stones:>2} epoch {}/{}  train mae {:.3}  valid mae {}  ({} positions, {:.1}s)",
            epoch + 1,
            args.epochs,
            train.mae,
            valid.map_or("-".into(), |v| format!("{:.3}", v.mae)),
            train.samples,
            log.seconds
        );
        let checkpoint = Checkpoint {
            version: CHECKPOINT_VERSION,
            stones,
            identity,
            data,
            epochs: epoch + 1,
            weights,
        };
        atomic_write(&path, &bincode::serialize(&checkpoint)?)?;
        weights = checkpoint.weights;
        writeln!(metrics.lock().unwrap(), "{}", serde_json::to_string(&log)?)?;
    }
    Ok(())
}

/// 作業ディレクトリ内の全チェックポイントを読み込み、DPR2 モデルファイルを書き出す。
pub fn export(args: &ExportArgs) -> Result<()> {
    validate_shrink(args.shrink)?;
    let mut trained = vec![None; deft_reversi_engine::PHASE_COUNT];
    let mut networks = vec![None; deft_reversi_engine::PHASE_COUNT];
    for stones in 4..=63u8 {
        if let Some(c) = read_checkpoint(&checkpoint_path(&args.work, stones))? {
            let p = usize::from(stones) - 4;
            let shrink = if args.no_shrink_stones.is_some_and(|r| r.contains(stones)) {
                0.0
            } else {
                args.shrink
            };
            match c.weights {
                Weights::Linear(w) => trained[p] = Some(w.export(shrink)),
                Weights::Pen(w) | Weights::PenOnly(w) => {
                    let (linear, network) = w.export(shrink);
                    trained[p] = Some(linear);
                    networks[p] = Some(network);
                }
            }
        }
    }
    let filled: Vec<usize> = (0..trained.len())
        .filter(|&p| trained[p].is_none())
        .collect();
    let bytes = if networks.iter().any(Option::is_some) {
        if networks
            .iter()
            .zip(&trained)
            .any(|(n, t)| n.is_some() != t.is_some())
        {
            return Err("a work directory mixes linear and PEN checkpoints".into());
        }
        crate::model::assemble_pen(&trained, &networks)?
    } else {
        crate::model::assemble(&mut |p| trained[p].take())?
    };
    atomic_write(&args.out, &bytes)?;
    if !filled.is_empty() {
        println!(
            "phases {} have no checkpoint and copy the nearest trained phase",
            ranges(&filled)
        );
    }
    println!("wrote {}", args.out.display());
    Ok(())
}

fn validate_shrink(shrink: f32) -> Result<()> {
    if !(shrink.is_finite() && shrink >= 0.0) {
        return Err(format!("--shrink must be nonnegative: {shrink}").into());
    }
    Ok(())
}

fn ranges(phases: &[usize]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < phases.len() {
        let mut j = i;
        while j + 1 < phases.len() && phases[j + 1] == phases[j] + 1 {
            j += 1;
        }
        parts.push(if i == j {
            phases[i].to_string()
        } else {
            format!("{}-{}", phases[i], phases[j])
        });
        i = j + 1;
    }
    parts.join(", ")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Model, PatternModel};

    #[test]
    fn training_resumes_and_exports_a_loadable_model() {
        let dir = crate::test_dir("train");
        let data = dir.join("data");
        let mut random = StdRng::seed_from_u64(5);
        for stones in [20u8, 21] {
            let records = crate::test_records(&mut random, stones, 300);
            rd::write(&rd::path(&data, stones, "train"), &records[..250]);
            rd::write(&rd::path(&data, stones, "valid"), &records[250..]);
        }
        let args = |epochs| TrainArgs {
            data: data.clone(),
            work: dir.join("work"),
            out: dir.join("model.dpr2"),
            stones: "20..=21".parse().unwrap(),
            epochs,
            limit: Some(200),
            lr: 0.05,
            l2: 1e-4,
            huber_delta: 4.0,
            shrink: 32.0,
            no_shrink_stones: None,
            seed: 1,
            no_mirror: false,
            pen: false,
            pen_no_linear: false,
            pen_lr: 0.01,
            pen_dense_lr: 0.001,
            pen_batch: 256,
            pen_decay: 0.01,
            threads: Some(2),
        };
        run(args(1)).unwrap();
        run(args(3)).unwrap();
        let log = fs::read_to_string(dir.join("work/metrics.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 6);
        let model = fs::read(dir.join("model.dpr2")).unwrap();
        Model::from_bytes(&model).unwrap();
        PatternModel::read_from(&model[..]).unwrap();

        // Resuming after one epoch gives the same model as three at once.
        let mut straight = args(3);
        straight.work = dir.join("straight");
        straight.out = dir.join("straight.dpr2");
        run(straight).unwrap();
        assert_eq!(fs::read(dir.join("straight.dpr2")).unwrap(), model);

        let mut other = args(4);
        other.lr = 0.1;
        assert!(run(other).is_err());
        let mut zero = args(4);
        zero.limit = Some(0);
        assert!(run(zero).is_err());
        let replaced = crate::test_records(&mut random, 20, 250);
        rd::write(&rd::path(&data, 20, "train"), &replaced);
        assert!(run(args(3)).is_err());

        // Phases in --no-shrink-stones export as with --shrink 0.
        let export_to = |out: &str, shrink, no_shrink_stones| {
            export(&ExportArgs {
                work: dir.join("work"),
                out: dir.join(out),
                shrink,
                no_shrink_stones,
            })
            .unwrap();
            fs::read(dir.join(out)).unwrap()
        };
        let plain = export_to("k0.dpr2", 0.0, None);
        assert_ne!(export_to("k1000.dpr2", 1000.0, None), plain);
        assert_eq!(
            export_to("k1000-none.dpr2", 1000.0, Some("20..=21".parse().unwrap())),
            plain
        );

        // So does a PEN run, with a partial last batch and decay.
        let pen = |epochs, work: &str| TrainArgs {
            work: dir.join(work),
            out: dir.join(format!("{work}.dpr2")),
            pen: true,
            pen_batch: 7,
            pen_decay: 0.01,
            ..args(epochs)
        };
        run(pen(1, "pen")).unwrap();
        run(pen(2, "pen")).unwrap();
        run(pen(2, "pen-straight")).unwrap();
        assert_eq!(
            fs::read(dir.join("pen.dpr2")).unwrap(),
            fs::read(dir.join("pen-straight.dpr2")).unwrap()
        );

        // A phase without positions leaves no checkpoint behind.
        rd::write(&rd::path(&data, 22, "train"), &[]);
        let mut empty = args(1);
        empty.work = dir.join("empty");
        empty.stones = "22".parse().unwrap();
        assert!(run(empty).is_err());
        assert!(!checkpoint_path(&dir.join("empty"), 22).exists());
        assert_eq!(ranges(&[0, 1, 2, 5, 7, 8]), "0-2, 5, 7-8");
        fs::remove_dir_all(dir).unwrap();
    }
}
