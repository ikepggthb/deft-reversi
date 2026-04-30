use crate::board::board::Board;
use crate::file::{invalid_data, EvaluatorData, PhaseData};
use std::io;

use super::evaluator_const::*;
use super::feature_indexes::FeatureIndexes;

/// 読み込み済みの評価関数本体。
///
/// 重みは phase ごとに保持し、探索中は `EvalState` と組み合わせて使う。
pub struct Evaluator {
    weights: EvalWeights,
}

struct EvalWeights {
    phases: Box<[PhaseWeights; N_PHASES]>,
}

struct PhaseWeights {
    pattern_weights: Box<[i16]>,
    mobility_weights: [i16; N_MOBILITY_MAX],
    bias: i16,
}

impl Default for PhaseWeights {
    fn default() -> Self {
        Self {
            pattern_weights: vec![0; TOTAL_PATTERN_WEIGHTS].into_boxed_slice(),
            mobility_weights: [0; N_MOBILITY_MAX],
            bias: 0,
        }
    }
}

impl Default for EvalWeights {
    fn default() -> Self {
        Self {
            phases: Box::new(std::array::from_fn(|_| PhaseWeights::default())),
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self {
            weights: EvalWeights::default(),
        }
    }
}

impl Evaluator {
    /// `board` と `state` から評価値を返す。
    ///
    /// 返り値は現在手番側から見た disc score で、`[-64, 64]` に丸める。
    #[inline(always)]
    pub fn evaluate(&self, board: &Board, state: &FeatureIndexes) -> i32 {
        div_round_to_disc_score(self.evaluate_raw(board, state)).clamp(-SCORE_MAX, SCORE_MAX)
    }

    /// `board` から都度 `EvalState` を作って評価する。
    ///
    /// 単発評価や test 用で、探索本体では使わない。
    #[inline(always)]
    pub fn evaluate_board_slow(&self, board: &Board) -> i32 {
        let state = FeatureIndexes::from_board(board);
        self.evaluate(board, &state)
    }

    #[inline(always)]
    fn evaluate_raw(&self, board: &Board, state: &FeatureIndexes) -> i32 {
        unsafe { self.evaluate_raw_unchecked(board, state) }
    }

    #[inline(always)]
    pub(crate) unsafe fn evaluate_raw_unchecked(
        &self,
        board: &Board,
        state: &FeatureIndexes,
    ) -> i32 {
        // 読み込み時に table サイズは検証済みなので、
        // hot path では unchecked で加算する。
        let phase_idx = phase_from_board(board);
        let phase = self.weights.phases.get_unchecked(phase_idx);
        let pattern_weights = phase.pattern_weights.as_ptr();
        let pattern_indexes = state.feature_indexes.as_ptr();

        let mut score = 0i32;
        let mut offset = 0usize;
        for pattern_idx in 0..N_PATTERNS {
            let feature_idx = pattern_idx * N_ROTATIONS;

            score +=
                *pattern_weights.add(offset + *pattern_indexes.add(feature_idx) as usize) as i32;
            score += *pattern_weights.add(offset + *pattern_indexes.add(feature_idx + 1) as usize)
                as i32;
            score += *pattern_weights.add(offset + *pattern_indexes.add(feature_idx + 2) as usize)
                as i32;
            score += *pattern_weights.add(offset + *pattern_indexes.add(feature_idx + 3) as usize)
                as i32;

            offset += PATTERN_TABLE_SIZES[pattern_idx];
        }

        let mobility_idx = N_MOBILITY_BASE + board.moves().count_ones() as usize
            - board.opponent_moves().count_ones() as usize;
        score += *phase.mobility_weights.get_unchecked(mobility_idx) as i32;
        score += phase.bias as i32;

        score
    }

    pub(crate) fn from_data(data: EvaluatorData) -> io::Result<Self> {
        Ok(Self {
            weights: EvalWeights::from_data(data)?,
        })
    }

    pub(crate) fn to_data(&self) -> EvaluatorData {
        EvaluatorData {
            phases: self.weights.to_data(),
        }
    }

