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
//! after every `batch` samples, with the mean gradient.
use crate::{
    phase::{Accumulator, Metrics, PhaseWeights, Settings},
    rd::Record,
};
use deft_reversi_engine::{
    Board, FEATURE_COUNT, Features, PATTERN_COUNT, PATTERN_WEIGHT_COUNT, PEN_DIM, PEN_HIDDEN,
    PEN_INPUTS, PatternPhase, PenPhase,
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
}

impl PenSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.lr.is_finite() && self.lr > 0.0)
            || !(self.dense_lr.is_finite() && self.dense_lr > 0.0)
            || self.batch == 0
        {
            return Err(format!(
                "--pen-lr and --pen-dense-lr must be positive and --pen-batch at least 1: {self:?}"
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
const MOBILITY_INPUT: usize = PATTERN_COUNT * PEN_DIM;

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

/// The dense parameters in one vector: hidden weights (unit-major), hidden
/// biases, output weights, output bias.
const W1: usize = 0;
const B1: usize = W1 + PEN_HIDDEN * PEN_INPUTS;
const W2: usize = B1 + PEN_HIDDEN;
const B2: usize = W2 + PEN_HIDDEN;
const DENSE: usize = B2 + 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct PenWeights {
    pub linear: PhaseWeights,
    embeddings: Vec<f32>,
    embedding_squares: Vec<f32>,
    /// Samples that used each pattern state.
    seen: Vec<u32>,
    dense: Vec<f32>,
    adam: Adam,
    steps: u64,
}

/// The forward pass of one sample, kept for the backward pass.
struct Pass {
    features: Features,
    sums: [f32; PATTERN_COUNT * PEN_DIM],
    inputs: [f32; PEN_INPUTS],
    pre: [f32; PEN_HIDDEN],
    hidden: [f32; PEN_HIDDEN],
    output: f32,
}

fn mobility_input(moves: usize) -> f32 {
    (moves * 4).min(127) as f32 / 127.0
}

impl PenWeights {
    pub fn new(records: &[Record], seed: u64) -> Self {
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
            linear: PhaseWeights::new(records),
            embeddings: vec![EMBEDDING_INIT; PATTERN_WEIGHT_COUNT * PEN_DIM],
            embedding_squares: vec![0.0; PATTERN_WEIGHT_COUNT * PEN_DIM],
            seen: vec![0; PATTERN_WEIGHT_COUNT],
            dense,
            adam: Adam::new(DENSE),
            steps: 0,
        }
    }

    fn forward(&self, board: &Board) -> Pass {
        let features = Features::of(board);
        let mut sums = [0.0; PATTERN_COUNT * PEN_DIM];
        for (i, &state) in features.patterns.iter().enumerate() {
            let p = i / 4;
            let e = &self.embeddings[state as usize * PEN_DIM..][..PEN_DIM];
            for (s, v) in sums[p * PEN_DIM..][..PEN_DIM].iter_mut().zip(e) {
                *s += v;
            }
        }
        let mut inputs = [0.0; PEN_INPUTS];
        for (x, s) in inputs.iter_mut().zip(&sums) {
            *x = s.clamp(0.0, 1.0);
        }
        let opponent = Board::from_bits(board.opponent_bits(), board.player_bits()).unwrap();
        inputs[MOBILITY_INPUT] = mobility_input(board.legal_moves().len());
        inputs[MOBILITY_INPUT + 1] = mobility_input(opponent.legal_moves().len());
        let mut pre = [0.0; PEN_HIDDEN];
        let mut hidden = [0.0; PEN_HIDDEN];
        let mut output = self.dense[B2];
        for j in 0..PEN_HIDDEN {
            let w = &self.dense[W1 + j * PEN_INPUTS..][..PEN_INPUTS];
            let mut sum = self.dense[B1 + j];
            for k in 0..PEN_INPUTS {
                sum += w[k] * inputs[k];
            }
            pre[j] = sum;
            hidden[j] = sum.clamp(0.0, 1.0);
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
        for r in records {
            let pass = self.forward(&r.board);
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
        m.finish()
    }

    fn backward(&mut self, pass: &Pass, grad: f32, grads: &mut [f32], pen: &PenSettings) {
        grads[B2] += grad;
        let mut d_pre = [0.0; PEN_HIDDEN];
        for j in 0..PEN_HIDDEN {
            grads[W2 + j] += grad * pass.hidden[j];
            let slope = if pass.pre[j] > 0.0 && pass.pre[j] < 1.0 {
                1.0
            } else {
                LEAK
            };
            d_pre[j] = grad * self.dense[W2 + j] * slope;
        }
        let mut d_inputs = [0.0; PATTERN_COUNT * PEN_DIM];
        for j in 0..PEN_HIDDEN {
            let d = d_pre[j];
            if d == 0.0 {
                continue;
            }
            grads[B1 + j] += d;
            let row = W1 + j * PEN_INPUTS;
            for k in 0..PEN_INPUTS {
                grads[row + k] += d * pass.inputs[k];
            }
            for (di, w) in d_inputs.iter_mut().zip(&self.dense[row..]) {
                *di += d * w;
            }
        }
        for (k, d) in d_inputs.iter_mut().enumerate() {
            if !(pass.sums[k] > 0.0 && pass.sums[k] < 1.0) {
                *d *= LEAK;
            }
        }
        let features: &[u32; FEATURE_COUNT] = &pass.features.patterns;
        for (i, &state) in features.iter().enumerate() {
            let p = i / 4;
            let base = state as usize * PEN_DIM;
            for d in 0..PEN_DIM {
                let g = d_inputs[p * PEN_DIM + d];
                if g == 0.0 {
                    continue;
                }
                let sq = &mut self.embedding_squares[base + d];
                *sq += g * g;
                let e = &mut self.embeddings[base + d];
                *e = (*e - pen.lr * g / (sq.sqrt() + 1e-8)).clamp(-1.0, 1.0);
            }
            self.seen[state as usize] += 1;
        }
    }

    fn dense_step(&mut self, grads: &mut [f32], n: u32, pen: &PenSettings) {
        for g in grads.iter_mut() {
            *g /= n as f32;
        }
        self.steps += 1;
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

    /// Engine weights. `shrink` pulls the linear weights toward 0 and the
    /// embeddings toward their common start, as `PhaseWeights::export` does.
    pub fn export(&self, shrink: f32) -> (PatternPhase, PenPhase) {
        let embeddings = (0..PATTERN_WEIGHT_COUNT)
            .map(|state| {
                let seen = self.seen[state] as f32;
                let keep = if seen + shrink > 0.0 {
                    seen / (seen + shrink)
                } else {
                    0.0
                };
                std::array::from_fn(|d| {
                    let e = self.embeddings[state * PEN_DIM + d];
                    let e = EMBEDDING_INIT + (e - EMBEDDING_INIT) * keep;
                    (e * 127.0).round().clamp(-127.0, 127.0) as i8
                })
            })
            .collect();
        let hidden: [[i8; PEN_INPUTS]; PEN_HIDDEN] = std::array::from_fn(|j| {
            std::array::from_fn(|k| {
                (self.dense[W1 + j * PEN_INPUTS + k] * 64.0)
                    .round()
                    .clamp(-127.0, 127.0) as i8
            })
        });
        let hidden_bias =
            std::array::from_fn(|j| (self.dense[B1 + j] * 127.0 * 64.0).round() as i32);
        let output = std::array::from_fn(|j| {
            (self.dense[W2 + j] * 128.0 * 128.0 / 127.0)
                .round()
                .clamp(-65536.0, 65536.0) as i32
        });
        let output_bias = (self.dense[B2] * 128.0).round() as i32;
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
    };
    const PEN: PenSettings = PenSettings {
        lr: 0.002,
        dense_lr: 0.001,
        batch: 8,
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
        let mut linear = PhaseWeights::new(train);
        let mut pen = PenWeights::new(train, 1);
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
        let mut pen = PenWeights::new(&records, 2);
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
        assert_eq!(&bytes[..8], b"DPR2\x03\x00\x00\x00");
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
