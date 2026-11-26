//! 線形パターン評価に加算する小型の非線形ニューラルネットワークです。
//!
//! 通常のパターン評価は各パターンの重みを単純に足し合わせるだけですが、
//! このネットワークを使うことでパターン同士の組み合わせや局面全体の状況（合法手数・石数）を踏まえた補正ができます。
//!
//! 仕組みは次のとおりです。
//! 1. 各パターン状態には、線形重みに加えて `PEN_DIM` 次元の `i8` 埋め込みベクトルを持たせます。
//! 2. 各パターンの 4 回転方向の埋め込みベクトルを足し合わせて 0..=127 に収めた値（`PATTERN_COUNT * PEN_DIM` 個）と、
//!    `PEN_GLOBALS` 個の盤面全体入力（双方の合法手数と手番側の石数）をネットワークの入力にします。
//! 3. `PEN_HIDDEN` ユニットの隠れ層 1 層を通して、1/128 石単位の補正値を計算します。
//! 4. この補正値を線形パターン評価の合計に加算してから、最終的な石数へ丸めます。
//!
//! 探索中に浮動小数点演算を避け、高速な整数演算（SIMD の積和命令を含む）だけで推論を実行するため、
//! 学習時の浮動小数点パラメータ（`E`, `W1`, `b1`, `w2`, `b2`、各活性化値の範囲は 0..=1）を
//! 以下のように固定小数点の整数へ量子化して計算します。
//! - `a = clamp(round(127 * E) を 4 回転分合算, 0, 127)` ≈ 127 a
//! - 着手可能数入力: 各手番の合法手数 `m` に対し `min(4 * m, 127)`、
//!   石数入力: 手番側石数 `d` に対し `min(2 * d, 127)`（[`global_inputs`]）
//! - `h = clamp((Σ a * W1q + b1q + 32) >> 6, 0, 127)`（`W1q = round(64 * W1)`, `b1q = round(127 * 64 * b1)`）≈ 127 h
//! - 出力 `(Σ h * w2q) >> 7`（`w2q = round(128 * 128 / 127 * w2)`）＋ `b2q = round(128 * b2)`
//!
//! トレーナ側にもこれに対応する浮動小数点モデルがあります（`deft-reversi-train/src/pen.rs` 参照）。
use super::{EvalState, PATTERN_COUNT, PATTERN_TABLE_SIZES, TOTAL_PATTERN_WEIGHTS};
use crate::Board;
use std::io::{self, Read, Write};

/// 1 パターン状態あたりの埋め込みベクトルの次元数です。
pub const PEN_DIM: usize = 8;
/// 隠れ層のユニット数です。
pub const PEN_HIDDEN: usize = 32;
/// 埋め込み和の後ろに連結する盤面全体入力の数（手番側合法手数、相手側合法手数、手番側石数）です。
pub const PEN_GLOBALS: usize = 3;
/// ネットワークの総入力次元数（パターン埋め込み和 ＋ 盤面全体入力）です。
pub const PEN_INPUTS: usize = PATTERN_COUNT * PEN_DIM + PEN_GLOBALS;
/// `PEN1` 形式（DPR2 バージョン 3）の入力次元数です。埋め込み和と双方の合法手数のみを含みます。
/// 読み込み時は残りの石数重みに 0 を入れた `PEN2` 相当として扱います。
pub(crate) const PEN1_INPUTS: usize = PATTERN_COUNT * PEN_DIM + 2;
/// 隠れ層バイアス・出力バイアスの絶対値上限です。
const PEN_BIAS_LIMIT: u32 = 1 << 24;
/// SIMD 命令（4 要素単位の積和）に合わせるため、入力次元数を 4 の倍数へ切り上げた値です。
const PEN_INPUTS_PADDED: usize = PEN_INPUTS.next_multiple_of(4);
/// 4 入力単位のグループ数です。
const GROUPS: usize = PEN_INPUTS_PADDED / 4;

/// 1 フェーズ分の補助ニューラルネットワークパラメータです。
#[derive(Clone)]
pub struct PenPhase {
    /// 各パターン状態に対応する埋め込みベクトルです（パターン重みと同じ順序で格納します）。
    embeddings: Box<[[i8; PEN_DIM]]>,
    /// 4 入力グループ単位に並べ替えて詰めた隠れ層重みです。
    /// `hidden[g][4 * j + t]` はユニット `j` における入力 `4 * g + t` の重みを表し、
    /// これにより 1 グループ分の全ユニット積和を 1 回のベクトル積で計算できます。
    hidden: Box<[[i8; 4 * PEN_HIDDEN]; GROUPS]>,
    hidden_bias: [i32; PEN_HIDDEN],
    output: [i32; PEN_HIDDEN],
    output_bias: i32,
    /// 石数入力（`PEN1_INPUTS` 以降）に非ゼロ重みを持つかどうかです。false なら `PEN1` 互換です。
    board_inputs: bool,
}

