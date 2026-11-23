//! Training the pattern embedding network (PEN) of one phase, together with
//! the linear pattern weights it is added to.
//!
//! The float network mirrors the engine's integers (`evaluation/pen.rs` in
//! the engine): embeddings and activations are in 0..=1 where the engine
//! uses 0..=127, and the weights are kept within what the engine's i8 can
//! hold.
//!
//! The linear weights and the embeddings take an AdaGrad step after every
//! sample, as the linear trainer does. The dense layers take an Adam step
//! after every `batch` samples, with the mean gradient, and decay toward 0
//! (`PenSettings::decay`).
//!
//! The network and the linear weights can trade a constant: before, the
//! network's output drifted to ±25 discs on average and the linear tables
//! carried the opposite offset, which shrinking rare weights at export
//! uncovered. After every pass the network's mean output moves into the
//! linear bias (through the output bias, which changes no prediction), and
//! the linear tables are recentred (`PhaseWeights::recenter`). Rarely seen
//! embeddings shrink toward their table's mean embedding, not toward the
//! starting value no trained state keeps.
use crate::{
    phase::{Accumulator, Metrics, PhaseWeights, Settings, shared_weights},
    rd::Record,
};
use deft_reversi_engine::{
    Board, FEATURE_COUNT, PATTERN_COUNT, PATTERN_WEIGHT_COUNT, PEN_DIM, PEN_GLOBALS, PEN_HIDDEN,
    PEN_INPUTS, PatternPhase, PenPhase, global_inputs, pattern_table_sizes,
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};

/// Network settings. Stored in checkpoints; resuming requires the same.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PenSettings {
    /// AdaGrad step size of the embeddings.
    pub lr: f32,
    /// Adam step size of the dense layers.
    pub dense_lr: f32,
    /// Samples per Adam step.
    pub batch: u32,
    /// Decoupled weight decay of the dense weights (not the biases): each
    /// Adam step also multiplies them by `1 - dense_lr * decay`.
    pub decay: f32,
}

impl PenSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.lr.is_finite() && self.lr > 0.0)
            || !(self.dense_lr.is_finite() && self.dense_lr > 0.0)
            || self.batch == 0
            || !(self.decay.is_finite() && self.decay >= 0.0 && self.decay * self.dense_lr < 1.0)
        {
            return Err(format!(
                "--pen-lr and --pen-dense-lr must be positive, --pen-batch at least 1 and \
                 --pen-decay nonnegative and below 1 / --pen-dense-lr: {self:?}"
            ));
        }
        Ok(())
    }
}

/// Every embedding starts here, so states never seen stay alike.
const EMBEDDING_INIT: f32 = 0.125;
/// Gradient kept through a clipped activation outside its range, so a
/// unit that left it can come back.
const LEAK: f32 = 0.01;
/// Largest hidden weight the engine's i8 holds at scale 64.
const HIDDEN_MAX: f32 = 127.0 / 64.0;
const GLOBAL_INPUT: usize = PATTERN_COUNT * PEN_DIM;

#[derive(Clone, Serialize, Deserialize)]
struct Adam {
    m: Vec<f32>,
    v: Vec<f32>,
}

impl Adam {
    fn new(n: usize) -> Self {
        Self {
            m: vec![0.0; n],
            v: vec![0.0; n],
        }
    }
    fn step(&mut self, params: &mut [f32], grads: &[f32], lr: f32, t: u64) {
        const B1: f32 = 0.9;
        const B2: f32 = 0.999;
        let c1 = 1.0 - B1.powi(t.min(1 << 20) as i32);
        let c2 = 1.0 - B2.powi(t.min(1 << 20) as i32);
        for i in 0..params.len() {
            self.m[i] = B1 * self.m[i] + (1.0 - B1) * grads[i];
            self.v[i] = B2 * self.v[i] + (1.0 - B2) * grads[i] * grads[i];
            params[i] -= lr * (self.m[i] / c1) / ((self.v[i] / c2).sqrt() + 1e-8);
        }
    }
}

