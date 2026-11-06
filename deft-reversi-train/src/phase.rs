//! The weights of one phase and their training.
//!
//! Every phase has its own weights and only sees positions with its own disc
//! count, so phases train independently: one at a time per thread, with the
//! whole phase in memory and shuffled every epoch.
use crate::rd::Record;
use deft_reversi_engine::{Features, MOBILITY_WEIGHT_COUNT, PATTERN_WEIGHT_COUNT, PatternPhase};
use serde::{Deserialize, Serialize};

/// Optimisation settings. They are stored in checkpoints, and resuming
/// requires the same values.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// AdaGrad step size, in discs.
    pub lr: f32,
    /// L2 penalty added to every gradient, times the weight.
    pub l2: f32,
    /// Errors beyond this many discs count linearly (Huber loss).
    pub huber_delta: f32,
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        let positive = |x: f32| x.is_finite() && x > 0.0;
        if !positive(self.lr)
            || !positive(self.huber_delta)
            || !(self.l2.is_finite() && self.l2 >= 0.0)
        {
            return Err(format!(
                "--lr and --huber-delta must be positive and --l2 nonnegative: {self:?}"
            ));
        }
        Ok(())
    }
}

/// A weight with its AdaGrad state, together so one update touches one
/// cache line.
#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize)]
struct Slot {
    weight: f32,
    grad_squares: f32,
    /// Training samples that used this weight.
    seen: u32,
}

impl Slot {
    #[inline]
    fn update(&mut self, grad: f32, s: &Settings) {
        let grad = grad + s.l2 * self.weight;
        self.grad_squares += grad * grad;
        self.weight -= s.lr * grad / (self.grad_squares.sqrt() + 1e-8);
        self.seen += 1;
    }

    /// The weight times 128, pulled toward 0 when few samples used it.
    fn quantized(&self, shrink: f32) -> i16 {
        let seen = self.seen as f32;
        let weight = if seen + shrink > 0.0 {
            self.weight * seen / (seen + shrink)
        } else {
            0.0
        };
        (weight * 128.0)
            .round()
            .clamp(i16::MIN.into(), i16::MAX.into()) as i16
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PhaseWeights {
    patterns: Vec<Slot>,
    mobility: Vec<Slot>,
    bias: Slot,
}

/// Mean Huber loss and mean absolute error, in discs.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub loss: f64,
    pub mae: f64,
    pub samples: u64,
}

impl PhaseWeights {
    /// Zero weights with the bias at the mean label, so early steps fit the
    /// patterns rather than the offset.
    pub fn new(records: &[Record]) -> Self {
        let mean =
            records.iter().map(|r| f64::from(r.value)).sum::<f64>() / records.len().max(1) as f64;
        Self {
            patterns: vec![Slot::default(); PATTERN_WEIGHT_COUNT],
            mobility: vec![Slot::default(); MOBILITY_WEIGHT_COUNT],
            bias: Slot {
                weight: mean as f32,
                ..Slot::default()
            },
        }
    }

    pub fn predict(&self, f: &Features) -> f32 {
        let mut sum = self.bias.weight + self.mobility[f.mobility].weight;
        for &i in &f.patterns {
            sum += self.patterns[i as usize].weight;
        }
        sum
    }

    /// One pass over `records` in their order, updating after every sample.
    pub fn train(&mut self, records: &[Record], s: &Settings) -> Metrics {
        let mut m = Accumulator::default();
        for r in records {
            let f = Features::of(&r.board);
            let error = self.predict(&f) - f32::from(r.value);
            m.add(error, s.huber_delta);
            self.update(&f, error.clamp(-s.huber_delta, s.huber_delta), s);
        }
        m.finish()
    }

    /// One step for one sample, with `grad` the clipped error.
    pub fn update(&mut self, f: &Features, grad: f32, s: &Settings) {
        // Symmetric discs can make two orientations read the same weight;
        // its gradient is then the sum, applied in one step.
        let mut indices = f.patterns;
        indices.sort_unstable();
        for run in indices.chunk_by(|a, b| a == b) {
            self.patterns[run[0] as usize].update(grad * run.len() as f32, s);
        }
        self.mobility[f.mobility].update(grad, s);
        self.bias.update(grad, s);
    }

