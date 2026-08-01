use crate::dataset::{prefetch_paths, read_split_shuffled, PhaseRange, RdReader, Sample};
use crate::loss::{huber_gradient, huber_loss};
use crate::train_log::{total_batches, BatchMetrics, EpochSummary, ProgressLogger};
use clap::Args;
use deft_reversi_engine::train_api::FeatureIndexes;
use deft_reversi_engine::train_api::MpcConfig;
use deft_reversi_engine::train_api::{
    EngineFile, EvaluatorData, Metadata, PatternEvaluatorData, PhaseData,
};
use deft_reversi_engine::train_api::{
    N_FEATURES, N_MOBILITY_BASE, N_MOBILITY_MAX, N_PATTERNS, N_PHASES, N_ROTATIONS,
    PATTERN_TABLE_SIZES, SCORE_SCALE, TOTAL_PATTERN_WEIGHTS,
};
use rand::{seq::SliceRandom, SeedableRng};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

#[derive(Args, Clone)]
pub struct PatternTrainArgs {
    #[arg(long)]
    pub data: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long, default_value_t = 3)]
    pub epochs: usize,
    #[arg(long, default_value_t = 1024)]
    pub batch_size: usize,
    #[arg(long, default_value_t = 0.05)]
    pub lr: f32,
    #[arg(long, default_value_t = 1e-8)]
    pub adagrad_eps: f32,
    #[arg(long, default_value_t = 1e-7)]
    pub l2: f32,
    #[arg(long, default_value_t = 32.0)]
    pub count_shrink_k: f32,
    #[arg(long, default_value_t = 4.0)]
    pub huber_delta: f32,
    #[arg(long, default_value_t = 200_000)]
    pub valid_limit: usize,
    #[arg(long)]
    pub train_limit: Option<u64>,
    #[arg(long, default_value_t = 65_536)]
    pub shuffle_buffer: usize,
    #[arg(long, default_value_t = 8_192)]
    pub prefetch_buffer: usize,
    #[arg(long, default_value_t = 1)]
    pub seed: u64,
    #[arg(long, default_value = "12..63")]
    pub phase_range: PhaseRange,
    #[arg(long)]
    pub checkpoint_dir: Option<PathBuf>,
    #[arg(long)]
    pub log_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 10_000)]
    pub checkpoint_every_steps: u64,
    #[arg(long, default_value_t = 1_000)]
    pub progress_every_steps: u64,
    #[arg(long)]
    pub resume: Option<PathBuf>,
    #[arg(long, default_value = "pattern-trained")]
    pub eval_name: String,
    #[arg(long, default_value = "0")]
    pub eval_version: String,
}

#[derive(Serialize, Deserialize)]
struct PatternModel {
    phases: Vec<PatternPhase>,
}

#[derive(Serialize, Deserialize)]
struct PatternPhase {
    pattern_weights: Vec<f32>,
    mobility_weights: Vec<f32>,
    bias: f32,
}

#[derive(Serialize, Deserialize)]
struct PatternOptimizer {
    phases: Vec<PatternOptimizerPhase>,
}

#[derive(Serialize, Deserialize)]
struct PatternOptimizerPhase {
    pattern_squares: Vec<f32>,
    mobility_squares: Vec<f32>,
    bias_square: f32,
    pattern_counts: Vec<u32>,
    mobility_counts: Vec<u32>,
    bias_count: u32,
}

#[derive(Serialize, Deserialize)]
struct PatternCheckpoint {
    format_version: u32,
    kind: String,
    model: PatternModel,
    optimizer: PatternOptimizer,
    epoch: usize,
    step: u64,
    seen_samples: u64,
}

#[derive(Clone)]
struct PhaseRecordSelection {
    path: PathBuf,
    n_records: u64,
    indices: Option<Vec<u64>>,
}

