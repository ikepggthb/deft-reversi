//! 1 フェーズ分の補助ニューラルネット（パターン埋め込みネットワーク）と、
//! それに併用する線形パターン重みを学習するモジュール。
//!
//! この補助ネットワークは、線形パターン評価だけでは捉えきれない「パターン同士の相互作用」や
//! 「お互いの着手可能手数などの盤面全体の情報」を補うための小型ニューラルネットである。
//! ここで学習した浮動小数点ネットワークは、エンジン側の高速な整数推論（`evaluation/pen.rs`）に
//! 対応しており、エンジンの `0..=127` に対して埋め込みと活性化関数出力を `0..=1`（埋め込みは `-1..=1`）で
//! 表現し、重みもエンジンの `i8` 範囲に収まるようクランプする。
//!
//! 最適化の役割分担：
//! - 線形重みとパターン埋め込みは、線形学習器と同様に 1 サンプルごとに AdaGrad で更新する。
//! - 全結合層（隠れ層・出力層）は `batch` サンプルごとの平均勾配を使って Adam で更新し、
//!   [`PenSettings::decay`] に従って重みを 0 方向へ減衰させる（Decoupled Weight Decay）。
//!
//! 定数オフセットの分離と中心化：
//! ネットワークと線形重みの間では、定数の足し引きを自由にやり取りできる。
//! そのため以前は、ネットワーク出力の平均が ±25 石ほどにずれ、線形テーブル側が逆符号のオフセットを
//! 持つことで、書き出し（export）時に出現回数の少ない重みを縮小した際に誤差が表面化していた。
//! そこで本実装では、各パスの最後にネットワークの平均出力を線形バイアスへ移し
//! （出力バイアス `B2` から同じ値を引くため予測結果は変わらない）、線形テーブル側も中心化
//! （[`PhaseWeights::recenter`]）する。さらに出現回数の少ない埋め込みは、初期値ではなく
//! 所属テーブルの平均埋め込みへ寄せる。
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

/// PEN 学習のハイパーパラメータ設定。チェックポイントに保存され、再開時には同じ値であることが要求される。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PenSettings {
    /// 埋め込みテーブル（AdaGrad）の学習率。
    pub lr: f32,
    /// 全結合層（Adam）の学習率。
    pub dense_lr: f32,
    /// Adam で 1 回更新するあたりのサンプル数（ミニバッチサイズ）。
    pub batch: u32,
    /// 全結合重み（バイアスを除く）に対する Decoupled Weight Decay（重み減衰）：
    /// 各 Adam ステップで重みに `1 - dense_lr * decay` を掛けて過学習を抑える。
    pub decay: f32,
}