/// 合法手数を 0..=127 のネットワーク入力値へスケーリングします。
#[inline]
fn mobility_input(moves: u32) -> u8 {
    (moves * 4).min(127) as u8
}

/// 石数を 0..=127 のネットワーク入力値へスケーリングします。
#[inline]
fn count_input(discs: u32) -> u8 {
    (discs * 2).min(127) as u8
}

/// `board` に対する盤面全体入力（0..=127）を `PEN_GLOBALS` 順で返します。トレーナもここから参照します。
#[cfg_attr(not(feature = "tuning"), allow(dead_code))]
pub fn global_inputs(board: &Board) -> [u8; PEN_GLOBALS] {
    [
        mobility_input(board.get_moves().count_ones()),
        mobility_input(board.get_opponent_moves().count_ones()),
        count_input(board.player_bits().count_ones()),
    ]
}

impl PenPhase {
    /// `hidden[j]` にユニット `j` の `PEN_INPUTS` 個の重みを渡して [`PenPhase`] を構築します。
    pub fn new(
        embeddings: Vec<[i8; PEN_DIM]>,
        hidden: &[[i8; PEN_INPUTS]; PEN_HIDDEN],
        hidden_bias: [i32; PEN_HIDDEN],
        output: [i32; PEN_HIDDEN],
        output_bias: i32,
    ) -> Result<Self, &'static str> {
        if embeddings.len() != TOTAL_PATTERN_WEIGHTS {
            return Err("invalid embedding table size");
        }
        // すべての加算結果が 32 ビット符号付き整数（`i32`）に収まるよう範囲を制限します。
        // 隠れユニットの入力和は `127 * 128 * PEN_INPUTS < 2^21` 未満、出力項は `127 * 2^16` 未満となり、
        // 線形評価項と合算しても `2^26` 未満に収まります。
        if output.iter().any(|w| w.unsigned_abs() > 1 << 16) {
            return Err("PEN output weight out of range");
        }
        if hidden_bias
            .iter()
            .chain([&output_bias])
            .any(|b| b.unsigned_abs() > PEN_BIAS_LIMIT)
        {
            return Err("PEN bias out of range");
        }
        let board_inputs = hidden
            .iter()
            .any(|row| row[PEN1_INPUTS..].iter().any(|&w| w != 0));
        let mut padded = Box::new([[0i8; 4 * PEN_HIDDEN]; GROUPS]);
        for (j, row) in hidden.iter().enumerate() {
            for (k, &w) in row.iter().enumerate() {
                padded[k / 4][4 * j + k % 4] = w;
            }
        }
        Ok(Self {
            embeddings: embeddings.into_boxed_slice(),
            hidden: padded,
            hidden_bias,
            output,
            output_bias,
            board_inputs,
        })
    }

    /// `PEN1` 形式に収まらない追加の盤面入力（石数）を使用しているかを返します。
    pub(crate) fn needs_pen2(&self) -> bool {
        self.board_inputs
    }

    /// ネットワークの出力値（1/128 石単位）を計算します。
    #[inline]
    pub(crate) fn evaluate(
        &self,
        board: &Board,
        state: &EvalState,
        moves: u32,
        opponent_moves: u32,
    ) -> i32 {
        let mut input = [0u8; PEN_INPUTS_PADDED];
        let mut offset = 0;
        for (p, size) in PATTERN_TABLE_SIZES.iter().enumerate() {
            let mut sum = [0i16; PEN_DIM];
            for r in 0..4 {
                let e = &self.embeddings[offset + state.current[p * 4 + r] as usize];
                for (s, &v) in sum.iter_mut().zip(e) {
                    *s += i16::from(v);
                }
            }
            for (dst, s) in input[p * PEN_DIM..][..PEN_DIM].iter_mut().zip(sum) {
                *dst = s.clamp(0, 127) as u8;
            }
            offset += size;
        }
        let globals = PATTERN_COUNT * PEN_DIM;
        input[globals] = mobility_input(moves);
        input[globals + 1] = mobility_input(opponent_moves);
        input[globals + 2] = count_input(board.player_bits().count_ones());
        let sums = hidden_sums(&input, &self.hidden);
        let mut out = 0i32;
        for ((sum, bias), w) in sums.iter().zip(&self.hidden_bias).zip(&self.output) {
            let h = ((sum + bias + 32) >> 6).clamp(0, 127);
            out += h * w;
        }
        (out >> 7) + self.output_bias
    }
    fn hidden_row(&self, j: usize) -> [i8; PEN_INPUTS] {
        std::array::from_fn(|k| self.hidden[k / 4][4 * j + k % 4])
    }

    /// 隠れユニットごとに `inputs` 個の重み（`PEN2` なら `PEN_INPUTS`、`PEN1` なら `PEN1_INPUTS`）を書き出します。
    pub(crate) fn write(&self, out: &mut impl Write, inputs: usize) -> io::Result<()> {
        assert!(inputs == PEN_INPUTS || (inputs == PEN1_INPUTS && !self.board_inputs));
        let mut bytes = Vec::with_capacity(TOTAL_PATTERN_WEIGHTS * PEN_DIM + 8192);
        for e in self.embeddings.iter() {
            bytes.extend(e.iter().map(|&v| v as u8));
        }
        for j in 0..PEN_HIDDEN {
            bytes.extend(self.hidden_row(j)[..inputs].iter().map(|&v| v as u8));
        }
        for v in self
            .hidden_bias
            .iter()
            .chain(&self.output)
            .chain(std::iter::once(&self.output_bias))
        {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        out.write_all(&bytes)
    }

    /// 隠れユニットあたり `inputs` 個の重み形式でネットワークを読み込みます（[`write`](Self::write) と対になる処理です）。
    pub(crate) fn read(input: &mut impl Read, inputs: usize) -> io::Result<Self> {
        let mut bytes = vec![0u8; TOTAL_PATTERN_WEIGHTS * PEN_DIM];
        input.read_exact(&mut bytes)?;
        let embeddings = bytes
            .as_chunks::<PEN_DIM>()
            .0
            .iter()
            .map(|c| c.map(|b| b as i8))
            .collect();
        let mut hidden = [[0i8; PEN_INPUTS]; PEN_HIDDEN];
        for row in &mut hidden {
            let mut b = [0u8; PEN_INPUTS];
            input.read_exact(&mut b[..inputs])?;
            *row = b.map(|v| v as i8);
        }
        let mut i32s = [0i32; 2 * PEN_HIDDEN + 1];
        for v in &mut i32s {
            let mut b = [0; 4];
            input.read_exact(&mut b)?;
            *v = i32::from_le_bytes(b);
        }
        Self::new(
            embeddings,
            &hidden,
            i32s[..PEN_HIDDEN].try_into().unwrap(),
            i32s[PEN_HIDDEN..2 * PEN_HIDDEN].try_into().unwrap(),
            i32s[2 * PEN_HIDDEN],
        )
        .map_err(io::Error::other)
    }
}