pub fn run(args: PatternTrainArgs) -> Result<(), String> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(args.seed);
    let train_paths = crate::dataset::split_paths(&args.data, "train", &args.phase_range)
        .map_err(|err| err.to_string())?;
    let record_counts = record_counts(&train_paths).map_err(|err| err.to_string())?;
    let selected_train_records = args
        .train_limit
        .map(|limit| build_limited_record_selection(&train_paths, &record_counts, limit, args.seed))
        .transpose()?;
    let n_train_records = selected_train_records
        .as_ref()
        .map(|selection| selected_record_count(selection))
        .unwrap_or_else(|| record_counts.iter().sum());
    let mut checkpoint = if let Some(path) = &args.resume {
        read_checkpoint(path)?
    } else {
        PatternCheckpoint {
            format_version: 1,
            kind: "pattern".to_string(),
            model: PatternModel::default(),
            optimizer: PatternOptimizer::default(),
            epoch: 0,
            step: 0,
            seen_samples: 0,
        }
    };

    for epoch in checkpoint.epoch..args.epochs {
        let paths = read_split_shuffled(&args.data, "train", &args.phase_range, &mut rng)
            .map_err(|err| err.to_string())?;
        let selected_paths = selected_train_records
            .as_ref()
            .map(|selection| order_selection_by_paths(selection, &paths));
        let mut logger = ProgressLogger::new(
            epoch,
            args.epochs,
            total_batches(n_train_records, args.batch_size),
            args.log_dir.as_ref().or(args.checkpoint_dir.as_ref()),
            "pattern",
        )
        .map_err(|err| err.to_string())?;
        let mut loss_sum = 0.0f64;
        let mut disc_error_sum = 0.0f64;
        let mut count = 0u64;
        let mut batch = Vec::with_capacity(args.batch_size.max(1));
        let mut sample_buffer = Vec::with_capacity(args.shuffle_buffer.max(args.batch_size).max(1));
        let samples: Box<dyn Iterator<Item = io::Result<Sample>>> =
            if let Some(selection) = selected_paths {
                Box::new(prefetch_selected_paths(selection, args.prefetch_buffer))
            } else {
                Box::new(prefetch_paths(paths, args.prefetch_buffer))
            };
        for sample in samples {
            let sample = sample.map_err(|err| err.to_string())?;
            sample_buffer.push(sample);
            if sample_buffer.len() >= args.shuffle_buffer.max(args.batch_size).max(1) {
                sample_buffer.shuffle(&mut rng);
                train_buffer(
                    &mut checkpoint,
                    &mut batch,
                    &mut sample_buffer,
                    &args,
                    epoch,
                    &mut loss_sum,
                    &mut disc_error_sum,
                    &mut count,
                    &mut logger,
                )?;
            }
        }
        sample_buffer.shuffle(&mut rng);
        train_buffer(
            &mut checkpoint,
            &mut batch,
            &mut sample_buffer,
            &args,
            epoch,
            &mut loss_sum,
            &mut disc_error_sum,
            &mut count,
            &mut logger,
        )?;
        if !batch.is_empty() {
            let metrics = train_batch(
                &mut checkpoint.model,
                &mut checkpoint.optimizer,
                &batch,
                &args,
            );
            loss_sum += metrics.loss as f64 * metrics.n_samples as f64;
            disc_error_sum += metrics.disc_mae as f64 * metrics.n_samples as f64;
            count += metrics.n_samples as u64;
            checkpoint.step += 1;
            checkpoint.seen_samples += metrics.n_samples as u64;
            batch.clear();
        }
        checkpoint.epoch = epoch + 1;
        let valid = validate(&checkpoint.model, &args)?;
        logger.log_epoch_summary(EpochSummary {
            epoch,
            total_epochs: args.epochs,
            train_loss_sum: loss_sum,
            train_disc_error_sum: disc_error_sum,
            train_samples: count,
            valid_loss: valid.loss as f64,
            valid_disc_mae: valid.disc_mae as f64,
            step: checkpoint.step,
        });
        if let Some(dir) = &args.checkpoint_dir {
            write_checkpoint(dir, &checkpoint)?;
        }
    }

    let engine_file = EngineFile {
        format_version: 4,
        metadata: metadata(&args, checkpoint.seen_samples, checkpoint.step),
        evaluator: EvaluatorData::Pattern(
            checkpoint
                .model
                .to_data(&checkpoint.optimizer, args.count_shrink_k),
        ),
        mpc: MpcConfig::default(),
    };
    engine_file
        .write_file(args.out.to_str().ok_or("output path is not valid UTF-8")?)
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn train_buffer(
    checkpoint: &mut PatternCheckpoint,
    batch: &mut Vec<Sample>,
    sample_buffer: &mut Vec<Sample>,
    args: &PatternTrainArgs,
    epoch: usize,
    loss_sum: &mut f64,
    disc_error_sum: &mut f64,
    count: &mut u64,
    logger: &mut ProgressLogger,
) -> Result<(), String> {
    for sample in sample_buffer.drain(..) {
        batch.push(sample);
        if batch.len() < args.batch_size.max(1) {
            continue;
        }
        train_one_batch(
            checkpoint,
            batch,
            args,
            epoch,
            loss_sum,
            disc_error_sum,
            count,
            logger,
        )?;
    }
    Ok(())
}

