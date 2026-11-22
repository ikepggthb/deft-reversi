//! Pattern embedding network (PEN): a small nonlinear term added to the
//! linear pattern evaluation.
//!
//! Every pattern state has, besides its linear weight, an embedding of
//! `PEN_DIM` i8 values. The four orientations of a pattern are summed, so
//! the network sees `PATTERN_COUNT * PEN_DIM` values, clipped to 0..=127,
//! and `PEN_GLOBALS` whole-board inputs: both mobilities, both sides' stable
//! discs and the mover's discs. One hidden layer of `PEN_HIDDEN` clipped units
//! gives the output in 1/128 discs, which is added to the linear sum before
//! the final rounding.
//!
//! The quantised network computes, with `E` the float embeddings, `W1`,
//! `b1`, `w2`, `b2` the float layers and every activation in 0..=1 as a
//! float:
//! - `a = clamp(round(127 E) summed over orientations, 0, 127)` ≈ 127 a
//! - mobility inputs `min(4 m, 127)` for each side's move count `m`; stable
//!   disc inputs `min(2 s, 127)` for each side's stable discs `s`; the disc
//!   input `min(2 d, 127)` for the mover's discs `d` (`global_inputs`)
//! - `h = clamp((Σ a W1q + b1q + 32) >> 6, 0, 127)` with `W1q = round(64 W1)`
//!   and `b1q = round(127 · 64 · b1)`, ≈ 127 h
//! - output `(Σ h w2q) >> 7` with `w2q = round(128 · 128 / 127 · w2)`, plus
//!   `b2q = round(128 b2)`
//!
//! The trainer mirrors this in floats; see `deft-reversi-train/src/pen.rs`.
use super::{EvalState, PATTERN_COUNT, PATTERN_TABLE_SIZES, TOTAL_PATTERN_WEIGHTS};
use crate::Board;
use std::io::{self, Read, Write};

/// Embedding values per pattern state.
pub const PEN_DIM: usize = 8;
/// Hidden units.
pub const PEN_HIDDEN: usize = 32;
/// Whole-board inputs after the summed embeddings: the mover's and the
/// opponent's moves, the mover's and the opponent's stable discs, and the
/// mover's discs.
pub const PEN_GLOBALS: usize = 5;
/// Network inputs: the summed embeddings, then the whole-board inputs.
pub const PEN_INPUTS: usize = PATTERN_COUNT * PEN_DIM + PEN_GLOBALS;
/// Inputs of a `PEN1` network (DPR2 version 3): the embeddings and the two
/// mobilities only. It reads as a network with zero weights on the others.
pub(crate) const PEN1_INPUTS: usize = PATTERN_COUNT * PEN_DIM + 2;
/// The largest magnitude of a hidden or output bias.
const PEN_BIAS_LIMIT: u32 = 1 << 24;
/// Inputs padded to whole groups of four.
const PEN_INPUTS_PADDED: usize = PEN_INPUTS.next_multiple_of(4);
const GROUPS: usize = PEN_INPUTS_PADDED / 4;

/// The network of one phase.
#[derive(Clone)]
pub struct PenPhase {
    /// One embedding per pattern state, indexed like the pattern weights.
    embeddings: Box<[[i8; PEN_DIM]]>,
    /// The hidden weights by groups of four inputs: `hidden[g][4 j + t]` is
    /// unit `j`'s weight of input `4 g + t`, so one group's products with
    /// every unit are one vector multiply.
    hidden: Box<[[i8; 4 * PEN_HIDDEN]; GROUPS]>,
    hidden_bias: [i32; PEN_HIDDEN],
    output: [i32; PEN_HIDDEN],
    output_bias: i32,
    /// Whether a weight reads the stable or disc inputs. Without, the
    /// network is a `PEN1` one and evaluation skips the stability count.
    board_inputs: bool,
}

/// The mover's moves as a network input.
#[inline]
fn mobility_input(moves: u32) -> u8 {
    (moves * 4).min(127) as u8
}

/// A disc count as a network input.
#[inline]
fn count_input(discs: u32) -> u8 {
    (discs * 2).min(127) as u8
}

/// The whole-board inputs of `board`, in 0..=127, in the order of
/// `PEN_GLOBALS`. The trainer reads them from here.
#[cfg_attr(not(feature = "tuning"), allow(dead_code))]
pub fn global_inputs(board: &Board) -> [u8; PEN_GLOBALS] {
    let (stable, opponent_stable) = crate::board::stability::stable_counts(*board);
    [
        mobility_input(board.get_moves().count_ones()),
        mobility_input(board.get_opponent_moves().count_ones()),
        count_input(stable),
        count_input(opponent_stable),
        count_input(board.player_bits().count_ones()),
    ]
}

impl PenPhase {
    /// `hidden[j]` holds unit `j`'s `PEN_INPUTS` weights.
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
        // Keeps every sum within i32: a hidden unit's input sum is below
        // 127 * 128 * PEN_INPUTS < 2^21, its output below 127 * 2^16 per
        // unit, and the network's output plus the linear part below 2^26.
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

    /// Whether the network reads more than a `PEN1` network can hold.
    pub(crate) fn needs_pen2(&self) -> bool {
        self.board_inputs
    }

    /// The network's output in 1/128 discs.
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
        if self.board_inputs {
            let (stable, opponent_stable) = crate::board::stability::stable_counts(*board);
            input[globals + 2] = count_input(stable);
            input[globals + 3] = count_input(opponent_stable);
            input[globals + 4] = count_input(board.player_bits().count_ones());
        }
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

    /// Writes the network with `inputs` weights per hidden unit:
    /// `PEN_INPUTS` (`PEN2`), or `PEN1_INPUTS` (`PEN1`) when `needs_pen2` is
    /// false.
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

    /// Reads a network written with `inputs` weights per hidden unit; see
    /// `write`.
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

/// Every unit's weighted input sum.
#[cfg(target_feature = "avx2")]
#[inline]
fn hidden_sums(
    input: &[u8; PEN_INPUTS_PADDED],
    weights: &[[i8; 4 * PEN_HIDDEN]; GROUPS],
) -> [i32; PEN_HIDDEN] {
    use std::arch::x86_64::*;
    // SAFETY: AVX2 is enabled at compile time; loads are unaligned and
    // within the arrays.
    unsafe {
        let ones = _mm256_set1_epi16(1);
        let mut acc = [_mm256_setzero_si256(); PEN_HIDDEN / 8];
        for (g, w) in weights.iter().enumerate() {
            let x = _mm256_set1_epi32(i32::from_le_bytes(input[4 * g..][..4].try_into().unwrap()));
            for (q, a) in acc.iter_mut().enumerate() {
                let wv = _mm256_loadu_si256(w.as_ptr().add(32 * q).cast());
                // Inputs are at most 127, so a pair of products fits i16.
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
