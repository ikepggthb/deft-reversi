//! The weights of one phase and their training.
//!
//! Every phase has its own weights and only sees positions with its own disc
//! count, so phases train independently: one at a time per thread, with the
//! whole phase in memory and shuffled every epoch.
//!
//! Three things keep the exported weights close to what training fitted:
//! - A pattern state and its mirror image share one weight (`mirror`), so
//!   the model values a position and its mirror image alike.
//! - Every position reads four weights of each pattern table and one
//!   mobility weight, so a constant added to a whole table only moves the
//!   score, and AdaGrad lets such offsets drift into the tables. After
//!   every pass `recenter` moves each table's mean (weighted by use) into
//!   the bias, which changes no prediction. Shrinking a rarely used weight
//!   toward 0 then pulls it toward an average state, not toward an offset.
//! - Rarely used mobility weights are pulled toward their well-used
//!   neighbours instead of 0, so the table has no cliff at its ends
//!   (`export`).
use crate::rd::Record;
use deft_reversi_engine::{
    Board, Features, MOBILITY_WEIGHT_COUNT, PATTERN_WEIGHT_COUNT, PatternPhase, mirror_weights,
    pattern_table_sizes,
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// For every pattern weight, the one it shares with its mirror image: the
/// smaller of the two indices.
pub fn shared_weights() -> &'static [u32] {
    static SHARED: OnceLock<Vec<u32>> = OnceLock::new();
    SHARED.get_or_init(|| {
        let mirror = mirror_weights();
        (0..mirror.len() as u32)
            .map(|i| i.min(mirror[i as usize]))
            .collect()
    })
}

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
    /// Share each pattern weight with its mirror image's.
    pub mirror: bool,
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
    /// See `Settings::mirror`.
    mirror: bool,
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
    pub fn new(records: &[Record], mirror: bool) -> Self {
        let mean =
            records.iter().map(|r| f64::from(r.value)).sum::<f64>() / records.len().max(1) as f64;
        Self {
            patterns: vec![Slot::default(); PATTERN_WEIGHT_COUNT],
            mobility: vec![Slot::default(); MOBILITY_WEIGHT_COUNT],
            bias: Slot {
                weight: mean as f32,
                ..Slot::default()
            },
            mirror,
        }
    }

    /// Adds `x` discs to the bias, for an offset moved out of another term.
    pub fn add_to_bias(&mut self, x: f32) {
        self.bias.weight += x;
    }

    /// Whether mirror images share their weights.
    pub fn mirror(&self) -> bool {
        self.mirror
    }

    /// The features of `board`, with mirror images sharing their weights.
    pub fn features(&self, board: &Board) -> Features {
        let mut f = Features::of(board);
        if self.mirror {
            let shared = shared_weights();
            for i in &mut f.patterns {
                *i = shared[*i as usize];
            }
        }
        f
    }

    /// Moves the mean of every pattern table and of the mobility table,
    /// each weight weighted by the positions that used it, into the bias. Predictions stay the same: every
    /// position reads four weights of each pattern table and one mobility
    /// weight.
    pub fn recenter(&mut self) {
        fn mean(slots: &[Slot]) -> f32 {
            let (sum, n) = slots.iter().fold((0.0f64, 0.0f64), |(s, n), slot| {
                let seen = f64::from(slot.seen);
                (s + seen * f64::from(slot.weight), n + seen)
            });
            if n > 0.0 { (sum / n) as f32 } else { 0.0 }
        }
        let mut offset = 0;
        for size in pattern_table_sizes() {
            let table = &mut self.patterns[offset..offset + size];
            let m = mean(table);
            for slot in table.iter_mut() {
                slot.weight -= m;
            }
            self.bias.weight += 4.0 * m;
            offset += size;
        }
        let m = mean(&self.mobility);
        for slot in &mut self.mobility {
            slot.weight -= m;
        }
        self.bias.weight += m;
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
            let f = self.features(&r.board);
            let error = self.predict(&f) - f32::from(r.value);
            m.add(error, s.huber_delta);
            self.update(&f, error.clamp(-s.huber_delta, s.huber_delta), s);
        }
        self.recenter();
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
            let error = self.predict(&self.features(&r.board)) - f32::from(r.value);
            m.add(error, huber_delta);
        }
        m.finish()
    }

    /// Engine weights, scaled by 128. `shrink` pulls weights seen in few
    /// samples toward 0, the mean of their table after `recenter`: a weight
    /// seen n times is multiplied by n / (n + shrink). Mobility weights are
    /// pulled toward `mobility_targets` instead. Mirror images get the
    /// weight they share.
    pub fn export(&self, shrink: f32) -> PatternPhase {
        let shared = shared_weights();
        let patterns = (0..self.patterns.len())
            .map(|i| {
                let i = if self.mirror { shared[i] as usize } else { i };
                self.patterns[i].quantized(shrink)
            })
            .collect();
        let targets = mobility_targets(&self.mobility, shrink.max(1.0));
        let mobility = std::array::from_fn(|i| {
            let slot = &self.mobility[i];
            let seen = slot.seen as f32;
            let keep = if seen + shrink > 0.0 {
                seen / (seen + shrink)
            } else {
                0.0
            };
            let weight = targets[i] + (slot.weight - targets[i]) * keep;
            (weight * 128.0)
                .round()
                .clamp(i16::MIN.into(), i16::MAX.into()) as i16
        });
        // The bias is never shrunk, used or not: without linear training
        // (`--pen-no-linear`) it still holds the mean label and the
        // network's moved offset.
        let bias = (self.bias.weight * 128.0)
            .round()
            .clamp(i16::MIN.into(), i16::MAX.into()) as i16;
        PatternPhase::from_flat(patterns, mobility, bias).unwrap()
    }
}