    pub(crate) fn validate_data(data: &EvaluatorData) -> io::Result<()> {
        let _ = EvalWeights::from_data(data.clone())?;
        Ok(())
    }
}

impl EvalWeights {
    fn from_data(data: EvaluatorData) -> io::Result<Self> {
        let phases = data.phases;
        let phases = fixed_vec::<_, N_PHASES>(phases, "phase count")?
            .map(|phase| PhaseWeights::from_file(&phase))
            .into_iter()
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            phases: Box::new(fixed_vec::<_, N_PHASES>(phases, "runtime phase count")?),
        })
    }

    fn to_data(&self) -> Vec<PhaseData> {
        self.phases.iter().map(PhaseWeights::to_file).collect()
    }
}

impl PhaseWeights {
    fn from_file(file: &PhaseData) -> io::Result<Self> {
        if file.pattern_weights.len() != N_PATTERNS {
            return Err(invalid_data(format!(
                "pattern_weights length must be {N_PATTERNS}, got {}",
                file.pattern_weights.len()
            )));
        }
        if file.mobility_weights.len() != N_MOBILITY_MAX {
            return Err(invalid_data(format!(
                "mobility_weights length must be {N_MOBILITY_MAX}, got {}",
                file.mobility_weights.len()
            )));
        }

        let mut pattern_weights = Vec::new();
        for pattern_idx in 0..N_PATTERNS {
            let weights = &file.pattern_weights[pattern_idx];
            let expected_len = PATTERN_TABLE_SIZES[pattern_idx];
            if weights.len() != expected_len {
                return Err(invalid_data(format!(
                    "pattern_weights[{pattern_idx}] length must be {expected_len}, got {}",
                    weights.len()
                )));
            }

            pattern_weights.extend_from_slice(weights);
        }

        Ok(Self {
            pattern_weights: pattern_weights.into_boxed_slice(),
            mobility_weights: fixed_vec(file.mobility_weights.clone(), "mobility weight count")?,
            bias: file.bias,
        })
    }

    fn to_file(&self) -> PhaseData {
        let mut pattern_weights = Vec::with_capacity(N_PATTERNS);
        let mut offset = 0usize;
        for pattern_idx in 0..N_PATTERNS {
            let len = PATTERN_TABLE_SIZES[pattern_idx];
            pattern_weights.push(self.pattern_weights[offset..offset + len].to_vec());
            offset += len;
        }

        PhaseData {
            pattern_weights,
            mobility_weights: self.mobility_weights.to_vec(),
            bias: self.bias,
        }
    }
}

#[inline(always)]
fn div_round_to_disc_score(mut score: i32) -> i32 {
    if score > 0 {
        score += SCORE_SCALE / 2;
    } else if score < 0 {
        score -= SCORE_SCALE / 2;
    }
    score / SCORE_SCALE
}

#[inline(always)]
fn phase_from_board(board: &Board) -> usize {
    (board.move_count() as usize).min(N_PHASES - 1)
}