/// The dense parameters in one vector: hidden weights (input-major: the
/// weights of input `k` for every unit are `W1 + k * PEN_HIDDEN ..`, so one
/// input updates all units with one vector operation), hidden biases,
/// output weights, output bias.
const W1: usize = 0;
const B1: usize = W1 + PEN_HIDDEN * PEN_INPUTS;
const W2: usize = B1 + PEN_HIDDEN;
const B2: usize = W2 + PEN_HIDDEN;
const DENSE: usize = B2 + 1;

/// A pattern state's embedding with its AdaGrad state, in one cache line:
/// a sample touches 44 states at random, and one miss each is the cost.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[repr(C, align(64))]
struct Embedding {
    value: [f32; PEN_DIM],
    squares: [f32; PEN_DIM],
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PenWeights {
    pub linear: PhaseWeights,
    embeddings: Vec<Embedding>,
    /// Samples that used each pattern state.
    seen: Vec<u32>,
    dense: Vec<f32>,
    adam: Adam,
    steps: u64,
}

/// The forward pass of one sample, kept for the backward pass.
struct Pass {
    features: deft_reversi_engine::Features,
    sums: [f32; PATTERN_COUNT * PEN_DIM],
    inputs: [f32; PEN_INPUTS],
    pre: [f32; PEN_HIDDEN],
    hidden: [f32; PEN_HIDDEN],
    output: f32,
}

/// 1 for a clipped activation's input inside its range, else `LEAK`;
/// without a branch, as the inputs fall either way at random.
#[inline]
fn inside_slope(x: f32) -> f32 {
    let inside = (x > 0.0) & (x < 1.0);
    LEAK + (1.0 - LEAK) * f32::from(u8::from(inside))
}

/// Calls `f(state, pattern, times)` once for every distinct state among
/// the 44 reads, with how many orientations read it. A state belongs to
/// one pattern's table, so only the four orientations of a pattern can
/// repeat one.
#[inline]
pub fn for_each_read(patterns: &[u32; FEATURE_COUNT], mut f: impl FnMut(u32, usize, u32)) {
    for (p, four) in patterns.as_chunks::<4>().0.iter().enumerate() {
        for i in 0..4 {
            if four[..i].contains(&four[i]) {
                continue;
            }
            let times = four[i..].iter().filter(|&&s| s == four[i]).count() as u32;
            f(four[i], p, times);
        }
    }
}

// The dense layer's loops, each in a function of its own: inlined into the
// large forward and backward passes, the compiler left them scalar (about
// 4x slower).

/// `bias + Σ_k inputs[k] · weights[k]`, every unit at once, input by input:
/// independent sums in vector registers (one sum per unit would chain every
/// add). `weights` is input-major.
#[inline(never)]
fn hidden_sums(
    weights: &[f32],
    bias: [f32; PEN_HIDDEN],
    inputs: &[f32; PEN_INPUTS],
) -> [f32; PEN_HIDDEN] {
    let mut sums = bias;
    for (k, &x) in inputs.iter().enumerate() {
        let column = &weights[k * PEN_HIDDEN..][..PEN_HIDDEN];
        for (s, w) in sums.iter_mut().zip(column) {
            *s += w * x;
        }
    }
    sums
}

/// `grads[k][j] += inputs[k] · d[j]`, the hidden weights' gradient.
#[inline(never)]
fn add_outer(grads: &mut [f32], inputs: &[f32; PEN_INPUTS], d: &[f32; PEN_HIDDEN]) {
    for (k, &x) in inputs.iter().enumerate() {
        let column = &mut grads[k * PEN_HIDDEN..][..PEN_HIDDEN];
        for (g, d) in column.iter_mut().zip(d) {
            *g += d * x;
        }
    }
}

/// The gradient of each summed embedding: the dot product of the units'
/// gradients with the input's weights, in eight lanes.
#[inline(never)]
fn input_gradients(weights: &[f32], d: &[f32; PEN_HIDDEN]) -> [f32; PATTERN_COUNT * PEN_DIM] {
    let mut out = [0.0; PATTERN_COUNT * PEN_DIM];
    for (k, o) in out.iter_mut().enumerate() {
        let column = &weights[k * PEN_HIDDEN..][..PEN_HIDDEN];
        let mut lanes = [0.0f32; 8];
        for (w, d) in column.as_chunks::<8>().0.iter().zip(d.as_chunks::<8>().0) {
            for t in 0..8 {
                lanes[t] += w[t] * d[t];
            }
        }
        *o = lanes.iter().sum();
    }
    out
}

impl PenWeights {
    pub fn new(records: &[Record], seed: u64, mirror: bool) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut dense = vec![0.0; DENSE];
        let bound = 0.5 * (6.0 / (PEN_INPUTS + PEN_HIDDEN) as f32).sqrt();
        for w in &mut dense[W1..B1] {
            *w = rng.gen_range(-bound..bound);
        }
        // Units start in the middle of their range; the output starts at 0,
        // so the model starts as the linear one.
        dense[B1..W2].fill(0.5);
        Self {
            linear: PhaseWeights::new(records, mirror),
            embeddings: vec![
                Embedding {
                    value: [EMBEDDING_INIT; PEN_DIM],
                    squares: [0.0; PEN_DIM],
                };
                PATTERN_WEIGHT_COUNT
            ],
            seen: vec![0; PATTERN_WEIGHT_COUNT],
            dense,
            adam: Adam::new(DENSE),
            steps: 0,
        }
    }

