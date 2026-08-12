use crate::board::board::Board;
use crate::file::{EngineFile, EvaluatorData, PatternEvaluatorData};
use crate::EngineError;
use serde::Deserialize;
use std::fs;
use std::io;
use std::sync::Arc;

use super::nnue_evaluator::NnueEvaluator;
use super::pattern_evaluator::PatternEvaluator;

/// 盤面を手番側から評価する評価関数の共通インターフェース。
///
/// 実行時に使用する具体的な評価器は、エンジンファイルの読み込み時に決まり、
/// 探索中は `dyn Evaluator` を通して呼び出される。
pub trait Evaluator: Send + Sync {
    fn evaluate(&self, board: &Board) -> i32;

    /// `board`への着手後の局面を評価する。
    ///
    /// 評価器が増分更新を持たない場合は、着手後の盤面を作って通常評価する。
    fn evaluate_move(&self, board: &Board, move_bit: u64, flip_bit: u64) -> i32 {
        self.evaluate(&board.make_move_from_flip_bit(move_bit, flip_bit))
    }
}

/// 既定のパターン評価器を生成する。
pub fn default_evaluator() -> Arc<dyn Evaluator> {
    Arc::new(PatternEvaluator::default())
}

/// ファイルから評価器を読み込む。
pub fn evaluator_from_path(path: &str) -> Result<Arc<dyn Evaluator>, EngineError> {
    let bytes = fs::read(path)?;
    evaluator_from_bytes(&bytes)
}

/// バイト列から評価器を読み込む。
pub fn evaluator_from_bytes(bytes: &[u8]) -> Result<Arc<dyn Evaluator>, EngineError> {
    let engine_file = match std::str::from_utf8(bytes) {
        Ok(s) => return evaluator_from_str_data(s),
        Err(_) => {
            EngineFile::from_bytes(bytes).map_err(|e| EngineError::InvalidData(e.to_string()))?
        }
    };
    let (_, evaluator, _) = engine_file
        .into_parts()
        .map_err(|e| EngineError::InvalidData(e.to_string()))?;
    Ok(evaluator)
}

/// JSON文字列から評価器を読み込む。旧パターン評価器形式にも対応する。
pub fn evaluator_from_str_data(input: &str) -> Result<Arc<dyn Evaluator>, EngineError> {
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
            evaluator_from_data(EvaluatorData::Pattern(legacy.evaluator))
                .map_err(|e| EngineError::InvalidData(e.to_string()))
        }
    }
}

pub(crate) fn evaluator_from_data(data: EvaluatorData) -> io::Result<Arc<dyn Evaluator>> {
    match data {
        EvaluatorData::Pattern(data) => Ok(Arc::new(PatternEvaluator::from_data(data)?)),
        EvaluatorData::Nnue(data) => Ok(Arc::new(NnueEvaluator::from_data(data)?)),
    }
}

pub(crate) fn validate_evaluator_data(data: &EvaluatorData) -> io::Result<()> {
    match data {
        EvaluatorData::Pattern(data) => PatternEvaluator::validate_data(data),
        EvaluatorData::Nnue(data) => NnueEvaluator::validate_data(data),
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
    fn default_evaluator_is_callable_through_trait_object() {
        let evaluator = default_evaluator();
        for board in [Board::new(), played_board()] {
            assert_eq!(
                PatternEvaluator::default().evaluate_board_slow(&board),
                evaluator.evaluate(&board)
            );
        }
    }

    #[test]
    fn nnue_evaluator_is_callable_through_trait_object() {
        let concrete = NnueEvaluator::default();
        let evaluator: Arc<dyn Evaluator> = Arc::new(NnueEvaluator::default());
        for board in [Board::new(), played_board()] {
            assert_eq!(
                concrete.evaluate_board_slow(&board),
                evaluator.evaluate(&board)
            );
        }
    }
}