fn record_counts(paths: &[PathBuf]) -> io::Result<Vec<u64>> {
    paths
        .iter()
        .map(|path| {
            let size = path.metadata()?.len();
            if size < crate::dataset::RD_MAGIC.len() as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: missing rd header", path.display()),
                ));
            }
            let payload = size - crate::dataset::RD_MAGIC.len() as u64;
            if payload % crate::dataset::RD_RECORD_SIZE as u64 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: invalid rd size", path.display()),
                ));
            }
            Ok(payload / crate::dataset::RD_RECORD_SIZE as u64)
        })
        .collect()
}

fn allocate_phase_balanced_limit(capacities: &[u64], limit: u64) -> Vec<u64> {
    let total = limit.min(capacities.iter().sum());
    if total == 0 || capacities.is_empty() {
        return vec![0; capacities.len()];
    }
    let base = total / capacities.len() as u64;
    let remainder = total % capacities.len() as u64;
    let mut allocations = capacities
        .iter()
        .enumerate()
        .map(|(idx, &capacity)| capacity.min(base + u64::from(idx < remainder as usize)))
        .collect::<Vec<_>>();
    let mut remaining = total - allocations.iter().sum::<u64>();
    while remaining > 0 {
        let mut progressed = false;
        for (allocation, &capacity) in allocations.iter_mut().zip(capacities) {
            if *allocation >= capacity {
                continue;
            }
            *allocation += 1;
            remaining -= 1;
            progressed = true;
            if remaining == 0 {
                break;
            }
        }
        if !progressed {
            break;
        }
    }
    allocations
}

fn build_limited_record_selection(
    paths: &[PathBuf],
    capacities: &[u64],
    limit: u64,
    seed: u64,
) -> Result<Vec<PhaseRecordSelection>, String> {
    let allocations = allocate_phase_balanced_limit(capacities, limit);
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut selections = Vec::new();
    for ((path, &capacity), take) in paths.iter().zip(capacities).zip(allocations) {
        if take == 0 {
            continue;
        }
        let indices = if take >= capacity {
            None
        } else {
            Some(sample_record_indices(capacity, take, &mut rng)?)
        };
        selections.push(PhaseRecordSelection {
            path: path.clone(),
            n_records: take,
            indices,
        });
    }
    Ok(selections)
}

fn sample_record_indices<R: rand::Rng>(
    capacity: u64,
    take: u64,
    rng: &mut R,
) -> Result<Vec<u64>, String> {
    let capacity_usize = usize::try_from(capacity)
        .map_err(|_| format!("phase has too many records to sample: {capacity}"))?;
    let take_usize =
        usize::try_from(take).map_err(|_| format!("sample size is too large: {take}"))?;
    let mut indices = (0..capacity_usize as u64).collect::<Vec<_>>();
    indices.shuffle(rng);
    indices.truncate(take_usize);
    indices.sort_unstable();
    Ok(indices)
}

fn selected_record_count(selection: &[PhaseRecordSelection]) -> u64 {
    selection.iter().map(|phase| phase.n_records).sum()
}

