//! 1 フェーズ分の線形パターン重みを学習するモジュール。
//!
//! 評価関数では盤上の石数（フェーズ）ごとに独立した重みテーブルを用意し、
//! 自身の石数に一致する局面だけを学習する。そのため、スレッドごとに 1 フェーズずつ
//! メモリに展開してエポックごとにシャッフルしながら独立に学習できる。
//!
//! 学習した重みをモデルとして書き出す（export）際に精度が落ちないよう、次の 3 つの工夫を行っている：
//! - **鏡像での重み共有（`mirror`）**: 盤面のパターン状態とその左右反転・対称形（鏡像）で重みを共有し、
//!   対称な局面を必ず同じスコアで評価する。
//! - **テーブル中心化（[`PhaseWeights::recenter`]）**: どの局面も各パターンテーブルから 4 個、
//!   着可能手数テーブルから 1 個の重みを必ず参照するため、テーブル全体に定数を足しても
//!   定数バイアスが増減するだけで予測結果は変わらない。AdaGrad で学習を続けるとこうした定数オフセットが
//!   テーブル内に溜まりやすいため、各パスの終わりに各テーブルの出現回数加重平均をバイアスへ移して
//!   平均を 0 に揃える（予測値は一切変わらない）。これにより、後述の `shrink` で出現回数の少ない重みを
//!   0 へ寄せたとき、偏ったオフセットではなく「テーブルの平均値」へ自然に引き寄せられる。
//! - **着可能手数重みの近傍補間（[`PhaseWeights::export`]）**: 出現回数の少ない着可能手数の重みは、
//!   0 ではなく近くの十分に学習された重みの値へ引き寄せることで、テーブルの両端に不自然な段差ができるのを防ぐ。
use crate::rd::Record;
use deft_reversi_engine::{
    Board, Features, MOBILITY_WEIGHT_COUNT, PATTERN_WEIGHT_COUNT, PatternPhase, mirror_weights,
    pattern_table_sizes,
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// 各パターン重みインデックスについて、鏡像と共有する代表インデックス（双方の小さい方）を返す。
pub fn shared_weights() -> &'static [u32] {
    static SHARED: OnceLock<Vec<u32>> = OnceLock::new();
    SHARED.get_or_init(|| {
        let mirror = mirror_weights();
        (0..mirror.len() as u32)
            .map(|i| i.min(mirror[i as usize]))
            .collect()
    })
}

/// 最適化のハイパーパラメータ設定。チェックポイントに保存され、再開時には同じ値であることが要求される。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// AdaGrad（勾配の二乗和に応じて歩幅を自動調整する最適化手法）の学習率（石単位）。
    pub lr: f32,
    /// 重みの肥大化を防ぐ L2 正則化係数（各勾配に `l2 * weight` を加算する）。
    pub l2: f32,
    /// Huber 損失（外れ値に強い誤差関数）において、誤差を二乗ではなく線形で扱う境界値（石数）。
    pub huber_delta: f32,
    /// パターン重みを鏡像と共有するかどうか。
    pub mirror: bool,
}

impl Settings {
    /// 設定値が有効範囲（`lr > 0`, `huber_delta > 0`, `l2 >= 0` かつ有限）かを検証する。
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

/// 重みとその AdaGrad 状態を 1 構造体にまとめ、更新時に 1 キャッシュラインで済むようにする。
#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize)]
struct Slot {
    weight: f32,
    grad_squares: f32,
    /// この重みが学習サンプルで参照された回数。
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

    /// 重みを 128 倍して `i16` に量子化する。
    /// データセットにあまり現れなかった重み（過学習しやすい）は、`shrink` を使って `seen / (seen + shrink)` 倍に縮め、0 方向へ寄せる。
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

/// 1 フェーズ分の線形パターン・着可能手数・バイアス重みと学習状態。
#[derive(Clone, Serialize, Deserialize)]
pub struct PhaseWeights {
    patterns: Vec<Slot>,
    mobility: Vec<Slot>,
    bias: Slot,
    /// [`Settings::mirror`] の値。
    mirror: bool,
}

/// 平均 Huber 損失および平均絶対誤差（MAE、単位：石）。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub loss: f64,
    pub mae: f64,
    pub samples: u64,
}

impl PhaseWeights {
    /// バイアスをラベル平均値で初期化し、パターン・着可能手数重みを 0 で初期化する。
    /// これにより学習初期ステップが定数オフセットではなくパターンの差を捉えることに集中できる。
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