    pub fn validate(&self, records: &[Record], huber_delta: f32) -> Metrics {
        let mut m = Accumulator::default();
        for r in records {
            let error = self.predict(&Features::of(&r.board)) - f32::from(r.value);
            m.add(error, huber_delta);
        }
        m.finish()
    }

    /// Engine weights, scaled by 128. `shrink` pulls weights seen in few
    /// samples toward 0: a weight seen n times is multiplied by
    /// n / (n + shrink).
    pub fn export(&self, shrink: f32) -> PatternPhase {
        let patterns = self.patterns.iter().map(|s| s.quantized(shrink)).collect();
        let mobility = std::array::from_fn(|i| self.mobility[i].quantized(shrink));
        PatternPhase::from_flat(patterns, mobility, self.bias.quantized(0.0)).unwrap()
    }
}

#[derive(Default)]
pub struct Accumulator {
    loss: f64,
    abs: f64,
    n: u64,
}

impl Accumulator {
    pub fn add(&mut self, error: f32, delta: f32) {
        let e = f64::from(error.abs());
        let d = f64::from(delta);
        self.loss += if e <= d {
            0.5 * e * e
        } else {
            d * (e - 0.5 * d)
        };
        self.abs += e;
        self.n += 1;
    }

    pub fn finish(self) -> Metrics {
        let n = self.n.max(1) as f64;
        Metrics {
            loss: self.loss / n,
            mae: self.abs / n,
            samples: self.n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Board, Model, PatternModel};

    const SETTINGS: Settings = Settings {
        lr: 0.05,
        l2: 0.0,
        huber_delta: 4.0,
    };

    #[test]
    fn training_fits_a_repeated_position() {
        let board = Board::new();
        let records = vec![Record { board, value: 10 }; 1];
        let mut w = PhaseWeights::new(&[Record { board, value: 0 }]);
        let first = w.validate(&records, 4.0).mae;
        for _ in 0..2000 {
            w.train(&records, &SETTINGS);
        }
        let last = w.validate(&records, 4.0).mae;
        assert!(first > 9.9 && last < 0.1, "{first} -> {last}");
    }

    #[test]
    fn exported_weights_evaluate_like_the_prediction() {
        let mut random = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(3);
        let records = crate::test_records(&mut random, 20, 200);
        let mut w = PhaseWeights::new(&records);
        for _ in 0..20 {
            w.train(&records, &SETTINGS);
        }
        let phase = Features::of(&records[0].board).phase;
        let model = crate::model::assemble(&mut |p| (p == phase).then(|| w.export(0.0))).unwrap();
        let model = PatternModel::read_from(&model[..]).unwrap();
        for r in &records {
            let predicted = w.predict(&Features::of(&r.board)).clamp(-64.0, 64.0);
            let engine = model.evaluate(&r.board) as f32;
            // Rounding every weight to 1/128 adds at most 46/256 discs.
            assert!((predicted - engine).abs() <= 0.5 + 46.0 / 256.0);
        }
        assert!(
            Model::from_bytes(&crate::model::assemble(&mut |_| Some(w.export(32.0))).unwrap())
                .is_ok()
        );
    }

    #[test]
    fn a_weight_read_twice_takes_one_step_with_the_summed_gradient() {
        let mut w = PhaseWeights::new(&[Record {
            board: Board::new(),
            value: 0,
        }]);
        // The initial board is symmetric: orientations share indices.
        let f = Features::of(&Board::new());
        let mut sorted = f.patterns;
        sorted.sort_unstable();
        assert!(sorted.windows(2).any(|p| p[0] == p[1]));
        w.train(
            &[Record {
                board: Board::new(),
                value: 1,
            }],
            &SETTINGS,
        );
        for &i in &f.patterns {
            assert_eq!(w.patterns[i as usize].seen, 1);
        }
        assert!(
            Settings {
                lr: f32::NAN,
                ..SETTINGS
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                huber_delta: -1.0,
                ..SETTINGS
            }
            .validate()
            .is_err()
        );
        assert!(SETTINGS.validate().is_ok());
    }

    #[test]
    fn shrink_pulls_rarely_seen_weights_to_zero() {
        let slot = Slot {
            weight: 1.0,
            grad_squares: 0.0,
            seen: 32,
        };
        assert_eq!(slot.quantized(0.0), 128);
        assert_eq!(slot.quantized(32.0), 64);
        assert_eq!(Slot::default().quantized(0.0), 0);
    }
}
