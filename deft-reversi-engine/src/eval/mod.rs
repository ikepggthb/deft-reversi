pub mod evaluator;
pub mod evaluator_const;
pub mod feature_indexes;
pub mod nnue_evaluator;
pub mod nnue_features;
pub mod pattern_evaluator;
mod util;

#[cfg(test)]
pub(crate) use evaluator::default_evaluator;
pub(crate) use evaluator::Evaluator;