    fn forward(&self, board: &Board) -> Pass {
        let features = self.linear.features(board);
        let mut sums = [0.0; PATTERN_COUNT * PEN_DIM];
        for (i, &state) in features.patterns.iter().enumerate() {
            let p = i / 4;
            let e = &self.embeddings[state as usize].value;
            for (s, v) in sums[p * PEN_DIM..][..PEN_DIM].iter_mut().zip(e) {
                *s += v;
            }
        }
        let mut inputs = [0.0; PEN_INPUTS];
        for (x, s) in inputs.iter_mut().zip(&sums) {
            *x = s.clamp(0.0, 1.0);
        }
        let globals: [u8; PEN_GLOBALS] = global_inputs(board);
        for (x, &g) in inputs[GLOBAL_INPUT..].iter_mut().zip(&globals) {
            *x = f32::from(g) / 127.0;
        }
        // Every unit at once, input by input: independent sums the compiler
        // keeps in vector registers (one sum per unit would chain every add).
        let pre = hidden_sums(
            &self.dense[W1..B1],
            self.dense[B1..W2].try_into().unwrap(),
            &inputs,
        );
        let mut hidden = [0.0; PEN_HIDDEN];
        let mut output = self.dense[B2];
        for j in 0..PEN_HIDDEN {
            hidden[j] = pre[j].clamp(0.0, 1.0);
            output += self.dense[W2 + j] * hidden[j];
        }
        Pass {
            features,
            sums,
            inputs,
            pre,
            hidden,
            output,
        }
    }

    pub fn predict(&self, board: &Board) -> f32 {
        let pass = self.forward(board);
        self.linear.predict(&pass.features) + pass.output
    }

    /// One pass over `records` in their order. Without `linear`, the linear
    /// weights stay as they started (only the bias, at the mean label) and
    /// the network alone fits the positions.
    pub fn train(
        &mut self,
        records: &[Record],
        s: &Settings,
        pen: &PenSettings,
        linear: bool,
    ) -> Metrics {
        let mut m = Accumulator::default();
        let mut grads = vec![0.0f32; DENSE];
        let mut in_batch = 0;
        let mut outputs = 0.0f64;
        for r in records {
            let pass = self.forward(&r.board);
            outputs += f64::from(pass.output);
            let error = self.linear.predict(&pass.features) + pass.output - f32::from(r.value);
            m.add(error, s.huber_delta);
            let grad = error.clamp(-s.huber_delta, s.huber_delta);
            if linear {
                self.linear.update(&pass.features, grad, s);
            }
            self.backward(&pass, grad, &mut grads, pen);

            in_batch += 1;
            if in_batch == pen.batch {
                self.dense_step(&mut grads, in_batch, pen);
                in_batch = 0;
            }
        }
        if in_batch > 0 {
            self.dense_step(&mut grads, in_batch, pen);
        }
        // The network's mean output into the linear bias, then the linear
        // tables' means. The mean is of the outputs as each sample saw them
        // during the pass, not of the final network, so a small mean can
        // remain (about 0.5 discs on the full data); it saves a pass.
        let mean = (outputs / records.len().max(1) as f64) as f32;
        self.dense[B2] -= mean;
        self.linear.add_to_bias(mean);
        self.linear.recenter();
        m.finish()
    }

