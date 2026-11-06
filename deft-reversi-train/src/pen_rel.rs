//! PEN with a relation path between the four orientations, for measurement
//! only (the engine cannot read it).
//!
//! The summed PEN sees each pattern as the sum of its four orientations, so
//! it cannot tell which side a shape is on. The relation path looks at the
//! four orientations together: orientation `r` is the board turned `r`
//! quarters, and its 11 pattern instances describe one view of the board.
//! For every view, one layer shared by the four views reads the view with
//! its right, opposite and left neighbours (in that order), so a top edge
//! can be weighed by the state of both side edges; the four results are
//! summed, which keeps the evaluation the same when the board is turned.
//!
//! - `plain`: each view's 11 clamped embeddings (88 values) are projected to
//!   16 values by a shared `P`, then mixed: 64 -> 16.
//! - `light`: each state has 8 relation values of its own; a view is the
//!   clamped sum of its 11 instances' relation values (8), then mixed:
//!   32 -> 8. No projection, and about 1,300 multiply-adds more than the
//!   summed PEN instead of about 9,700.
//!
//! The mixed result is added to the summed PEN's inputs before its MLP.
use crate::{
    pen::{Adam, EMBEDDING_INIT, HIDDEN_MAX, LEAK, PenSettings},
    phase::{Accumulator, Metrics, PhaseWeights, Settings},
    rd::Record,
};
use clap::ValueEnum;
use deft_reversi_engine::{
    Board, Features, PATTERN_COUNT, PATTERN_WEIGHT_COUNT, PEN_DIM, PEN_HIDDEN,
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum Relation {
    Plain,
    Light,
}

const VIEW: usize = PATTERN_COUNT * PEN_DIM;
const SUMMED: usize = PATTERN_COUNT * PEN_DIM;
const MAX_Z: usize = 16;
const MAX_U: usize = 16;

#[derive(Clone, Copy)]
struct Shape {
    relation: Relation,
}

impl Shape {
    /// Values per view after the projection (or the relation table).
    fn z(self) -> usize {
        match self.relation {
            Relation::Plain => 16,
            Relation::Light => 8,
        }
    }
    /// Outputs of the mixing layer.
    fn u(self) -> usize {
        match self.relation {
            Relation::Plain => 16,
            Relation::Light => 8,
        }
    }
    fn mix_in(self) -> usize {
        4 * self.z()
    }
    fn inputs(self) -> usize {
        SUMMED + self.u() + 2
    }
    // Dense parameters, back to back.
    fn w1(self) -> usize {
        0
    }
    fn b1(self) -> usize {
        PEN_HIDDEN * self.inputs()
    }
    fn w2(self) -> usize {
        self.b1() + PEN_HIDDEN
    }
    fn b2(self) -> usize {
        self.w2() + PEN_HIDDEN
    }
    fn m(self) -> usize {
        self.b2() + 1
    }
    fn mb(self) -> usize {
        self.m() + self.u() * self.mix_in()
    }
    fn p(self) -> usize {
        self.mb() + self.u()
    }
    fn pb(self) -> usize {
        self.p()
            + match self.relation {
                Relation::Plain => self.z() * VIEW,
                Relation::Light => 0,
            }
    }
    fn dense(self) -> usize {
        self.pb()
            + match self.relation {
                Relation::Plain => self.z(),
                Relation::Light => 0,
            }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RelWeights {
    relation: Relation,
    pub linear: PhaseWeights,
    embeddings: Vec<f32>,
    embedding_squares: Vec<f32>,
    /// `light` only: 8 relation values per state.
    relations: Vec<f32>,
    relation_squares: Vec<f32>,
    dense: Vec<f32>,
    adam: Adam,
    steps: u64,
}

struct Pass {
    features: Features,
    sums: [f32; SUMMED],
    /// `plain`: the clamped embeddings of each view.
    views: [[f32; VIEW]; 4],
    /// Before the clamp (`light`) or the projection's output (`plain`).
    z_raw: [[f32; MAX_Z]; 4],
    z: [[f32; MAX_Z]; 4],
    mix_pre: [[f32; MAX_U]; 4],
    inputs: Vec<f32>,
    pre: [f32; PEN_HIDDEN],
    hidden: [f32; PEN_HIDDEN],
    output: f32,
}

fn mobility_input(moves: usize) -> f32 {
    (moves * 4).min(127) as f32 / 127.0
}

fn open(x: f32) -> bool {
    x > 0.0 && x < 1.0
}

impl RelWeights {
    pub fn new(records: &[Record], seed: u64, relation: Relation) -> Self {
        let sh = Shape { relation };
        let mut rng = StdRng::seed_from_u64(seed);
        let mut dense = vec![0.0; sh.dense()];
        let mut uniform = |range: std::ops::Range<usize>, fan: usize, dense: &mut Vec<f32>| {
            let bound = 0.5 * (6.0 / fan as f32).sqrt();
            for w in &mut dense[range] {
                *w = rng.gen_range(-bound..bound);
            }
        };
        uniform(sh.w1()..sh.b1(), sh.inputs() + PEN_HIDDEN, &mut dense);
        dense[sh.b1()..sh.w2()].fill(0.5);
        uniform(sh.m()..sh.mb(), sh.mix_in() + sh.u(), &mut dense);
        dense[sh.mb()..sh.p()].fill(0.5);
        if relation == Relation::Plain {
            uniform(sh.p()..sh.pb(), VIEW + sh.z(), &mut dense);
        }
        let relations = match relation {
            // A view sums 11 instances; start each view near 0.5.
            Relation::Light => vec![0.5 / PATTERN_COUNT as f32; PATTERN_WEIGHT_COUNT * 8],
            Relation::Plain => Vec::new(),
        };
        Self {
            relation,
            linear: PhaseWeights::new(records),
            embeddings: vec![EMBEDDING_INIT; PATTERN_WEIGHT_COUNT * PEN_DIM],
            embedding_squares: vec![0.0; PATTERN_WEIGHT_COUNT * PEN_DIM],
            relation_squares: vec![0.0; relations.len()],
            relations,
            adam: Adam::new(dense.len()),
            dense,
            steps: 0,
        }
    }

    pub fn relation(&self) -> Relation {
        self.relation
    }

    fn forward(&self, board: &Board) -> Pass {
        let sh = Shape {
            relation: self.relation,
        };
        let (nz, nu) = (sh.z(), sh.u());
        let features = Features::of(board);
        let mut sums = [0.0; SUMMED];
        let mut views = [[0.0; VIEW]; 4];
        let mut z_raw = [[0.0; MAX_Z]; 4];
        for (i, &state) in features.patterns.iter().enumerate() {
            let (p, r) = (i / 4, i % 4);
            let e = &self.embeddings[state as usize * PEN_DIM..][..PEN_DIM];
            for d in 0..PEN_DIM {
                sums[p * PEN_DIM + d] += e[d];
                views[r][p * PEN_DIM + d] = e[d].clamp(0.0, 1.0);
            }
            if self.relation == Relation::Light {
                let rel = &self.relations[state as usize * 8..][..8];
                for d in 0..8 {
                    z_raw[r][d] += rel[d];
                }
            }
        }
        let mut z = [[0.0; MAX_Z]; 4];
        for r in 0..4 {
            match self.relation {
                Relation::Light => {
                    for d in 0..nz {
                        z[r][d] = z_raw[r][d].clamp(0.0, 1.0);
                    }
                }
                Relation::Plain => {
                    for k in 0..nz {
                        let row = &self.dense[sh.p() + k * VIEW..][..VIEW];
                        let mut v = self.dense[sh.pb() + k];
                        for (w, x) in row.iter().zip(&views[r]) {
                            v += w * x;
                        }
                        z_raw[r][k] = v;
                        z[r][k] = v;
                    }
                }
            }
        }
        let mut mix_pre = [[0.0; MAX_U]; 4];
        let mut t = [0.0; MAX_U];
        for r in 0..4 {
            for j in 0..nu {
                let row = &self.dense[sh.m() + j * sh.mix_in()..][..sh.mix_in()];
                let mut v = self.dense[sh.mb() + j];
                for k in 0..4 {
                    let zz = &z[(r + k) % 4];
                    for d in 0..nz {
                        v += row[k * nz + d] * zz[d];
                    }
                }
                mix_pre[r][j] = v;
                t[j] += v.clamp(0.0, 1.0);
            }
        }
        let mut inputs = vec![0.0; sh.inputs()];
        for (x, s) in inputs.iter_mut().zip(&sums) {
            *x = s.clamp(0.0, 1.0);
        }
        inputs[SUMMED..SUMMED + nu].copy_from_slice(&t[..nu]);
        let opponent = Board::from_bits(board.opponent_bits(), board.player_bits()).unwrap();
        inputs[SUMMED + nu] = mobility_input(board.legal_moves().len());
        inputs[SUMMED + nu + 1] = mobility_input(opponent.legal_moves().len());
        let n = sh.inputs();
        let mut pre = [0.0; PEN_HIDDEN];
        let mut hidden = [0.0; PEN_HIDDEN];
        let mut output = self.dense[sh.b2()];
        for j in 0..PEN_HIDDEN {
            let w = &self.dense[j * n..][..n];
            let mut v = self.dense[sh.b1() + j];
            for k in 0..n {
                v += w[k] * inputs[k];
            }
            pre[j] = v;
            hidden[j] = v.clamp(0.0, 1.0);
            output += self.dense[sh.w2() + j] * hidden[j];
        }
        Pass {
            features,
            sums,
            views,
            z_raw,
            z,
            mix_pre,
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

    pub fn train(&mut self, records: &[Record], s: &Settings, pen: &PenSettings) -> Metrics {
        let mut m = Accumulator::default();
        let mut grads = vec![0.0f32; self.dense.len()];
        let mut in_batch = 0;
        for r in records {
            let pass = self.forward(&r.board);
            let error = self.linear.predict(&pass.features) + pass.output - f32::from(r.value);
            m.add(error, s.huber_delta);
            let grad = error.clamp(-s.huber_delta, s.huber_delta);
            self.linear.update(&pass.features, grad, s);
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
        let sh = Shape {
            relation: self.relation,
        };
        let (nz, nu, n) = (sh.z(), sh.u(), sh.inputs());
        // Output layer and hidden layer.
        grads[sh.b2()] += grad;
        let mut d_inputs = vec![0.0f32; n];
        for j in 0..PEN_HIDDEN {
            grads[sh.w2() + j] += grad * pass.hidden[j];
            let slope = if open(pass.pre[j]) { 1.0 } else { LEAK };
            let d = grad * self.dense[sh.w2() + j] * slope;
            grads[sh.b1() + j] += d;
            let row = j * n;
            for k in 0..n {
                grads[row + k] += d * pass.inputs[k];
                d_inputs[k] += d * self.dense[row + k];
            }
        }
        // Mixing layer: every view's output feeds `t`.
        let mut d_z = [[0.0f32; MAX_Z]; 4];
        for r in 0..4 {
            for j in 0..nu {
                let slope = if open(pass.mix_pre[r][j]) { 1.0 } else { LEAK };
                let d = d_inputs[SUMMED + j] * slope;
                if d == 0.0 {
                    continue;
                }
                grads[sh.mb() + j] += d;
                let row = sh.m() + j * sh.mix_in();
                for k in 0..4 {
                    let v = (r + k) % 4;
                    for dd in 0..nz {
                        grads[row + k * nz + dd] += d * pass.z[v][dd];
                        d_z[v][dd] += d * self.dense[row + k * nz + dd];
                    }
                }
            }
        }
        // Into the views: the projection (`plain`) or the relation table
        // (`light`).
        let mut d_views = [[0.0f32; VIEW]; 4];
        match self.relation {
            Relation::Plain => {
                for r in 0..4 {
                    for k in 0..nz {
                        let d = d_z[r][k];
                        grads[sh.pb() + k] += d;
                        let row = sh.p() + k * VIEW;
                        for x in 0..VIEW {
                            grads[row + x] += d * pass.views[r][x];
                            d_views[r][x] += d * self.dense[row + x];
                        }
                    }
                }
            }
            Relation::Light => {
                for (i, &state) in pass.features.patterns.iter().enumerate() {
                    let r = i % 4;
                    let base = state as usize * 8;
                    for (d, (&dz, &raw)) in d_z[r].iter().zip(&pass.z_raw[r]).take(8).enumerate() {
                        let mut g = dz;
                        if !open(raw) {
                            g *= LEAK;
                        }
                        if g == 0.0 {
                            continue;
                        }
                        let sq = &mut self.relation_squares[base + d];
                        *sq += g * g;
                        let w = &mut self.relations[base + d];
                        *w = (*w - pen.lr * g / (sq.sqrt() + 1e-8)).clamp(-1.0, 1.0);
                    }
                }
            }
        }
        // Embeddings: through the summed inputs, and through the views.
        for (i, &state) in pass.features.patterns.iter().enumerate() {
            let (p, r) = (i / 4, i % 4);
            let base = state as usize * PEN_DIM;
            for d in 0..PEN_DIM {
                let k = p * PEN_DIM + d;
                let mut g = d_inputs[k] * if open(pass.sums[k]) { 1.0 } else { LEAK };
                if self.relation == Relation::Plain {
                    let e = self.embeddings[base + d];
                    g += d_views[r][k] * if open(e) { 1.0 } else { LEAK };
                }
                if g == 0.0 {
                    continue;
                }
                let sq = &mut self.embedding_squares[base + d];
                *sq += g * g;
                let e = &mut self.embeddings[base + d];
                *e = (*e - pen.lr * g / (sq.sqrt() + 1e-8)).clamp(-1.0, 1.0);
            }
        }
    }

    fn dense_step(&mut self, grads: &mut [f32], n: u32, pen: &PenSettings) {
        let sh = Shape {
            relation: self.relation,
        };
        for g in grads.iter_mut() {
            *g /= n as f32;
        }
        self.steps += 1;
        self.adam
            .step(&mut self.dense, grads, pen.dense_lr, self.steps);
        for range in [sh.w1()..sh.b1(), sh.m()..sh.mb()] {
            for w in &mut self.dense[range] {
                *w = w.clamp(-HIDDEN_MAX, HIDDEN_MAX);
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Turning the board a quarter leaves the evaluation unchanged.
    #[test]
    fn evaluation_is_the_same_on_turned_boards() {
        let mut random = StdRng::seed_from_u64(5);
        let records = crate::test_records(&mut random, 30, 600);
        for relation in [Relation::Plain, Relation::Light] {
            let mut w = RelWeights::new(&records, 3, relation);
            for _ in 0..3 {
                w.train(&records, &SETTINGS, &PEN);
            }
            for r in &records[..50] {
                let b = r.board;
                let turned = b.symmetries();
                let base = w.predict(&b);
                let rotations: Vec<f32> = turned.iter().map(|t| w.predict(t)).collect();
                let same = rotations
                    .iter()
                    .filter(|v| (*v - base).abs() < 1e-3)
                    .count();
                // The identity and the three quarter turns.
                assert!(same >= 4, "{relation:?}: {base} vs {rotations:?}");
            }
        }
    }

    #[test]
    fn the_network_learns_a_three_side_rule() {
        // Each side's own discs count up when both neighbouring sides hold a
        // disc of the mover and down otherwise: a rule over three sides that
        // turns with the board, which the summed PEN cannot place.
        let sides = [
            0x0000_0000_0000_00ffu64,
            0x8080_8080_8080_8080,
            0xff00_0000_0000_0000,
            0x0101_0101_0101_0101,
        ];
        let mut random = StdRng::seed_from_u64(9);
        let mut records = crate::test_records(&mut random, 30, 12000);
        for r in &mut records {
            let own = r.board.player_bits();
            let has = |i: usize| own & sides[i % 4] != 0;
            let v: i32 = (0..4)
                .map(|i| {
                    let n = (own & sides[i]).count_ones() as i32;
                    if has(i + 1) && has(i + 3) {
                        3 * n
                    } else {
                        -3 * n
                    }
                })
                .sum();
            r.value = v.clamp(-64, 64) as i8;
        }
        let (train, valid) = records.split_at(10000);
        for relation in [Relation::Plain, Relation::Light] {
            let mut w = RelWeights::new(train, 1, relation);
            let first = w.validate(valid, 4.0).mae;
            for _ in 0..10 {
                w.train(train, &SETTINGS, &PEN);
            }
            let last = w.validate(valid, 4.0).mae;
            assert!(last < first * 0.9, "{relation:?}: {first} -> {last}");
        }
    }
}
