use crate::dataset::{
    count_records, prefetch_paths, read_split_shuffled, PhaseRange, RdReader, Sample,
};
use crate::loss::{huber_gradient, huber_loss};
use crate::train_log::{total_batches, BatchMetrics, EpochSummary, ProgressLogger};
use clap::Args;
use deft_reversi_engine::train_api::{
    DenseLayerData, EngineFile, EvaluatorData, Metadata, MpcConfig, NnueEvaluatorData, NnueState,
    NnueTowerData, NNUE_ACCUMULATOR_SIZE, NNUE_ACTIVATION_MAX, NNUE_ACTIVATION_SCALE,
    NNUE_DEFAULT_WEIGHT_SCALE, NNUE_DENSE_LAYER_SIZES, NNUE_INPUT_SIZE, NNUE_TOWER_COUNT,
};
use rand::{seq::SliceRandom, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Args, Clone)]
pub struct NnueTrainArgs {
    #[arg(long)]
    pub data: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long, default_value_t = 3)]
    pub epochs: usize,
    #[arg(long, default_value_t = 1024)]
    pub batch_size: usize,
    #[arg(long, default_value_t = 0.001)]
    pub lr: f32,
    #[arg(long, default_value_t = 0.9)]
    pub adam_beta1: f32,
    #[arg(long, default_value_t = 0.999)]
    pub adam_beta2: f32,
    #[arg(long, default_value_t = 1e-8)]
    pub adam_eps: f32,
    #[arg(long, default_value_t = 1e-6)]
    pub l2: f32,
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
    #[arg(long, default_value = "nnue-trained")]
    pub eval_name: String,
    #[arg(long, default_value = "0")]
    pub eval_version: String,
}

#[derive(Serialize, Deserialize)]
struct NnueModel {
    shape: NnueShape,
    input_weights: Vec<Vec<f32>>,
    input_bias: Vec<f32>,
    dense_layers: Vec<DenseLayer>,
    output_weights: Vec<f32>,
    output_bias: f32,
}

#[derive(Clone, Serialize, Deserialize)]
struct NnueShape {
    input_size: usize,
    accumulator_size: usize,
    dense_layer_sizes: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
struct DenseLayer {
    weights: Vec<Vec<f32>>,
    bias: Vec<f32>,
}

#[derive(Serialize, Deserialize)]
struct AdamState {
    input_weights_m: Vec<Vec<f32>>,
    input_weights_v: Vec<Vec<f32>>,
    input_bias_m: Vec<f32>,
    input_bias_v: Vec<f32>,
    dense_layers: Vec<DenseAdamState>,
    output_weights_m: Vec<f32>,
    output_weights_v: Vec<f32>,
    output_bias_m: f32,
    output_bias_v: f32,
}

#[derive(Serialize, Deserialize)]
struct DenseAdamState {
    weights_m: Vec<Vec<f32>>,
    weights_v: Vec<Vec<f32>>,
    bias_m: Vec<f32>,
    bias_v: Vec<f32>,
}

#[derive(Serialize, Deserialize)]
struct NnueCheckpoint {
    format_version: u32,
    kind: String,
    model: NnueModel,
    optimizer: AdamState,
    epoch: usize,
    step: u64,
    seen_samples: u64,
}

struct ForwardCache {
    active_features: [Vec<usize>; 2],
    accumulator: Vec<f32>,
    raw_activations: Vec<Vec<f32>>,
    activations: Vec<Vec<f32>>,
    prediction: f32,
}

pub fn run(args: NnueTrainArgs) -> Result<(), String> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(args.seed);
    let mut checkpoint = if let Some(path) = &args.resume {
        read_checkpoint(path)?
    } else {
        let model = NnueModel::new(&mut rng);
        let optimizer = AdamState::zeros_like(&model);
        NnueCheckpoint {
            format_version: 1,
            kind: "nnue".to_string(),
            model,
            optimizer,
            epoch: 0,
            step: 0,
            seen_samples: 0,
        }
    };