    /// 他項（補助ニューラルネットの出力など）から移動してきた定数オフセット `x` 石分をバイアスに加算する。
    pub fn add_to_bias(&mut self, x: f32) {
        self.bias.weight += x;
    }

    /// 鏡像で重みを共有しているかどうかを返す。
    pub fn mirror(&self) -> bool {
        self.mirror
    }

    /// `board` の特徴量を抽出する。`mirror` 有効時は鏡像共有インデックスへ写像する。
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

    /// 各パターンテーブルおよび着可能手数テーブルの出現回数加重平均をバイアスへ移す。
    /// どの局面も各パターンテーブルから 4 重み、着可能手数テーブルから 1 重みを読むため、
    /// パターン平均の 4 倍と着可能手数平均の 1 倍をバイアスへ移せば予測値は一切変化しない。
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

    /// 特徴量 `f` に対する現在の線形予測値（石単位）を返す。
    pub fn predict(&self, f: &Features) -> f32 {
        let mut sum = self.bias.weight + self.mobility[f.mobility].weight;
        for &i in &f.patterns {
            sum += self.patterns[i as usize].weight;
        }
        sum
    }

    /// `records` を先頭から順に 1 パス走査し、1 サンプルごとに重みを更新したあと中心化する。
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

    /// 1 サンプル分の更新処理（`grad` は Huber クリップ済みの誤差）。
    pub fn update(&mut self, f: &Features, grad: f32, s: &Settings) {
        // 対称な石配置により同じ重みインデックスが複数回参照される場合、
        // 勾配を合算して 1 回の AdaGrad ステップとして適用する。
        crate::pen::for_each_read(&f.patterns, |i, _, times| {
            self.patterns[i as usize].update(grad * times as f32, s);
        });
        self.mobility[f.mobility].update(grad, s);
        self.bias.update(grad, s);
    }

    /// `records` 上で重みを更新せずに Huber 損失と MAE を計測する。
    pub fn validate(&self, records: &[Record], huber_delta: f32) -> Metrics {
        let mut m = Accumulator::default();
        for r in records {
            let error = self.predict(&self.features(&r.board)) - f32::from(r.value);
            m.add(error, huber_delta);
        }
        m.finish()
    }

    /// 128 倍に量子化したエンジン用の [`PatternPhase`] を生成する。
    ///
    /// 出現回数 `n` の少ない重みは過学習しやすいため、`shrink` を使って `n / (n + shrink)` 倍に縮める：
    /// パターン重みは `recenter` 済みのテーブル平均（0）へ寄せ、着可能手数重みは [`mobility_targets`] で決まる
    /// 近傍の信頼できる重みの補間値へ引き寄せる。鏡像共有が有効な場合は共有元の値を複製する。
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
        // バイアスは参照回数に関わらず縮小しない：線形学習を行わない場合（`--pen-no-linear`）でも
        // ラベル平均と補助ニューラルネット出力から移されたオフセットを保持しているため。
        let bias = (self.bias.weight * 128.0)
            .round()
            .clamp(i16::MIN.into(), i16::MAX.into()) as i16;
        PatternPhase::from_flat(patterns, mobility, bias).unwrap()
    }
}

/// 参照回数が少ない着可能手数重みの縮小先ターゲットを計算する。
///
/// `reliable` 回以上参照された重みをアンカーとし、最外アンカーより外側は最外アンカーの値、
/// アンカー間は線形補間値とする。アンカーが 1 つもない場合は全て 0.0 とする。
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

/// 1 パス分の Huber 損失と絶対誤差を集計するアキュムレータ。
#[derive(Default)]
pub struct Accumulator {
    loss: f64,
    abs: f64,
    n: u64,
}

impl Accumulator {
    /// 誤差 `error` と Huber 閾値 `delta` から 1 サンプル分の損失を加算する。
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

    /// 集計結果を [`Metrics`] として返す。
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