fn fixed_vec<T, const N: usize>(values: Vec<T>, name: &str) -> io::Result<[T; N]> {
    values
        .try_into()
        .map_err(|values: Vec<T>| invalid_data(format!("{name} must be {N}, got {}", values.len())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::EngineFile;
    use crate::search::mpc::MpcConfig;
    use std::hint::black_box;
    use std::time::Instant;

    const D3: u64 = 1u64 << 19;

    fn played_board() -> Board {
        let mut board = Board::new();
        let flip = board.flip_bit(D3);
        board.make_move_from_flip_bit(D3, flip);
        board
    }

    #[test]
    fn default_evaluator_returns_zero_for_initial_board() {
        let board = Board::new();
        let evaluator = Evaluator::default();
        let state = FeatureIndexes::from_board(&board);

        assert_eq!(evaluator.evaluate(&board, &state), 0);
    }

    #[test]
    fn slow_board_eval_matches_state_eval() {
        let evaluator = Evaluator::default();
        for board in [Board::new(), played_board()] {
            let state = FeatureIndexes::from_board(&board);
            assert_eq!(
                evaluator.evaluate_board_slow(&board),
                evaluator.evaluate(&board, &state)
            );
        }
    }

    #[test]
    fn evaluator_data_round_trip_matches_default() {
        let evaluator = Evaluator::default();
        let loaded = Evaluator::from_data(evaluator.to_data()).unwrap();
        let board = played_board();

        assert_eq!(
            evaluator.evaluate_board_slow(&board),
            loaded.evaluate_board_slow(&board)
        );
    }

    #[test]
    fn invalid_weight_lengths_return_invalid_data() {
        let mut engine_file = EngineFile::from_parts(
            crate::file::Metadata::default(),
            &Evaluator::default(),
            &MpcConfig::default(),
        );
        engine_file.evaluator.phases[0].mobility_weights.pop();

        let json = serde_json::to_string(&engine_file).unwrap();
        match EngineFile::read_string(&json) {
            Ok(_) => panic!("invalid weight length should fail"),
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidData),
        }
    }

    fn evaluate_raw_checked_for_bench(
        evaluator: &Evaluator,
        board: &Board,
        state: &FeatureIndexes,
    ) -> i32 {
        let phase_idx = phase_from_board(board);
        let phase = &evaluator.weights.phases[phase_idx];

        let mut score = 0i32;
        let mut offset = 0usize;
        for pattern_idx in 0..N_PATTERNS {
            let feature_idx = pattern_idx * N_ROTATIONS;

            score +=
                phase.pattern_weights[offset + state.feature_indexes[feature_idx] as usize] as i32;
            score += phase.pattern_weights[offset + state.feature_indexes[feature_idx + 1] as usize]
                as i32;
            score += phase.pattern_weights[offset + state.feature_indexes[feature_idx + 2] as usize]
                as i32;
            score += phase.pattern_weights[offset + state.feature_indexes[feature_idx + 3] as usize]
                as i32;

            offset += PATTERN_TABLE_SIZES[pattern_idx];
        }

        let mobility_idx = N_MOBILITY_BASE + board.moves().count_ones() as usize
            - board.opponent_moves().count_ones() as usize;
        score += phase.mobility_weights[mobility_idx] as i32;
        score += phase.bias as i32;

        score
    }

    fn sample_boards(limit: usize) -> Vec<Board> {
        let mut boards = Vec::with_capacity(limit);
        let mut frontier = vec![Board::new()];

        while let Some(board) = frontier.pop() {
            boards.push(board);
            if boards.len() >= limit {
                break;
            }

            let mut moves = board.moves();
            if moves == 0 {
                if board.opponent_moves() != 0 {
                    frontier.push(board.passed());
                }
                continue;
            }

            while moves != 0 {
                let move_bit = 1u64 << moves.trailing_zeros();
                moves &= moves - 1;

                frontier.push(board.make_move(move_bit));
            }
        }

        boards
    }

    #[test]
    #[ignore]
    fn benchmark_evaluate_checked_vs_unchecked() {
        let boards = sample_boards(256);
        let engine_file = EngineFile::read_file("../data/eval/eval.bin").unwrap();
        let (_, evaluator, _) = engine_file.into_parts().unwrap();
        let states: Vec<_> = boards.iter().map(FeatureIndexes::from_board).collect();
        let iterations = 20_000;

        let start = Instant::now();
        let mut checked_checksum = 0i64;
        for _ in 0..iterations {
            for (board, state) in boards.iter().zip(&states) {
                checked_checksum ^= black_box(evaluate_raw_checked_for_bench(
                    black_box(&evaluator),
                    black_box(board),
                    black_box(state),
                )) as i64;
            }
        }
        let checked_elapsed = start.elapsed();

        let start = Instant::now();
        let mut unchecked_checksum = 0i64;
        for _ in 0..iterations {
            for (board, state) in boards.iter().zip(&states) {
                unchecked_checksum ^= black_box(unsafe {
                    evaluator.evaluate_raw_unchecked(black_box(board), black_box(state))
                }) as i64;
            }
        }
        let unchecked_elapsed = start.elapsed();

        eprintln!(
            "evaluate checked: {:?}, unchecked: {:?}, checksums: {} / {}",
            checked_elapsed, unchecked_elapsed, checked_checksum, unchecked_checksum
        );
    }
}