impl PenSettings {
    /// 設定値が有効範囲内かを検証する。
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

/// 全埋め込みの初期値。未出現の状態同士が同じ値になるよう統一する。
const EMBEDDING_INIT: f32 = 0.125;
/// クリップ済み活性化関数（`[0, 1]`）の範囲外で通すリーク勾配。範囲外に出たユニットが復帰できるようにする。
const LEAK: f32 = 0.01;
/// エンジンの `i8`（スケール 64）で表現可能な隠れ層重みの最大絶対値。
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

/// 全結合層の全パラメータを 1 本のベクタに格納するためのオフセット定義。
/// 隠れ層重みは入力メジャー（入力 `k` に対する全ユニットの重みが `W1 + k * PEN_HIDDEN ..` に連続配置）とし、
/// 1 入力あたり 1 回のベクトル演算で全ユニットを更新できるようにする。
const W1: usize = 0;
const B1: usize = W1 + PEN_HIDDEN * PEN_INPUTS;
const W2: usize = B1 + PEN_HIDDEN;
const B2: usize = W2 + PEN_HIDDEN;
const DENSE: usize = B2 + 1;

/// 1 パターン状態の埋め込みベクトルと AdaGrad 状態を 1 キャッシュライン（64 バイト）に収めた構造体。
/// 1 サンプルにつき 44 個の状態をランダムアクセスするため、各状態のアクセスを 1 キャッシュミスに抑える。
#[derive(Clone, Copy, Serialize, Deserialize)]
#[repr(C, align(64))]
struct Embedding {
    value: [f32; PEN_DIM],
    squares: [f32; PEN_DIM],
}

/// 1 フェーズ分の PEN 学習重み（線形重み + パターン埋め込み + 全結合層 + Adam 状態）。
#[derive(Clone, Serialize, Deserialize)]
pub struct PenWeights {
    /// 併用する線形パターン重み。
    pub linear: PhaseWeights,
    embeddings: Vec<Embedding>,
    /// 各パターン状態が学習サンプルで参照された回数。
    seen: Vec<u32>,
    dense: Vec<f32>,
    adam: Adam,
    steps: u64,
}

/// 逆伝播用に保持する 1 サンプル分の順伝播中間値。
struct Pass {
    features: deft_reversi_engine::Features,
    sums: [f32; PATTERN_COUNT * PEN_DIM],
    inputs: [f32; PEN_INPUTS],
    pre: [f32; PEN_HIDDEN],
    hidden: [f32; PEN_HIDDEN],
    output: f32,
}

/// クリップ活性化関数の入力が `(0, 1)` 内のとき `1.0`、範囲外のとき `LEAK` を返す。
/// 入力がランダムに範囲内外へ落ちるため、分岐なし（branchless）で計算する。
#[inline]
fn inside_slope(x: f32) -> f32 {
    let inside = (x > 0.0) & (x < 1.0);
    LEAK + (1.0 - LEAK) * f32::from(u8::from(inside))
}

/// 44 個の参照パターン状態のうち重複を除いた各状態について `f(state, pattern, times)` を 1 回ずつ呼ぶ。
/// 各状態は 1 つのパターンテーブルに属するため、重複しうるのは同一パターンの 4 向きのみ。
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

// 全結合層のループ群はそれぞれ独立した関数に切り出す：
// 大きな順伝播・逆伝播関数へインライン展開するとコンパイラがスカラ化してしまうため（約 4 倍遅くなる）。

/// `bias + Σ_k inputs[k] · weights[k]` を全ユニット同時に計算する（`weights` は入力メジャー）。
/// ユニット単位ではなく入力単位で加算することで、ベクトルレジスタ上で独立した累積和を保てる。
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

/// 隠れ層重みの勾配 `grads[k][j] += inputs[k] · d[j]` を加算する。
#[inline(never)]
fn add_outer(grads: &mut [f32], inputs: &[f32; PEN_INPUTS], d: &[f32; PEN_HIDDEN]) {
    for (k, &x) in inputs.iter().enumerate() {
        let column = &mut grads[k * PEN_HIDDEN..][..PEN_HIDDEN];
        for (g, d) in column.iter_mut().zip(d) {
            *g += d * x;
        }
    }
}

/// 合算済み埋め込み入力の勾配（入力重みとユニット勾配 `d` の内積）を 8 レーン並列で計算する。
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
    /// 指定した学習レコードの平均値で線形バイアスを初期化し、PEN の重み・埋め込みを初期化する。
    pub fn new(records: &[Record], seed: u64, mirror: bool) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut dense = vec![0.0; DENSE];
        let bound = 0.5 * (6.0 / (PEN_INPUTS + PEN_HIDDEN) as f32).sqrt();
        for w in &mut dense[W1..B1] {
            *w = rng.gen_range(-bound..bound);
        }
        // 隠れユニットは活性域 `[0, 1]` の中央（0.5）から開始し、出力層重み・バイアスは 0 とすることで
        // 学習開始時点では線形モデルと同一の予測値からスタートする。
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
        // 入力単位で全ユニットを同時更新し、ベクトルレジスタ上で独立した累積和を保つ。
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

    /// 盤面 `board` に対する線形モデル + PEN の合算予測値（石単位）を返す。
    pub fn predict(&self, board: &Board) -> f32 {
        let pass = self.forward(board);
        self.linear.predict(&pass.features) + pass.output
    }

    /// `records` を先頭から順に 1 パス学習する。
    /// `linear` が `false` の場合は線形重みを更新せず（ラベル平均バイアスのみ保持）、
    /// PEN ネットワーク単体で局面を学習する。
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
        // ネットワークの平均出力を線形バイアスへ移し、さらに線形テーブルを中心化する。
        // ここで用いる平均はパス中に各サンプルが見た出力の平均であり最終重みでの再評価ではないため、
        // わずかな平均残差（全データで約 0.5 石）が残る場合があるが、1 パス分の計算を節約できる。
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
        // 同一パス内で複数回参照される埋め込み（対称石配置や鏡像共有）は、
        // 線形重みと同様に勾配を合算して 1 回の AdaGrad ステップとして更新する。
        // 同一パターンの各向きは同じ勾配を持ち、各状態は 1 つのテーブルのみに属する。
        for_each_read(&pass.features.patterns, |state, p, times| {
            let times = times as f32;
            // 全次元をまとめて更新する（勾配 0 の次元は値が変わらないためスキップと同等）。
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
        // AdamW に準拠し、ステップ前に Decoupled Weight Decay（w · keep − Δ）を適用する。
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

    /// `records` 上で重みを更新せずに Huber 損失と MAE を計測する。
    pub fn validate(&self, records: &[Record], huber_delta: f32) -> Metrics {
        let mut m = Accumulator::default();
        for r in records {
            m.add(self.predict(&r.board) - f32::from(r.value), huber_delta);
        }
        m.finish()
    }

    /// 各パターンテーブルの参照回数加重平均埋め込みを計算する（未使用テーブルは初期値 `EMBEDDING_INIT`）。
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

    /// エンジン用に量子化した線形フェーズ [`PatternPhase`] とネットワーク [`PenPhase`] を生成する。
    ///
    /// 出現回数の少ない重みや埋め込みは `shrink` を使って縮小し、過学習を防ぐ：
    /// 線形重みは [`PhaseWeights::export`] と同様にテーブル平均（0）へ寄せ、
    /// 埋め込みベクトルは所属テーブルの平均埋め込みへ寄せる。鏡像共有が有効な場合は同じ値を複製する。
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
        // エンジン側のバイアス上限（`PenPhase::new`）に合わせる。
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
