#[cfg(feature = "endgame-pattern-ordering")]
mod end_ordering;
mod pattern;
use crate::Board;
use pattern::definition::{
    FEATURE_COORD, PATTERN_TABLE_SIZES, SQUARE_TO_FEATURES, TOTAL_PATTERN_WEIGHTS,
};
use std::{
    io::{self, Read, Write},
    sync::Arc,
};

pub const PATTERN_COUNT: usize = 11;
pub const PHASE_COUNT: usize = 60;

/// Runtime model: tables in pattern order, weights shared across rotations.
#[derive(Clone)]
pub struct PatternModel {
    phases: Arc<[PatternPhase]>,
}
#[derive(Clone)]
pub struct PatternPhase {
    weights: Box<[i16]>,
    mobility: [i16; 128],
    bias: i16,
}
impl PatternPhase {
    pub fn new(
        patterns: Vec<Vec<i16>>,
        mobility: [i16; 128],
        bias: i16,
    ) -> Result<Self, &'static str> {
        if patterns.len() != PATTERN_COUNT
            || patterns
                .iter()
                .zip(PATTERN_TABLE_SIZES)
                .any(|(p, n)| p.len() != n)
        {
            return Err("invalid pattern table dimensions");
        }
        Ok(Self {
            weights: patterns.into_iter().flatten().collect(),
            mobility,
            bias,
        })
    }
}
impl PatternModel {
    pub fn new(phases: Vec<PatternPhase>) -> Result<Self, &'static str> {
        if phases.len() != PHASE_COUNT {
            return Err("model requires 60 phases");
        }
        Ok(Self {
            phases: phases.into(),
        })
    }
    /// Explicitly zero weights, useful for rule/search tests. Not a trained AI.
    pub fn zero() -> Self {
        Self::new(
            (0..PHASE_COUNT)
                .map(|_| PatternPhase {
                    weights: vec![0; TOTAL_PATTERN_WEIGHTS].into_boxed_slice(),
                    mobility: [0; 128],
                    bias: 0,
                })
                .collect(),
        )
        .unwrap()
    }
    pub fn evaluate(&self, board: &Board) -> i32 {
        self.evaluate_state(board, &EvalState::from_board(*board))
    }
    #[cfg(test)]
    pub(crate) fn nonzero_for_tests() -> Self {
        let mut model = Self::zero();
        for (phase, values) in Arc::get_mut(&mut model.phases)
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            for (i, weight) in values.weights.iter_mut().enumerate() {
                *weight = ((i * 37 + phase * 19) % 257) as i16 - 128;
            }
            for (i, weight) in values.mobility.iter_mut().enumerate() {
                *weight = (i as i16 - 64) * 3;
            }
            values.bias = phase as i16 - 30;
        }
        model
    }
    pub(crate) fn evaluate_state(&self, board: &Board, state: &EvalState) -> i32 {
        self.evaluate_state_and_moves(board, state).0
    }
    /// Return the legal moves already computed for the model's mobility term.
    pub(crate) fn evaluate_state_and_moves(&self, board: &Board, state: &EvalState) -> (i32, u64) {
        let phase = usize::from(60u8.saturating_sub(board.empty_count())).min(59);
        let weights = &self.phases[phase];
        let mut raw = i32::from(weights.bias);
        let mut offset = 0;
        for (p, size) in PATTERN_TABLE_SIZES.iter().enumerate() {
            for r in 0..4 {
                raw += i32::from(weights.weights[offset + state.current[p * 4 + r] as usize]);
            }
            offset += size;
        }
        let moves = board.get_moves();
        let mobility =
            64 + moves.count_ones() as i32 - board.get_opponent_moves().count_ones() as i32;
        raw += i32::from(weights.mobility[mobility as usize]);
        (((raw + raw.signum() * 64) / 128).clamp(-64, 64), moves)
    }
    /// DPR2 version 1: LE i16, 60 phases, 11 fixed-schema tables, mobility, bias.
    pub fn write_to(&self, mut out: impl Write) -> io::Result<()> {
        out.write_all(b"DPR2\x01\x00\x00\x00")?;
        let mut bytes = Vec::with_capacity((TOTAL_PATTERN_WEIGHTS + 129) * 2);
        for phase in self.phases.iter() {
            bytes.clear();
            for &value in phase
                .weights
                .iter()
                .chain(phase.mobility.iter())
                .chain(std::iter::once(&phase.bias))
            {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            out.write_all(&bytes)?;
        }
        Ok(())
    }
    pub fn read_from(mut input: impl Read) -> io::Result<Self> {
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        if &magic != b"DPR2\x01\x00\x00\x00" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported pattern model/schema",
            ));
        }
        let mut phases = Vec::with_capacity(PHASE_COUNT);
        let mut bytes = vec![0; (TOTAL_PATTERN_WEIGHTS + 129) * 2];
        for _ in 0..PHASE_COUNT {
            input.read_exact(&mut bytes)?;
            let mut values = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes([b[0], b[1]]));
            let weights = values.by_ref().take(TOTAL_PATTERN_WEIGHTS).collect();
            let mobility = std::array::from_fn(|_| values.next().unwrap());
            let bias = values.next().unwrap();
            phases.push(PatternPhase {
                weights,
                mobility,
                bias,
            });
        }
        let mut extra = [0];
        if input.read(&mut extra)? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "trailing model data",
            ));
        }
        Ok(Self::new(phases).unwrap())
    }
}