    fn backward(&mut self, pass: &Pass, grad: f32, grads: &mut [f32], pen: &PenSettings) {
        grads[B2] += grad;
        let mut d_pre = [0.0; PEN_HIDDEN];
        for j in 0..PEN_HIDDEN {
            grads[W2 + j] += grad * pass.hidden[j];
            let slope = inside_slope(pass.pre[j]);
            d_pre[j] = grad * self.dense[W2 + j] * slope;
        }
        for (g, d) in grads[B1..W2].iter_mut().zip(&d_pre) {
            *g += d;
        }
        add_outer(&mut grads[W1..B1], &pass.inputs, &d_pre);
        let mut d_inputs = input_gradients(&self.dense[W1..B1], &d_pre);
        for (d, &sum) in d_inputs.iter_mut().zip(&pass.sums) {
            *d *= inside_slope(sum);
        }
        // An embedding read more than once (symmetric discs, or a state and
        // its mirror image sharing one) takes one step with the summed
        // gradient, as the linear weights do. Every orientation of a
        // pattern has the same gradient, and a state belongs to one table.
        for_each_read(&pass.features.patterns, |state, p, times| {
            let times = times as f32;
            // All dimensions at once; a zero gradient leaves its dimension
            // as it is, as skipping it would.
            let e = &mut self.embeddings[state as usize];
            let g = &d_inputs[p * PEN_DIM..][..PEN_DIM];
            for d in 0..PEN_DIM {
                let g = g[d] * times;
                e.squares[d] += g * g;
                e.value[d] =
                    (e.value[d] - pen.lr * g / (e.squares[d].sqrt() + 1e-8)).clamp(-1.0, 1.0);
            }
            self.seen[state as usize] += times as u32;
        });
    }

    fn dense_step(&mut self, grads: &mut [f32], n: u32, pen: &PenSettings) {
        for g in grads.iter_mut() {
            *g /= n as f32;
        }
        self.steps += 1;
        // Decay before the step, as AdamW: w · keep − Δ.
        let keep = 1.0 - pen.dense_lr * pen.decay;
        let (hidden, rest) = self.dense[W1..B2].split_at_mut(B1 - W1);
        for w in hidden.iter_mut().chain(&mut rest[W2 - B1..]) {
            *w *= keep;
        }
        self.adam
            .step(&mut self.dense, grads, pen.dense_lr, self.steps);
        for w in &mut self.dense[W1..B1] {
            *w = w.clamp(-HIDDEN_MAX, HIDDEN_MAX);
        }
        grads.fill(0.0);
    }

    pub fn validate(&self, records: &[Record], huber_delta: f32) -> Metrics {
        let mut m = Accumulator::default();
        for r in records {
            m.add(self.predict(&r.board) - f32::from(r.value), huber_delta);
        }
        m.finish()
    }

    /// The mean embedding of every pattern table, weighted by use; the
    /// start value for a table no sample used.
    fn mean_embeddings(&self) -> Vec<[f32; PEN_DIM]> {
        let mut offset = 0;
        pattern_table_sizes()
            .iter()
            .map(|&size| {
                let mut sum = [0.0f64; PEN_DIM];
                let mut n = 0.0f64;
                for state in offset..offset + size {
                    let seen = f64::from(self.seen[state]);
                    n += seen;
                    for (s, &e) in sum.iter_mut().zip(&self.embeddings[state].value) {
                        *s += seen * f64::from(e);
                    }
                }
                offset += size;
                sum.map(|s| {
                    if n > 0.0 {
                        (s / n) as f32
                    } else {
                        EMBEDDING_INIT
                    }
                })
            })
            .collect()
    }