fn order_selection_by_paths(
    selection: &[PhaseRecordSelection],
    paths: &[PathBuf],
) -> Vec<PhaseRecordSelection> {
    paths
        .iter()
        .filter_map(|path| selection.iter().find(|phase| phase.path == *path).cloned())
        .collect()
}

struct PrefetchedSelectedSamples {
    receiver: mpsc::Receiver<io::Result<Sample>>,
}

impl Iterator for PrefetchedSelectedSamples {
    type Item = io::Result<Sample>;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv().ok()
    }
}

fn prefetch_selected_paths(
    selections: Vec<PhaseRecordSelection>,
    capacity: usize,
) -> PrefetchedSelectedSamples {
    let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
    thread::spawn(move || {
        for selection in selections {
            let reader = match RdReader::open(&selection.path) {
                Ok(reader) => reader,
                Err(err) => {
                    let _ = sender.send(Err(err));
                    return;
                }
            };
            if let Some(indices) = selection.indices {
                let mut next_indices = indices.into_iter().peekable();
                for (idx, sample) in reader.enumerate() {
                    let Some(&next_idx) = next_indices.peek() else {
                        break;
                    };
                    if idx as u64 != next_idx {
                        continue;
                    }
                    next_indices.next();
                    if sender.send(sample).is_err() {
                        return;
                    }
                }
            } else {
                for sample in reader {
                    if sender.send(sample).is_err() {
                        return;
                    }
                }
            }
        }
    });
    PrefetchedSelectedSamples { receiver }
}

fn train_one_batch(
    checkpoint: &mut PatternCheckpoint,
    batch: &mut Vec<Sample>,
    args: &PatternTrainArgs,
    epoch: usize,
    loss_sum: &mut f64,
    disc_error_sum: &mut f64,
    count: &mut u64,
    logger: &mut ProgressLogger,
) -> Result<(), String> {
    let metrics = train_batch(
        &mut checkpoint.model,
        &mut checkpoint.optimizer,
        batch,
        args,
    );
    *loss_sum += metrics.loss as f64 * metrics.n_samples as f64;
    *disc_error_sum += metrics.disc_mae as f64 * metrics.n_samples as f64;
    *count += metrics.n_samples as u64;
    checkpoint.step += 1;
    checkpoint.seen_samples += metrics.n_samples as u64;
    batch.clear();

    if args.progress_every_steps > 0 && checkpoint.step % args.progress_every_steps == 0 {
        logger.log_train(
            checkpoint.step,
            *count,
            checkpoint.seen_samples,
            *loss_sum,
            *disc_error_sum,
            &metrics,
            args.lr,
        );
    }
    if let Some(dir) = &args.checkpoint_dir {
        if args.checkpoint_every_steps > 0 && checkpoint.step % args.checkpoint_every_steps == 0 {
            checkpoint.epoch = epoch;
            write_checkpoint(dir, checkpoint)?;
        }
    }
    Ok(())
}

fn train_batch(
    model: &mut PatternModel,
    optimizer: &mut PatternOptimizer,
    batch: &[Sample],
    args: &PatternTrainArgs,
) -> BatchMetrics {
    let batch_scale = 1.0 / batch.len().max(1) as f32;
    let mut loss_sum = 0.0;
    let mut disc_error_sum = 0.0;
    for &sample in batch {
        let (loss, disc_error) = train_sample(model, optimizer, sample, args, batch_scale);
        loss_sum += loss;
        disc_error_sum += disc_error;
    }
    BatchMetrics {
        loss: loss_sum / batch.len().max(1) as f32,
        disc_mae: disc_error_sum / batch.len().max(1) as f32,
        n_samples: batch.len(),
    }
}

fn train_sample(
    model: &mut PatternModel,
    optimizer: &mut PatternOptimizer,
    sample: Sample,
    args: &PatternTrainArgs,
    grad_scale: f32,
) -> (f32, f32) {
    let features = PatternFeatures::from_sample(sample);
    let prediction = model.predict(&features);
    let loss = huber_loss(prediction, sample.target, args.huber_delta);
    let disc_error = (prediction - sample.target).abs();
    let grad = huber_gradient(prediction, sample.target, args.huber_delta) * grad_scale;
    model.update(optimizer, &features, grad, args);
    (loss, disc_error)
}