/// Feature lanes rounded up so differential updates are whole vector adds.
const LANES: usize = 48;

/// `SQUARE_DELTA[sq][f]` is the base-3 weight of `sq` in feature `f`, or 0.
static SQUARE_DELTA: [[u16; LANES]; 64] = {
    let mut table = [[0u16; LANES]; 64];
    let mut sq = 0;
    while sq < 64 {
        let features = &SQUARE_TO_FEATURES[sq];
        let mut i = 0;
        while i < features.len as usize {
            let feature = features.features[i];
            table[sq][feature.feature_idx as usize] = feature.base3_weight;
            i += 1;
        }
        sq += 1;
    }
    table
};

/// Both viewpoints avoid a base-3 digit swap on every move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EvalState {
    current: [u16; LANES],
    swapped: [u16; LANES],
}
impl EvalState {
    pub(crate) fn from_board(board: Board) -> Self {
        let mut state = Self {
            current: [0; LANES],
            swapped: [0; LANES],
        };
        for (p, pattern) in FEATURE_COORD.iter().enumerate() {
            for r in 0..4 {
                for &sq in &pattern.feature_coord[r][..pattern.n_squares as usize] {
                    let bit = 1u64 << sq;
                    let digit = if board.player_bits() & bit != 0 {
                        2
                    } else if board.opponent_bits() & bit != 0 {
                        1
                    } else {
                        0
                    };
                    state.current[p * 4 + r] = state.current[p * 4 + r] * 3 + digit;
                    state.swapped[p * 4 + r] =
                        state.swapped[p * 4 + r] * 3 + if digit == 0 { 0 } else { 3 - digit };
                }
            }
        }
        state
    }
    pub(crate) fn passed(self) -> Self {
        Self {
            current: self.swapped,
            swapped: self.current,
        }
    }
    /// The mover's digit goes 0 -> 2 on the placed square and 1 -> 2 on flips
    /// (opponent's view: 0 -> 1 and 2 -> 1); then the viewpoints swap.
    #[inline]
    pub(crate) fn played(self, bit: u64, mut flips: u64) -> Self {
        let mut flipped = [0u16; LANES];
        while flips != 0 {
            let delta = &SQUARE_DELTA[flips.trailing_zeros() as usize];
            flips &= flips - 1;
            for (sum, d) in flipped.iter_mut().zip(delta) {
                *sum = sum.wrapping_add(*d);
            }
        }
        let placed = &SQUARE_DELTA[bit.trailing_zeros() as usize];
        let mut next = self;
        for i in 0..LANES {
            next.current[i] = self.swapped[i]
                .wrapping_add(placed[i])
                .wrapping_sub(flipped[i]);
            next.swapped[i] = self.current[i]
                .wrapping_add(placed[i].wrapping_mul(2))
                .wrapping_add(flipped[i]);
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn differential_matches_refresh_through_games() {
        let mut random = 73u64;
        for _ in 0..64 {
            let mut board = Board::new();
            let mut state = EvalState::from_board(board);
            loop {
                assert_eq!(state, EvalState::from_board(board));
                let moves = board.get_moves();
                if moves == 0 {
                    if board.is_terminal() {
                        break;
                    }
                    board = board.board_after_pass();
                    state = state.passed();
                    continue;
                }
                let bit = crate::test_util::random_move(moves, &mut random);
                let flips = board.flip_mask(bit);
                #[cfg(feature = "endgame-pattern-ordering")]
                assert_eq!(
                    state.ordering_child_indices(bit, flips),
                    EvalState::from_board(board.after_known_move(bit, flips)).current,
                );
                state = state.played(bit, flips);
                board = board.after_known_move(bit, flips);
            }
        }
    }
    #[test]
    fn schema_tables_fit_u16() {
        for size in PATTERN_TABLE_SIZES {
            assert!(size <= u16::MAX as usize);
        }
        assert_eq!(pattern::definition::POW3[10], 59049);
    }
}

#[cfg(test)]
mod score_tests {
    use super::*;
    #[test]
    fn phase_mobility_and_signed_rounding() {
        let phases = (0..60)
            .map(|phase| {
                let mut mobility = [0; 128];
                mobility[64] = 64;
                PatternPhase {
                    weights: vec![0; TOTAL_PATTERN_WEIGHTS].into_boxed_slice(),
                    mobility,
                    bias: phase as i16 * 128,
                }
            })
            .collect();
        let model = PatternModel::new(phases).unwrap();
        assert_eq!(model.evaluate(&Board::new()), 1);
        let board = Board::new();
        let bit = board.get_moves() & board.get_moves().wrapping_neg();
        let child = board.after_known_move(bit, board.flip_mask(bit));
        assert_eq!(
            model.evaluate(&child),
            if child.get_moves().count_ones() == child.get_opponent_moves().count_ones() {
                2
            } else {
                1
            }
        );
        let negative = PatternModel::new(
            (0..60)
                .map(|_| PatternPhase {
                    weights: vec![0; TOTAL_PATTERN_WEIGHTS].into_boxed_slice(),
                    mobility: [0; 128],
                    bias: -64,
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(negative.evaluate(&Board::new()), -1);
    }
}