    for epoch in checkpoint.epoch..args.epochs {
        let paths = read_split_shuffled(&args.data, "train", &args.phase_range, &mut rng)
            .map_err(|err| err.to_string())?;
        let n_train_records = count_records(&paths)
            .map_err(|err| err.to_string())?
            .min(args.train_limit.unwrap_or(u64::MAX));
        let mut logger = ProgressLogger::new(
            epoch,
            args.epochs,
            total_batches(n_train_records, args.batch_size),
            args.log_dir.as_ref().or(args.checkpoint_dir.as_ref()),
            "nnue",
        )
        .map_err(|err| err.to_string())?;
        let mut loss_sum = 0.0f64;
        let mut disc_error_sum = 0.0f64;
        let mut count = 0u64;
        let mut batch = Vec::with_capacity(args.batch_size.max(1));
        let mut sample_buffer = Vec::with_capacity(args.shuffle_buffer.max(args.batch_size).max(1));
        for sample in prefetch_paths(paths, args.prefetch_buffer) {
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
                if args.train_limit.is_some_and(|limit| count >= limit) {
                    break;
                }
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
            let metrics = train_batch(&mut checkpoint, &batch, &args);
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
        evaluator: EvaluatorData::Nnue(checkpoint.model.to_data()),
        mpc: MpcConfig::default(),
    };
    engine_file
        .write_file(args.out.to_str().ok_or("output path is not valid UTF-8")?)
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn train_buffer(
    checkpoint: &mut NnueCheckpoint,
    batch: &mut Vec<Sample>,
    sample_buffer: &mut Vec<Sample>,
    args: &NnueTrainArgs,
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

fn train_one_batch(
    checkpoint: &mut NnueCheckpoint,
    batch: &mut Vec<Sample>,
    args: &NnueTrainArgs,
    epoch: usize,
    loss_sum: &mut f64,
    disc_error_sum: &mut f64,
    count: &mut u64,
    logger: &mut ProgressLogger,
) -> Result<(), String> {
    let metrics = train_batch(checkpoint, batch, args);
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
    checkpoint: &mut NnueCheckpoint,
    batch: &[Sample],
    args: &NnueTrainArgs,
) -> BatchMetrics {
    let grad_scale = 1.0 / batch.len().max(1) as f32;
    let step = checkpoint.step + 1;
    let mut loss_sum = 0.0;
    let mut disc_error_sum = 0.0;
    for &sample in batch {
        let (loss, disc_error) = train_sample(checkpoint, sample, args, grad_scale, step);
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
    checkpoint: &mut NnueCheckpoint,
    sample: Sample,
    args: &NnueTrainArgs,
    grad_scale: f32,
    step: u64,
) -> (f32, f32) {
    let cache = checkpoint.model.forward(sample);
    let loss = huber_loss(cache.prediction, sample.target, args.huber_delta);
    let disc_error = (cache.prediction - sample.target).abs();
    let grad = huber_gradient(cache.prediction, sample.target, args.huber_delta) * grad_scale;
    checkpoint
        .model
        .backward_update(&mut checkpoint.optimizer, &cache, grad, step, args);
    (loss, disc_error)
}

fn validate(model: &NnueModel, args: &NnueTrainArgs) -> Result<BatchMetrics, String> {
    let paths = crate::dataset::split_paths(&args.data, "valid", &args.phase_range)
        .map_err(|err| err.to_string())?;
    let mut count = 0usize;
    let mut loss = 0.0f32;
    let mut disc_error = 0.0f32;
    'outer: for path in paths {
        for sample in RdReader::open(&path).map_err(|err| err.to_string())? {
            let sample = sample.map_err(|err| err.to_string())?;
            let cache = model.forward(sample);
            loss += huber_loss(cache.prediction, sample.target, args.huber_delta);
            disc_error += (cache.prediction - sample.target).abs();
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

impl NnueModel {
    fn new<R: Rng>(rng: &mut R) -> Self {
        Self::new_with_shape(NnueShape::production(), rng)
    }

    fn new_with_shape<R: Rng>(shape: NnueShape, rng: &mut R) -> Self {
        let mut dense_layers = Vec::with_capacity(shape.dense_layer_sizes.len());
        let mut input_size = shape.accumulator_size * 2;
        for &output_size in &shape.dense_layer_sizes {
            dense_layers.push(DenseLayer {
                weights: random_matrix(rng, output_size, input_size, 16.0),
                bias: vec![0.0; output_size],
            });
            input_size = output_size;
        }
        let output_size = *shape
            .dense_layer_sizes
            .last()
            .unwrap_or(&shape.accumulator_size);
        Self {
            input_weights: random_matrix(rng, shape.input_size, shape.accumulator_size, 8.0),
            input_bias: vec![0.0; shape.accumulator_size],
            dense_layers,
            output_weights: random_vec(rng, output_size, 2.0),
            output_bias: 0.0,
            shape,
        }
    }

    fn forward(&self, sample: Sample) -> ForwardCache {
        let state = NnueState::from_board(&sample.board);
        self.forward_feature_views([
            state.active_features_for_view(0).to_vec(),
            state.active_features_for_view(1).to_vec(),
        ])
    }

    #[cfg(test)]
    fn forward_features(&self, active_features: &[usize]) -> ForwardCache {
        self.forward_feature_views([active_features.to_vec(), Vec::new()])
    }

    fn forward_feature_views(&self, active_features: [Vec<usize>; 2]) -> ForwardCache {
        let mut accumulator = Vec::with_capacity(self.shape.accumulator_size * 2);
        for feature_view in &active_features {
            let mut view_accumulator = self.input_bias.clone();
            for &feature_id in feature_view {
                for (acc, &weight) in view_accumulator
                    .iter_mut()
                    .zip(&self.input_weights[feature_id])
                {
                    *acc += weight;
                }
            }
            accumulator.extend(view_accumulator);
        }
        let mut raw_activations = Vec::with_capacity(self.dense_layers.len());
        let mut activations = Vec::with_capacity(self.dense_layers.len() + 1);
        let mut activation = clamp_vec(&accumulator);
        activations.push(activation.clone());
        for layer in &self.dense_layers {
            let mut next = vec![0.0; layer.bias.len()];
            let mut raw_next = vec![0.0; layer.bias.len()];
            for (out_idx, out) in next.iter_mut().enumerate() {
                let mut raw = layer.bias[out_idx];
                for (&input, &weight) in activation.iter().zip(&layer.weights[out_idx]) {
                    raw += input * weight;
                }
                raw /= NNUE_DEFAULT_WEIGHT_SCALE as f32;
                raw_next[out_idx] = raw;
                *out = raw.clamp(0.0, NNUE_ACTIVATION_MAX as f32);
            }
            activation = next;
            raw_activations.push(raw_next);
            activations.push(activation.clone());
        }

        let mut prediction = self.output_bias;
        let final_activation = activations.last().unwrap();
        for (&input, &weight) in final_activation.iter().zip(&self.output_weights) {
            prediction += input * weight;
        }
        prediction /= NNUE_DEFAULT_WEIGHT_SCALE as f32;

        ForwardCache {
            active_features,
            accumulator,
            raw_activations,
            activations,
            prediction,
        }
    }

    fn backward_update(
        &mut self,
        adam: &mut AdamState,
        cache: &ForwardCache,
        output_grad: f32,
        step: u64,
        args: &NnueTrainArgs,
    ) {
        let final_activation = cache.activations.last().unwrap();
        let mut grad_activation = vec![0.0; final_activation.len()];
        for i in 0..self.output_weights.len() {
            let old_weight = self.output_weights[i];
            let grad = output_grad * final_activation[i] / NNUE_DEFAULT_WEIGHT_SCALE as f32
                + args.l2 * self.output_weights[i];
            adam_update(
                &mut self.output_weights[i],
                &mut adam.output_weights_m[i],
                &mut adam.output_weights_v[i],
                grad,
                step,
                args,
            );
            grad_activation[i] += output_grad * old_weight / NNUE_DEFAULT_WEIGHT_SCALE as f32;
        }
        adam_update(
            &mut self.output_bias,
            &mut adam.output_bias_m,
            &mut adam.output_bias_v,
            output_grad / NNUE_DEFAULT_WEIGHT_SCALE as f32,
            step,
            args,
        );

        for layer_idx in (0..self.dense_layers.len()).rev() {
            let input_activation = &cache.activations[layer_idx];
            let raw_activation = &cache.raw_activations[layer_idx];
            let mut grad_input = vec![0.0; input_activation.len()];
            let layer = &mut self.dense_layers[layer_idx];
            let layer_adam = &mut adam.dense_layers[layer_idx];

            for out_idx in 0..layer.bias.len() {
                let grad_out = if raw_activation[out_idx] > 0.0
                    && raw_activation[out_idx] < NNUE_ACTIVATION_MAX as f32
                {
                    grad_activation[out_idx]
                } else {
                    0.0
                };
                for in_idx in 0..input_activation.len() {
                    let old_weight = layer.weights[out_idx][in_idx];
                    grad_input[in_idx] += grad_out * old_weight / NNUE_DEFAULT_WEIGHT_SCALE as f32;
                    let grad = grad_out * input_activation[in_idx]
                        / NNUE_DEFAULT_WEIGHT_SCALE as f32
                        + args.l2 * layer.weights[out_idx][in_idx];
                    adam_update(
                        &mut layer.weights[out_idx][in_idx],
                        &mut layer_adam.weights_m[out_idx][in_idx],
                        &mut layer_adam.weights_v[out_idx][in_idx],
                        grad,
                        step,
                        args,
                    );
                }
                adam_update(
                    &mut layer.bias[out_idx],
                    &mut layer_adam.bias_m[out_idx],
                    &mut layer_adam.bias_v[out_idx],
                    grad_out / NNUE_DEFAULT_WEIGHT_SCALE as f32,
                    step,
                    args,
                );
            }
            grad_activation = grad_input;
        }

        for (idx, grad) in grad_activation.iter_mut().enumerate() {
            if cache.accumulator[idx] <= 0.0 || cache.accumulator[idx] >= NNUE_ACTIVATION_MAX as f32
            {
                *grad = 0.0;
            }
        }
        for idx in 0..self.input_bias.len() {
            let grad = grad_activation[idx] + grad_activation[self.shape.accumulator_size + idx];
            adam_update(
                &mut self.input_bias[idx],
                &mut adam.input_bias_m[idx],
                &mut adam.input_bias_v[idx],
                grad,
                step,
                args,
            );
        }
        for view in 0..2 {
            let grad_offset = view * self.shape.accumulator_size;
            for &feature_id in &cache.active_features[view] {
                for idx in 0..self.shape.accumulator_size {
                    let grad = grad_activation[grad_offset + idx]
                        + args.l2 * self.input_weights[feature_id][idx];
                    adam_update(
                        &mut self.input_weights[feature_id][idx],
                        &mut adam.input_weights_m[feature_id][idx],
                        &mut adam.input_weights_v[feature_id][idx],
                        grad,
                        step,
                        args,
                    );
                }
            }
        }
    }

    fn to_data(&self) -> NnueEvaluatorData {
        let tower = NnueTowerData {
            dense_layers: self
                .dense_layers
                .iter()
                .enumerate()
                .map(|(idx, layer)| DenseLayerData {
                    input_size: if idx == 0 {
                        self.shape.accumulator_size * 2
                    } else {
                        self.shape.dense_layer_sizes[idx - 1]
                    },
                    output_size: self.shape.dense_layer_sizes[idx],
                    weights: layer.weights.iter().map(|row| quantize_vec(row)).collect(),
                    bias: quantize_bias_vec(&layer.bias),
                })
                .collect(),
            output_weights: quantize_vec(&self.output_weights),
            output_bias: quantize_bias(self.output_bias),
        };
        NnueEvaluatorData {
            input_size: self.shape.input_size,
            acc_size: self.shape.accumulator_size,
            pattern_set: "v2".to_string(),
            share_rotations: false,
            accumulator_size: self.shape.accumulator_size,
            activation_scale: NNUE_ACTIVATION_SCALE,
            weight_scale: NNUE_DEFAULT_WEIGHT_SCALE,
            tower_count: NNUE_TOWER_COUNT,
            pairwise: true,
            input_weights: self
                .input_weights
                .iter()
                .map(|row| quantize_vec(row))
                .collect(),
            input_bias: quantize_bias_vec(&self.input_bias),
            towers: (0..NNUE_TOWER_COUNT).map(|_| tower.clone()).collect(),
        }
    }
}

impl NnueShape {
    fn production() -> Self {
        Self {
            input_size: NNUE_INPUT_SIZE,
            accumulator_size: NNUE_ACCUMULATOR_SIZE,
            dense_layer_sizes: NNUE_DENSE_LAYER_SIZES.to_vec(),
        }
    }
}

impl AdamState {
    fn zeros_like(model: &NnueModel) -> Self {
        Self {
            input_weights_m: zero_matrix_like(&model.input_weights),
            input_weights_v: zero_matrix_like(&model.input_weights),
            input_bias_m: vec![0.0; model.input_bias.len()],
            input_bias_v: vec![0.0; model.input_bias.len()],
            dense_layers: model
                .dense_layers
                .iter()
                .map(|layer| DenseAdamState {
                    weights_m: zero_matrix_like(&layer.weights),
                    weights_v: zero_matrix_like(&layer.weights),
                    bias_m: vec![0.0; layer.bias.len()],
                    bias_v: vec![0.0; layer.bias.len()],
                })
                .collect(),
            output_weights_m: vec![0.0; model.output_weights.len()],
            output_weights_v: vec![0.0; model.output_weights.len()],
            output_bias_m: 0.0,
            output_bias_v: 0.0,
        }
    }
}

fn adam_update(
    weight: &mut f32,
    m: &mut f32,
    v: &mut f32,
    grad: f32,
    step: u64,
    args: &NnueTrainArgs,
) {
    *m = args.adam_beta1 * *m + (1.0 - args.adam_beta1) * grad;
    *v = args.adam_beta2 * *v + (1.0 - args.adam_beta2) * grad * grad;
    let m_hat = *m / (1.0 - args.adam_beta1.powi(step.min(i32::MAX as u64) as i32));
    let v_hat = *v / (1.0 - args.adam_beta2.powi(step.min(i32::MAX as u64) as i32));
    *weight -= args.lr * m_hat / (v_hat.sqrt() + args.adam_eps);
}

fn random_matrix<R: Rng>(rng: &mut R, rows: usize, cols: usize, scale: f32) -> Vec<Vec<f32>> {
    (0..rows).map(|_| random_vec(rng, cols, scale)).collect()
}

fn random_vec<R: Rng>(rng: &mut R, len: usize, scale: f32) -> Vec<f32> {
    (0..len)
        .map(|_| rng.gen_range(-scale..scale))
        .collect::<Vec<_>>()
}

fn zero_matrix_like(matrix: &[Vec<f32>]) -> Vec<Vec<f32>> {
    matrix.iter().map(|row| vec![0.0; row.len()]).collect()
}

fn clamp_vec(values: &[f32]) -> Vec<f32> {
    values
        .iter()
        .map(|&value| value.clamp(0.0, NNUE_ACTIVATION_MAX as f32))
        .collect()
}

fn quantize(value: f32) -> i16 {
    value.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

fn quantize_vec(values: &[f32]) -> Vec<i16> {
    values.iter().map(|&value| quantize(value)).collect()
}

fn quantize_bias(value: f32) -> i32 {
    value.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn quantize_bias_vec(values: &[f32]) -> Vec<i32> {
    values.iter().map(|&value| quantize_bias(value)).collect()
}

fn metadata(args: &NnueTrainArgs, n_data_set: u64, n_iteration: u64) -> Metadata {
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

fn read_checkpoint(path: &PathBuf) -> Result<NnueCheckpoint, String> {
    let bytes = fs::read(path.join("nnue-checkpoint.bin")).map_err(|err| err.to_string())?;
    let checkpoint: NnueCheckpoint = bincode::deserialize(&bytes).map_err(|err| err.to_string())?;
    if checkpoint.format_version != 1 || checkpoint.kind != "nnue" {
        return Err(format!("{}: incompatible nnue checkpoint", path.display()));
    }
    Ok(checkpoint)
}

fn write_checkpoint(dir: &PathBuf, checkpoint: &NnueCheckpoint) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let bytes = bincode::serialize(checkpoint).map_err(|err| err.to_string())?;
    atomic_write(&dir.join("nnue-checkpoint.bin"), &bytes).map_err(|err| err.to_string())?;
    let meta = serde_json::json!({
        "kind": "nnue",
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

    #[test]
    fn nnue_tiny_model_training_reduces_loss() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut model = NnueModel::new_with_shape(
            NnueShape {
                input_size: 4,
                accumulator_size: 4,
                dense_layer_sizes: vec![3, 2],
            },
            &mut rng,
        );
        model.input_weights = vec![vec![0.0; 4]; 4];
        model.input_bias = vec![512.0; 4];
        for layer in &mut model.dense_layers {
            for row in &mut layer.weights {
                for weight in row {
                    *weight = 64.0;
                }
            }
            for bias in &mut layer.bias {
                *bias = 0.0;
            }
        }
        model.output_weights = vec![1024.0; 2];
        model.output_bias = 0.0;
        let mut optimizer = AdamState::zeros_like(&model);
        let args = test_args();
        let target = 4.0;
        let active_features = [0usize, 1usize];
        let before = huber_loss(
            model.forward_features(&active_features).prediction,
            target,
            args.huber_delta,
        );

        for step in 1..64 {
            let cache = model.forward_features(&active_features);
            let grad = huber_gradient(cache.prediction, target, args.huber_delta);
            model.backward_update(&mut optimizer, &cache, grad, step, &args);
        }

        let after = huber_loss(
            model.forward_features(&active_features).prediction,
            target,
            args.huber_delta,
        );
        assert!(after < before, "after={after} before={before}");
    }

    fn test_args() -> NnueTrainArgs {
        NnueTrainArgs {
            data: PathBuf::new(),
            out: PathBuf::new(),
            epochs: 1,
            batch_size: 4,
            lr: 0.01,
            adam_beta1: 0.9,
            adam_beta2: 0.999,
            adam_eps: 1e-8,
            l2: 0.0,
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
            eval_name: "test-nnue".to_string(),
            eval_version: "0".to_string(),
        }
    }
}