fn validate(model: &PatternModel, args: &PatternTrainArgs) -> Result<BatchMetrics, String> {
    let paths = crate::dataset::split_paths(&args.data, "valid", &args.phase_range)
        .map_err(|err| err.to_string())?;
    let mut count = 0usize;
    let mut loss = 0.0f32;
    let mut disc_error = 0.0f32;
    'outer: for path in paths {
        for sample in RdReader::open(&path).map_err(|err| err.to_string())? {
            let sample = sample.map_err(|err| err.to_string())?;
            let features = PatternFeatures::from_sample(sample);
            let prediction = model.predict(&features);
            loss += huber_loss(prediction, sample.target, args.huber_delta);
            disc_error += (prediction - sample.target).abs();
            count += 1;
            if count >= args.valid_limit {
                break 'outer;
            }
        }
    }
    Ok(BatchMetrics {
        loss: loss / count.max(1) as f32,
        disc_mae: disc_error / count.max(1) as f32,
        n_samples: count,
    })
}

struct PatternFeatures {
    phase: usize,
    indexes: [usize; N_FEATURES],
    mobility: usize,
}

impl PatternFeatures {
    fn from_sample(sample: Sample) -> Self {
        let feature_indexes = FeatureIndexes::from_board(&sample.board);
        let mut indexes = [0usize; N_FEATURES];
        let mut offset = 0usize;
        for pattern_idx in 0..N_PATTERNS {
            for rotation in 0..N_ROTATIONS {
                let idx = pattern_idx * N_ROTATIONS + rotation;
                indexes[idx] = offset + feature_indexes.indexes()[idx] as usize;
            }
            offset += PATTERN_TABLE_SIZES[pattern_idx];
        }
        let mobility = (N_MOBILITY_BASE as i32 + sample.board.moves().count_ones() as i32
            - sample.board.opponent_moves().count_ones() as i32)
            .clamp(0, (N_MOBILITY_MAX - 1) as i32) as usize;
        Self {
            phase: sample.phase,
            indexes,
            mobility,
        }
    }
}

impl Default for PatternModel {
    fn default() -> Self {
        Self {
            phases: (0..N_PHASES)
                .map(|_| PatternPhase {
                    pattern_weights: vec![0.0; TOTAL_PATTERN_WEIGHTS],
                    mobility_weights: vec![0.0; N_MOBILITY_MAX],
                    bias: 0.0,
                })
                .collect(),
        }
    }
}

impl Default for PatternOptimizer {
    fn default() -> Self {
        Self {
            phases: (0..N_PHASES)
                .map(|_| PatternOptimizerPhase {
                    pattern_squares: vec![0.0; TOTAL_PATTERN_WEIGHTS],
                    mobility_squares: vec![0.0; N_MOBILITY_MAX],
                    bias_square: 0.0,
                    pattern_counts: vec![0; TOTAL_PATTERN_WEIGHTS],
                    mobility_counts: vec![0; N_MOBILITY_MAX],
                    bias_count: 0,
                })
                .collect(),
        }
    }
}

impl PatternModel {
    fn predict(&self, features: &PatternFeatures) -> f32 {
        let phase = &self.phases[features.phase];
        let mut score = phase.bias;
        for &idx in &features.indexes {
            score += phase.pattern_weights[idx];
        }
        score + phase.mobility_weights[features.mobility]
    }