    /// Engine weights. `shrink` pulls the linear weights toward their
    /// table's mean, as `PhaseWeights::export` does, and the embeddings
    /// toward their table's mean embedding. Mirror images get the values
    /// they share.
    pub fn export(&self, shrink: f32) -> (PatternPhase, PenPhase) {
        let means = self.mean_embeddings();
        let shared = shared_weights();
        let mut table = Vec::with_capacity(PATTERN_WEIGHT_COUNT);
        for (p, size) in pattern_table_sizes().into_iter().enumerate() {
            table.extend(std::iter::repeat_n(p, size));
        }
        let embeddings = (0..PATTERN_WEIGHT_COUNT)
            .map(|state| {
                let mean = &means[table[state]];
                let state = if self.linear.mirror() {
                    shared[state] as usize
                } else {
                    state
                };
                let seen = self.seen[state] as f32;
                let keep = if seen + shrink > 0.0 {
                    seen / (seen + shrink)
                } else {
                    0.0
                };
                std::array::from_fn(|d| {
                    let e = self.embeddings[state].value[d];
                    let e = mean[d] + (e - mean[d]) * keep;
                    (e * 127.0).round().clamp(-127.0, 127.0) as i8
                })
            })
            .collect();
        let hidden: [[i8; PEN_INPUTS]; PEN_HIDDEN] = std::array::from_fn(|j| {
            std::array::from_fn(|k| {
                (self.dense[W1 + k * PEN_HIDDEN + j] * 64.0)
                    .round()
                    .clamp(-127.0, 127.0) as i8
            })
        });
        // The engine's limit on the biases (`PenPhase::new`).
        const BIAS: f32 = (1 << 24) as f32;
        let hidden_bias = std::array::from_fn(|j| {
            (self.dense[B1 + j] * 127.0 * 64.0)
                .round()
                .clamp(-BIAS, BIAS) as i32
        });
        let output = std::array::from_fn(|j| {
            (self.dense[W2 + j] * 128.0 * 128.0 / 127.0)
                .round()
                .clamp(-65536.0, 65536.0) as i32
        });
        let output_bias = (self.dense[B2] * 128.0).round().clamp(-BIAS, BIAS) as i32;
        let pen = PenPhase::new(embeddings, &hidden, hidden_bias, output, output_bias).unwrap();
        (self.linear.export(shrink), pen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Model, PHASE_COUNT, PatternModel};

    const SETTINGS: Settings = Settings {
        lr: 0.05,
        l2: 0.0,
        huber_delta: 4.0,
        mirror: true,
    };
    const PEN: PenSettings = PenSettings {
        lr: 0.002,
        dense_lr: 0.001,
        batch: 8,
        decay: 0.01,
    };

    #[test]
    fn the_network_learns_what_the_linear_model_cannot() {
        // The label is the product of the two mobilities: the linear model
        // sees only their difference, the network sees both.
        let mut random = StdRng::seed_from_u64(9);
        let mut records = crate::test_records(&mut random, 30, 20000);
        for r in &mut records {
            let opponent =
                Board::from_bits(r.board.opponent_bits(), r.board.player_bits()).unwrap();
            let product = r.board.legal_moves().len() * opponent.legal_moves().len();
            r.value = (product as i32 / 4 - 16).clamp(-64, 64) as i8;
        }
        let (train, valid) = records.split_at(18000);
        let mut linear = PhaseWeights::new(train, true);
        let mut pen = PenWeights::new(train, 1, true);
        for _ in 0..20 {
            linear.train(train, &SETTINGS);
            pen.train(train, &SETTINGS, &PEN, true);
        }
        let l = linear.validate(valid, 4.0).mae;
        let p = pen.validate(valid, 4.0).mae;
        assert!(p < l * 0.8, "linear {l}, pen {p}");
    }

    #[test]
    fn the_engine_reads_the_exported_network_like_the_trainer() {
        let mut random = StdRng::seed_from_u64(4);
        let records = crate::test_records(&mut random, 24, 2000);
        let mut pen = PenWeights::new(&records, 2, true);
        for _ in 0..10 {
            pen.train(&records, &SETTINGS, &PEN, true);
        }
        let (linear, network) = pen.export(0.0);
        let model = PatternModel::new(vec![linear; PHASE_COUNT])
            .unwrap()
            .with_pen(vec![network; PHASE_COUNT])
            .unwrap();
        let mut bytes = Vec::new();
        Model::from_pattern(model).write_to(&mut bytes).unwrap();
        // The network reads the disc input, so the file is a PEN2 one.
        assert_eq!(&bytes[..8], b"DPR2\x04\x00\x00\x00");
        let model = Model::from_bytes(&bytes).unwrap();
        let mut total = 0.0;
        for r in &records {
            let engine = model.pattern().evaluate(&r.board) as f32;
            let float = pen.predict(&r.board).clamp(-64.0, 64.0);
            assert!((engine - float).abs() <= 1.5, "{engine} vs {float}");
            total += (engine - float).abs();
        }
        // Rounding to whole discs alone gives about 0.25 on average.
        assert!(total / (records.len() as f32) < 0.4);
    }
}

#[cfg(test)]
mod symmetry_tests {
    use super::*;
    use deft_reversi_engine::{PHASE_COUNT, PatternModel};

