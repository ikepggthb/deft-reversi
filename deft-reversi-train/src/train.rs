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

#[derive(Args, Debug)]
pub struct TrainArgs {
    /// rdg dataset directory with `phase_<NN>/{train,valid}.rd`.
    #[arg(long)]
    data: PathBuf,
    /// Checkpoints and metrics; an interrupted run resumes from here.
    #[arg(long)]
    work: PathBuf,
    /// The model to write when every phase is done (DPR2).
    #[arg(long)]
    out: PathBuf,
    /// Disc counts to train, inclusive.
    #[arg(long, default_value = "12..=63")]
    stones: StoneRange,
    /// Passes over each phase's data. Raising it on a finished run trains
    /// further from the checkpoints.
    #[arg(long, default_value_t = 4)]
    epochs: u32,
    /// At most this many training positions per disc count, chosen at random.
    #[arg(long)]
    limit: Option<u64>,
    #[arg(long, default_value_t = 0.05)]
    lr: f32,
    #[arg(long, default_value_t = 1e-4)]
    l2: f32,
    #[arg(long, default_value_t = 4.0)]
    huber_delta: f32,
    /// See `export --shrink`.
    #[arg(long, default_value_t = 32.0)]
    shrink: f32,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Add a pattern embedding network (PEN) to the linear weights.
    #[arg(long)]
    pen: bool,
    /// With `--pen`, leave out the linear weights: the network alone fits
    /// the positions (the exported linear tables are zero).
    #[arg(long, requires = "pen")]
    pen_no_linear: bool,
    /// AdaGrad step size of the PEN embeddings.
    #[arg(long, default_value_t = 0.01)]
    pen_lr: f32,
    /// Adam step size of the PEN dense layers.
    #[arg(long, default_value_t = 0.001)]
    pen_dense_lr: f32,
    /// Samples per Adam step of the PEN dense layers.
    #[arg(long, default_value_t = 256)]
    pen_batch: u32,
    /// Phases trained at once. Each holds its data in memory: 24 bytes a
    /// position, up to about 1.2 GB for the largest phase of the full set.
    #[arg(short, long)]
    threads: Option<usize>,
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    #[arg(long)]
    work: PathBuf,
    #[arg(long)]
    out: PathBuf,
    /// Pulls weights seen in few positions toward 0: a weight seen n times
    /// is multiplied by n / (n + shrink).
    #[arg(long, default_value_t = 32.0)]
    shrink: f32,
}

/// What a checkpoint must match to resume.
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
    /// PEN without the linear weights (`--pen-no-linear`). Last, so older
    /// checkpoints keep their tags.
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
const CHECKPOINT_VERSION: u32 = 3;

#[derive(Serialize)]
struct EpochLog {
    stones: u8,
    epoch: u32,
    train: Metrics,
    valid: Option<Metrics>,
    seconds: f64,
}

pub fn run(args: TrainArgs) -> Result<()> {
    fs::create_dir_all(&args.work)?;
    let identity = Identity {
        settings: Settings {
            lr: args.lr,
            l2: args.l2,
            huber_delta: args.huber_delta,
        },
        pen: args.pen.then_some(PenSettings {
            lr: args.pen_lr,
            dense_lr: args.pen_dense_lr,
            batch: args.pen_batch,
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
    // The largest phases first, so no thread is left with one at the end.
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
        // No checkpoint, so the export copies a neighbouring phase.
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
            ));
            let w = if args.pen_no_linear {
                Weights::PenOnly(w)
            } else {
                Weights::Pen(w)
            };
            (w, 0)
        }
        None => (Weights::Linear(PhaseWeights::new(&records)), 0),
    };
    // Replay the finished epochs' shuffles so a resumed run sees the same
    // orders as an uninterrupted one.
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

/// Writes the model from every checkpoint in the work directory.
pub fn export(args: &ExportArgs) -> Result<()> {
    validate_shrink(args.shrink)?;
    let mut trained = vec![None; deft_reversi_engine::PHASE_COUNT];
    let mut networks = vec![None; deft_reversi_engine::PHASE_COUNT];
    for stones in 4..=63u8 {
        if let Some(c) = read_checkpoint(&checkpoint_path(&args.work, stones))? {
            let p = usize::from(stones) - 4;
            match c.weights {
                Weights::Linear(w) => trained[p] = Some(w.export(args.shrink)),
                Weights::Pen(w) | Weights::PenOnly(w) => {
                    let (linear, network) = w.export(args.shrink);
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
            seed: 1,
            pen: false,
            pen_no_linear: false,
            pen_lr: 0.01,
            pen_dense_lr: 0.001,
            pen_batch: 256,
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