    fn update(
        &mut self,
        optimizer: &mut PatternOptimizer,
        features: &PatternFeatures,
        grad: f32,
        args: &PatternTrainArgs,
    ) {
        let phase = &mut self.phases[features.phase];
        let opt = &mut optimizer.phases[features.phase];
        for &idx in &features.indexes {
            let grad = grad + args.l2 * phase.pattern_weights[idx];
            opt.pattern_squares[idx] += grad * grad;
            opt.pattern_counts[idx] += 1;
            phase.pattern_weights[idx] -=
                args.lr * grad / (opt.pattern_squares[idx].sqrt() + args.adagrad_eps);
        }
        let grad_mobility = grad + args.l2 * phase.mobility_weights[features.mobility];
        opt.mobility_squares[features.mobility] += grad_mobility * grad_mobility;
        opt.mobility_counts[features.mobility] += 1;
        phase.mobility_weights[features.mobility] -= args.lr * grad_mobility
            / (opt.mobility_squares[features.mobility].sqrt() + args.adagrad_eps);

        opt.bias_square += grad * grad;
        opt.bias_count += 1;
        phase.bias -= args.lr * grad / (opt.bias_square.sqrt() + args.adagrad_eps);
    }

    fn to_data(&self, optimizer: &PatternOptimizer, count_shrink_k: f32) -> PatternEvaluatorData {
        PatternEvaluatorData {
            phases: self
                .phases
                .iter()
                .zip(&optimizer.phases)
                .map(|(phase, opt)| phase.to_data(opt, count_shrink_k))
                .collect(),
        }
    }
}

impl PatternPhase {
    fn to_data(&self, optimizer: &PatternOptimizerPhase, count_shrink_k: f32) -> PhaseData {
        let mut pattern_weights = Vec::with_capacity(N_PATTERNS);
        let mut offset = 0usize;
        for &len in &PATTERN_TABLE_SIZES {
            pattern_weights.push(quantize_shrunk_vec(
                &self.pattern_weights[offset..offset + len],
                &optimizer.pattern_counts[offset..offset + len],
                count_shrink_k,
            ));
            offset += len;
        }
        PhaseData {
            pattern_weights,
            mobility_weights: quantize_shrunk_vec(
                &self.mobility_weights,
                &optimizer.mobility_counts,
                count_shrink_k,
            ),
            bias: quantize(self.bias),
        }
    }
}