    const SETTINGS: Settings = Settings {
        lr: 0.05,
        l2: 0.0,
        huber_delta: 4.0,
        mirror: true,
    };
    const PEN: PenSettings = PenSettings {
        lr: 0.01,
        dense_lr: 0.002,
        batch: 16,
        decay: 0.01,
    };

    fn flip(bits: u64) -> u64 {
        (0..64)
            .filter(|sq| bits >> sq & 1 != 0)
            .fold(0u64, |acc, sq| acc | 1 << (sq / 8 * 8 + 7 - sq % 8))
    }

    #[test]
    fn the_exported_network_values_mirror_images_alike() {
        let mut random = StdRng::seed_from_u64(12);
        let records = crate::test_records(&mut random, 28, 1500);
        let mut pen = PenWeights::new(&records, 3, true);
        for _ in 0..4 {
            pen.train(&records, &SETTINGS, &PEN, true);
        }
        let (linear, network) = pen.export(8.0);
        let model = PatternModel::new(vec![linear; PHASE_COUNT])
            .unwrap()
            .with_pen(vec![network; PHASE_COUNT])
            .unwrap();
        for r in &records {
            let b = r.board;
            let image = Board::from_bits(flip(b.player_bits()), flip(b.opponent_bits())).unwrap();
            assert_eq!(model.evaluate(&b), model.evaluate(&image));
        }
    }

    #[test]
    fn the_network_alone_exports_its_bias() {
        let mut random = StdRng::seed_from_u64(14);
        let mut records = crate::test_records(&mut random, 28, 1500);
        for r in &mut records {
            r.value = r.value.saturating_add(12);
        }
        let mut pen = PenWeights::new(&records, 5, true);
        for _ in 0..3 {
            pen.train(&records, &SETTINGS, &PEN, false);
        }
        let (linear, network) = pen.export(0.0);
        let model = PatternModel::new(vec![linear; PHASE_COUNT])
            .unwrap()
            .with_pen(vec![network; PHASE_COUNT])
            .unwrap();
        let mut total = 0.0;
        for r in &records {
            let engine = model.evaluate(&r.board) as f32;
            total += (engine - pen.predict(&r.board).clamp(-64.0, 64.0)).abs();
        }
        assert!(
            total / (records.len() as f32) < 0.5,
            "{}",
            total / records.len() as f32
        );
    }

    #[test]
    fn the_network_output_is_centred_after_a_pass() {
        let mut random = StdRng::seed_from_u64(13);
        let mut records = crate::test_records(&mut random, 28, 3000);
        for r in &mut records {
            r.value = r.value.saturating_add(20);
        }
        let mut pen = PenWeights::new(&records, 4, true);
        // An offset in the network, as the old trainer let it drift.
        pen.dense[B2] += 25.0;
        let before: Vec<f32> = records.iter().map(|r| pen.predict(&r.board)).collect();
        let mean_output = |pen: &PenWeights| {
            records
                .iter()
                .map(|r| pen.forward(&r.board).output)
                .sum::<f32>()
                / records.len() as f32
        };
        assert!(mean_output(&pen) > 20.0);
        // With no step size the pass only moves the offsets.
        let still = PenSettings {
            lr: 1e-30,
            dense_lr: 1e-30,
            ..PEN
        };
        let s = Settings {
            lr: 1e-30,
            ..SETTINGS
        };
        pen.train(&records, &s, &still, true);
        assert!(mean_output(&pen).abs() < 1e-3);
        for (r, b) in records.iter().zip(before) {
            assert!((pen.predict(&r.board) - b).abs() < 1e-3);
        }
    }
}