/// What each mobility weight is pulled toward when few samples used it:
/// the weights seen at least `reliable` times keep their value; beyond the
/// outermost of them a weight takes the outermost one's value, and between
/// two of them the straight line between their values. Without any such
/// weight every target is 0.
fn mobility_targets(slots: &[Slot], reliable: f32) -> Vec<f32> {
    let anchors: Vec<usize> = (0..slots.len())
        .filter(|&i| slots[i].seen as f32 >= reliable)
        .collect();
    let (Some(&first), Some(&last)) = (anchors.first(), anchors.last()) else {
        return vec![0.0; slots.len()];
    };
    (0..slots.len())
        .map(|i| {
            if i <= first {
                return slots[first].weight;
            }
            if i >= last {
                return slots[last].weight;
            }
            let k = anchors.partition_point(|&a| a <= i);
            let (a, b) = (anchors[k - 1], anchors[k]);
            if a == i {
                return slots[i].weight;
            }
            let t = (i - a) as f32 / (b - a) as f32;
            slots[a].weight + (slots[b].weight - slots[a].weight) * t
        })
        .collect()
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
        mirror: true,
    };

    #[test]
    fn training_fits_a_repeated_position() {
        let board = Board::new();
        let records = vec![Record { board, value: 10 }; 1];
        let mut w = PhaseWeights::new(&[Record { board, value: 0 }], true);
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
        let mut w = PhaseWeights::new(&records, true);
        for _ in 0..20 {
            w.train(&records, &SETTINGS);
        }
        let phase = Features::of(&records[0].board).phase;
        let model = crate::model::assemble(&mut |p| (p == phase).then(|| w.export(0.0))).unwrap();
        let model = PatternModel::read_from(&model[..]).unwrap();
        for r in &records {
            let predicted = w.predict(&w.features(&r.board)).clamp(-64.0, 64.0);
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
        let mut w = PhaseWeights::new(
            &[Record {
                board: Board::new(),
                value: 0,
            }],
            true,
        );
        // The initial board is symmetric: orientations share indices.
        let f = w.features(&Board::new());
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
    fn recentering_keeps_predictions_and_centres_the_tables() {
        let mut random = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(8);
        let records = crate::test_records(&mut random, 24, 300);
        let mut w = PhaseWeights::new(&records, true);
        for _ in 0..3 {
            w.train(&records, &SETTINGS);
        }
        // Offsets a recentring removes.
        for slot in &mut w.patterns[..59049] {
            slot.weight += 2.5;
        }
        for slot in &mut w.mobility {
            slot.weight -= 1.5;
        }
        let before: Vec<f32> = records
            .iter()
            .map(|r| w.predict(&w.features(&r.board)))
            .collect();
        w.recenter();
        for (r, b) in records.iter().zip(before) {
            assert!((w.predict(&w.features(&r.board)) - b).abs() < 1e-3);
        }
        let mean = |slots: &[Slot]| {
            let n: f64 = slots.iter().map(|s| f64::from(s.seen)).sum();
            slots
                .iter()
                .map(|s| f64::from(s.seen) * f64::from(s.weight))
                .sum::<f64>()
                / n
        };
        assert!(mean(&w.patterns[..59049]).abs() < 1e-4);
        assert!(mean(&w.mobility).abs() < 1e-4);
    }

    #[test]
    fn mirror_images_evaluate_alike() {
        let mut random = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(6);
        let records = crate::test_records(&mut random, 30, 400);
        let flip = |bits: u64| {
            (0..64)
                .filter(|sq| bits >> sq & 1 != 0)
                .fold(0u64, |acc, sq| acc | 1 << (sq / 8 * 8 + 7 - sq % 8))
        };
        for mirror in [true, false] {
            let mut w = PhaseWeights::new(&records, mirror);
            for _ in 0..5 {
                w.train(&records, &SETTINGS);
            }
            let model = crate::model::assemble(&mut |_| Some(w.export(4.0))).unwrap();
            let model = PatternModel::read_from(&model[..]).unwrap();
            let differ = records
                .iter()
                .filter(|r| {
                    let b = r.board;
                    let image =
                        Board::from_bits(flip(b.player_bits()), flip(b.opponent_bits())).unwrap();
                    model.evaluate(&b) != model.evaluate(&image)
                })
                .count();
            assert_eq!(differ == 0, mirror, "mirror {mirror}: {differ} differ");
        }
    }

    #[test]
    fn rare_mobility_weights_lean_on_their_neighbours() {
        let slot = |weight: f32, seen: u32| Slot {
            weight,
            grad_squares: 0.0,
            seen,
        };
        let mut slots = vec![slot(-0.3, 0); MOBILITY_WEIGHT_COUNT];
        slots[60] = slot(-4.0, 100);
        slots[62] = slot(-2.0, 100);
        slots[63] = slot(9.0, 1);
        slots[64] = slot(1.0, 100);
        slots[70] = slot(4.0, 100);
        let targets = mobility_targets(&slots, 32.0);
        assert_eq!(targets[0], -4.0);
        assert_eq!(targets[59], -4.0);
        assert_eq!(targets[63], -0.5);
        assert_eq!(targets[67], 2.5);
        assert_eq!(targets[127], 4.0);
        let mut w = PhaseWeights::new(&[], true);
        w.mobility = slots;
        let exported = w.export(32.0);
        let model = PatternModel::new(vec![exported; 60]).unwrap();
        let mut bytes = Vec::new();
        model.write_to(&mut bytes).unwrap();
        // The mobility weights follow the pattern tables.
        let mobility: Vec<i16> = bytes[8 + 2 * PATTERN_WEIGHT_COUNT..][..256]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(mobility[0], -4 * 128);
        assert_eq!(mobility[127], 4 * 128);
        // Seen once of 33: 1/33 of the way from the line to its own value.
        let expected: f32 = (-0.5 + (9.0 + 0.5) / 33.0) * 128.0;
        assert_eq!(mobility[63], expected.round() as i16);
        assert!(mobility_targets(&[slot(1.0, 3); 4], 32.0) == vec![0.0; 4]);
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
