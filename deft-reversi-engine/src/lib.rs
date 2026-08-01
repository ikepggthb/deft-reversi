mod board;
mod error;
mod eval;
mod file;
mod game;
mod search;
mod t_table;

pub use board::board::Board;
pub use board::position::{position_num_to_str, position_str_to_num};
pub use error::EngineError;
pub use eval::evaluator::Evaluator;
pub use game::{check_record, Color, Game, Position};
pub use search::{
    MpcConfig, Solver, SolverOptions, SolverResult, SolverType, NO_MPC_SELECTIVITY_LV,
    SOLVE_LEVEL_MAX,
};

#[cfg(feature = "train-tools")]
pub mod train_api {
    pub use crate::eval::evaluator_const::*;
    pub use crate::eval::feature_indexes::FeatureIndexes;
    pub use crate::eval::nnue_evaluator::NnueState;
    pub use crate::eval::nnue_features::{
        NNUE_ACCUMULATOR_SIZE, NNUE_ACTIVATION_MAX, NNUE_ACTIVATION_SCALE,
        NNUE_DEFAULT_WEIGHT_SCALE, NNUE_DENSE_LAYER_SIZES, NNUE_INPUT_SIZE, NNUE_PATTERN_FEATURES,
        NNUE_PATTERN_INSTANCE_COUNT, NNUE_TOWER_COUNT,
    };
    pub use crate::file::{
        DenseLayerData, EngineFile, EvaluatorData, Metadata, NnueEvaluatorData, NnueTowerData,
        PatternEvaluatorData, PhaseData,
    };
    pub use crate::search::mpc::MpcConfig;
}