fn quantize(value: f32) -> i16 {
    (value * SCORE_SCALE as f32)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

fn quantize_shrunk_vec(values: &[f32], counts: &[u32], k: f32) -> Vec<i16> {
    values
        .iter()
        .zip(counts)
        .map(|(&value, &count)| {
            let count = count as f32;
            let shrink = count / (count + k);
            quantize(value * shrink)
        })
        .collect()
}

fn metadata(args: &PatternTrainArgs, n_data_set: u64, n_iteration: u64) -> Metadata {
    Metadata {
        eval_name: args.eval_name.clone(),
        eval_version: args.eval_version.clone(),
        trained_at: String::new(),
        engine_version: String::new(),
        git_commit: String::new(),
        n_data_set,
        n_iteration,
        ..Metadata::default()
    }
}

fn read_checkpoint(path: &PathBuf) -> Result<PatternCheckpoint, String> {
    let bytes = fs::read(path.join("pattern-checkpoint.bin")).map_err(|err| err.to_string())?;
    let checkpoint: PatternCheckpoint =
        bincode::deserialize(&bytes).map_err(|err| err.to_string())?;
    if checkpoint.format_version != 1 || checkpoint.kind != "pattern" {
        return Err(format!(
            "{}: incompatible pattern checkpoint",
            path.display()
        ));
    }
    Ok(checkpoint)
}

fn write_checkpoint(dir: &PathBuf, checkpoint: &PatternCheckpoint) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let bytes = bincode::serialize(checkpoint).map_err(|err| err.to_string())?;
    atomic_write(&dir.join("pattern-checkpoint.bin"), &bytes).map_err(|err| err.to_string())?;
    let meta = serde_json::json!({
        "kind": "pattern",
        "format_version": checkpoint.format_version,
        "epoch": checkpoint.epoch,
        "step": checkpoint.step,
        "seen_samples": checkpoint.seen_samples,
    });
    atomic_write(
        &dir.join("checkpoint.json"),
        &serde_json::to_vec_pretty(&meta).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn atomic_write(path: &PathBuf, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::RD_MAGIC;
    use deft_reversi_engine::Board;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn pattern_training_reduces_loss_on_repeated_sample() {
        let args = test_args(temp_dir().join("out.bin"));
        let sample = Sample {
            board: Board::new(),
            target: 8.0,
            phase: 0,
        };
        let mut model = PatternModel::default();
        let mut optimizer = PatternOptimizer::default();
        let features = PatternFeatures::from_sample(sample);
        let before = huber_loss(model.predict(&features), sample.target, args.huber_delta);

        for _ in 0..32 {
            let batch = [sample; 4];
            let _ = train_batch(&mut model, &mut optimizer, &batch, &args);
        }

        let after = huber_loss(model.predict(&features), sample.target, args.huber_delta);
        assert!(after < before, "after={after} before={before}");
    }

    #[test]
    fn pattern_training_writes_readable_engine_file() {
        let dir = temp_dir();
        write_split(&dir, "train", Board::new(), 4, 8);
        write_split(&dir, "valid", Board::new(), 4, 8);
        let out = dir.join("pattern-eval.bin");
        let mut args = test_args(out.clone());
        args.data = dir.clone();
        args.epochs = 1;
        args.batch_size = 2;
        args.train_limit = Some(4);
        args.valid_limit = 2;
        args.shuffle_buffer = 4;
        args.phase_range = PhaseRange { start: 4, end: 4 };

        run(args).unwrap();

        let engine_file = EngineFile::read_file(out.to_str().unwrap()).unwrap();
        assert!(matches!(engine_file.evaluator, EvaluatorData::Pattern(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn balanced_limit_matches_python_allocation_cases() {
        assert_eq!(
            allocate_phase_balanced_limit(&[10, 10, 10], 5),
            vec![2, 2, 1]
        );
        assert_eq!(
            allocate_phase_balanced_limit(&[2, 10, 10], 12),
            vec![2, 5, 5]
        );
        assert_eq!(
            allocate_phase_balanced_limit(&[1, 100, 100], 10),
            vec![1, 5, 4]
        );
        assert_eq!(
            allocate_phase_balanced_limit(&[2, 3, 4], 100),
            vec![2, 3, 4]
        );
        assert_eq!(allocate_phase_balanced_limit(&[2, 3, 4], 0), vec![0, 0, 0]);
    }

    #[test]
    fn sampled_record_indices_are_sorted_unique_and_reproducible() {
        let mut rng_a = rand::rngs::StdRng::seed_from_u64(7);
        let mut rng_b = rand::rngs::StdRng::seed_from_u64(7);
        let indices_a = sample_record_indices(100, 12, &mut rng_a).unwrap();
        let indices_b = sample_record_indices(100, 12, &mut rng_b).unwrap();

        assert_eq!(indices_a, indices_b);
        assert_eq!(indices_a.len(), 12);
        assert!(indices_a.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(indices_a.iter().all(|&idx| idx < 100));
    }

    fn test_args(out: PathBuf) -> PatternTrainArgs {
        PatternTrainArgs {
            data: temp_dir(),
            out,
            epochs: 1,
            batch_size: 4,
            lr: 0.05,
            adagrad_eps: 1e-8,
            l2: 0.0,
            count_shrink_k: 0.0,
            huber_delta: 4.0,
            valid_limit: 10,
            train_limit: None,
            shuffle_buffer: 16,
            prefetch_buffer: 4,
            seed: 1,
            phase_range: PhaseRange { start: 4, end: 4 },
            checkpoint_dir: None,
            log_dir: None,
            checkpoint_every_steps: 0,
            progress_every_steps: 0,
            resume: None,
            eval_name: "test-pattern".to_string(),
            eval_version: "0".to_string(),
        }
    }

    fn write_split(dir: &PathBuf, split: &str, board: Board, stones: usize, value: i16) {
        let phase_dir = dir.join(format!("phase_{stones}"));
        fs::create_dir_all(&phase_dir).unwrap();
        let mut bytes = RD_MAGIC.to_vec();
        for _ in 0..4 {
            bytes.extend_from_slice(&board.player.to_le_bytes());
            bytes.extend_from_slice(&board.opponent.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fs::write(phase_dir.join(format!("{split}.rd")), bytes).unwrap();
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "deft-pattern-train-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
