use crate::board::board::Board;
use crate::file::{EngineFile, EvaluatorData, PatternEvaluatorData};
use crate::EngineError;
use serde::Deserialize;
use std::fs;
use std::io;

use super::feature_indexes::FeatureIndexes;
use super::nnue_evaluator::{NnueEvaluator, NnueState};
use super::pattern_evaluator::PatternEvaluator;

pub enum Evaluator {
    Pattern(PatternEvaluator),
    Nnue(NnueEvaluator),
}

// Incremental evaluation state for the planned fast evaluator path.
#[allow(dead_code)]
pub enum EvalState {
    Pattern(FeatureIndexes),
    Nnue(NnueState),
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::Pattern(PatternEvaluator::default())
    }
}

impl Evaluator {
    pub fn from_path(path: &str) -> Result<Self, EngineError> {
        let bytes = fs::read(path)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        let engine_file = match std::str::from_utf8(&bytes) {
            Ok(s) => return Self::from_str_data(s),
            Err(_) => EngineFile::from_bytes(bytes)
                .map_err(|e| EngineError::InvalidData(e.to_string()))?,
        };
        let (_, evaluator, _) = engine_file
            .into_parts()
            .map_err(|e| EngineError::InvalidData(e.to_string()))?;
        Ok(evaluator)
    }

    pub fn from_str_data(input: &str) -> Result<Self, EngineError> {
        match EngineFile::read_string(input) {
            Ok(file) => {
                let (_, evaluator, _) = file
                    .into_parts()
                    .map_err(|e| EngineError::InvalidData(e.to_string()))?;
                Ok(evaluator)
            }
            Err(v3_error) => {
                #[derive(Deserialize)]
                struct LegacyPatternFile {
                    evaluator: PatternEvaluatorData,
                }
                let legacy: LegacyPatternFile = serde_json::from_str(input)
                    .map_err(|_| EngineError::InvalidData(v3_error.to_string()))?;
                Self::from_data(EvaluatorData::Pattern(legacy.evaluator))
                    .map_err(|e| EngineError::InvalidData(e.to_string()))
            }
        }
    }

    // Builds incremental state for the planned fast evaluator path.
    #[allow(dead_code)]
    pub(crate) fn state_from_board(&self, board: &Board) -> EvalState {
        match self {
            Self::Pattern(_) => EvalState::Pattern(FeatureIndexes::from_board(board)),
            Self::Nnue(evaluator) => EvalState::Nnue(evaluator.state_from_board(board)),
        }
    }

    #[inline(always)]
    // Uses incremental state for the planned fast evaluator path.
    #[allow(dead_code)]
    pub(crate) fn evaluate(&self, board: &Board, state: &EvalState) -> i32 {
        match (self, state) {
            (Self::Pattern(evaluator), EvalState::Pattern(state)) => {
                evaluator.evaluate(board, state)
            }
            (Self::Nnue(evaluator), EvalState::Nnue(state)) => evaluator.evaluate(board, state),
            _ => panic!("evaluator/state kind mismatch"),
        }
    }

    #[inline(always)]
    pub fn evaluate_board_slow(&self, board: &Board) -> i32 {
        match self {
            Self::Pattern(evaluator) => evaluator.evaluate_board_slow(board),
            Self::Nnue(evaluator) => evaluator.evaluate_board_slow(board),
        }
    }

    pub(crate) fn from_data(data: EvaluatorData) -> io::Result<Self> {
        Ok(match data {
            EvaluatorData::Pattern(data) => Self::Pattern(PatternEvaluator::from_data(data)?),
            EvaluatorData::Nnue(data) => Self::Nnue(NnueEvaluator::from_data(data)?),
        })
    }

    // Serialization hook retained for engine-file export tooling.
    #[allow(dead_code)]
    pub(crate) fn to_data(&self) -> EvaluatorData {
        match self {
            Self::Pattern(evaluator) => EvaluatorData::Pattern(evaluator.to_data()),
            Self::Nnue(evaluator) => EvaluatorData::Nnue(evaluator.to_data()),
        }
    }

    pub(crate) fn validate_data(data: &EvaluatorData) -> io::Result<()> {
        match data {
            EvaluatorData::Pattern(data) => PatternEvaluator::validate_data(data),
            EvaluatorData::Nnue(data) => NnueEvaluator::validate_data(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D3: u64 = 1u64 << 19;

    fn played_board() -> Board {
        let board = Board::new();
        let flip = board.flip_bit(D3);
        board.make_move_from_flip_bit(D3, flip)
    }

    #[test]
    fn default_evaluator_is_pattern() {
        assert!(matches!(Evaluator::default(), Evaluator::Pattern(_)));
    }

    #[test]
    fn enum_state_eval_matches_slow_eval() {
        let evaluator = Evaluator::default();
        for board in [Board::new(), played_board()] {
            let state = evaluator.state_from_board(&board);
            assert_eq!(
                evaluator.evaluate_board_slow(&board),
                evaluator.evaluate(&board, &state)
            );
        }
    }

    #[test]
    fn nnue_state_eval_matches_slow_eval() {
        let evaluator = Evaluator::Nnue(NnueEvaluator::default());
        for board in [Board::new(), played_board()] {
            let state = evaluator.state_from_board(&board);
            assert_eq!(
                evaluator.evaluate_board_slow(&board),
                evaluator.evaluate(&board, &state)
            );
        }
    }
}