/// 各隠れユニットの重み付き入力総和を計算します（AVX2 版：`_mm256_maddubs_epi16` で 4 入力ずつ並列積和を行います）。
#[cfg(target_feature = "avx2")]
#[inline]
fn hidden_sums(
    input: &[u8; PEN_INPUTS_PADDED],
    weights: &[[i8; 4 * PEN_HIDDEN]; GROUPS],
) -> [i32; PEN_HIDDEN] {
    use std::arch::x86_64::*;
    // SAFETY: AVX2 はコンパイル時に有効化されており、配列境界内の非アラインロードのみを行います。
    unsafe {
        let ones = _mm256_set1_epi16(1);
        let mut acc = [_mm256_setzero_si256(); PEN_HIDDEN / 8];
        for (g, w) in weights.iter().enumerate() {
            let x = _mm256_set1_epi32(i32::from_le_bytes(input[4 * g..][..4].try_into().unwrap()));
            for (q, a) in acc.iter_mut().enumerate() {
                let wv = _mm256_loadu_si256(w.as_ptr().add(32 * q).cast());
                // 入力は最大 127 なので、2 要素の積和は符号付き `i16` の範囲内にオーバーフローせず収まります。
                let pairs = _mm256_maddubs_epi16(x, wv);
                *a = _mm256_add_epi32(*a, _mm256_madd_epi16(pairs, ones));
            }
        }
        let mut out = [0i32; PEN_HIDDEN];
        for (q, a) in acc.iter().enumerate() {
            _mm256_storeu_si256(out.as_mut_ptr().add(8 * q).cast(), *a);
        }
        out
    }
}

/// 各隠れユニットの重み付き入力総和を計算します（汎用スカラー版）。
#[cfg(not(target_feature = "avx2"))]
#[inline]
fn hidden_sums(
    input: &[u8; PEN_INPUTS_PADDED],
    weights: &[[i8; 4 * PEN_HIDDEN]; GROUPS],
) -> [i32; PEN_HIDDEN] {
    let mut out = [0i32; PEN_HIDDEN];
    for (g, w) in weights.iter().enumerate() {
        for (j, o) in out.iter_mut().enumerate() {
            for t in 0..4 {
                *o += i32::from(input[4 * g + t]) * i32::from(w[4 * j + t]);
            }
        }
    }
    out
}
