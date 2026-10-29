//! Reference engine-file v3/v4 pattern models, with their MPC configuration.
use crate::{
    evaluation::{PatternModel, PatternPhase},
    mpc::MpcConfig,
};
use bincode::Options;
use serde::{Deserialize, Serialize};
use std::io;

pub(crate) fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub eval_name: String,
    pub eval_version: String,
    pub trained_at: String,
    pub engine_version: String,
    pub git_commit: String,
    pub n_data_set: u64,
    pub n_iteration: u64,
    pub score_scale: i32,
    pub n_phases: usize,
    pub n_patterns: usize,
    pub pattern_schema_version: u32,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PatternData {
    pub phases: Vec<PhaseData>,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct PhaseData {
    pub pattern_weights: Vec<Vec<i16>>,
    pub mobility_weights: Vec<i16>,
    pub bias: i16,
}
#[derive(Serialize, Deserialize)]
enum BinaryEvaluator {
    Pattern(PatternData),
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
enum HumanEvaluator {
    Pattern(PatternData),
}
#[derive(Serialize, Deserialize)]
struct BinaryFile {
    format_version: u32,
    metadata: ModelMetadata,
    evaluator: BinaryEvaluator,
    mpc: MpcConfig,
}
#[derive(Serialize, Deserialize)]
struct HumanFile {
    format_version: u32,
    metadata: ModelMetadata,
    evaluator: HumanEvaluator,
    mpc: MpcConfig,
}

/// Evaluation weights with their MPC calibration, as loaded from a model file.
#[derive(Clone)]
pub struct Model {
    pub(crate) metadata: Option<ModelMetadata>,
    pub(crate) pattern: PatternModel,
    pub(crate) mpc: MpcConfig,
}
impl Model {
    /// Reads reference v3/v4 engine files (JSON or bincode), the legacy JSON
    /// pattern wrapper, and weight-only DPR2. Files without an MPC
    /// configuration get the reference default.
    pub fn metadata(&self) -> Option<&ModelMetadata> {
        self.metadata.as_ref()
    }
    /// Weights alone, with the reference default MPC configuration.
    #[cfg(feature = "tuning")]
    pub fn from_pattern(pattern: PatternModel) -> Self {
        Self {
            metadata: None,
            pattern,
            mpc: MpcConfig::default(),
        }
    }
    #[cfg(feature = "tuning")]
    pub fn pattern(&self) -> &PatternModel {
        &self.pattern
    }
    #[cfg(feature = "tuning")]
    pub fn mpc(&self) -> &MpcConfig {
        &self.mpc
    }
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.starts_with(b"DPR2") {
            return Ok(Self {
                metadata: None,
                pattern: PatternModel::read_from(bytes)?,
                mpc: MpcConfig::default(),
            });
        }
        let (version, metadata, data, mpc) =
            if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'{') {
                let file: HumanFile = match serde_json::from_slice(bytes) {
                    Ok(file) => file,
                    Err(modern_error) => {
                        #[derive(Deserialize)]
                        struct Legacy {
                            #[serde(default)]
                            format_version: Option<u32>,
                            evaluator: PatternData,
                        }
                        let legacy: Legacy = serde_json::from_slice(bytes)
                            .map_err(|_| invalid_data(modern_error))?;
                        if legacy.format_version.is_some() {
                            return Err(invalid_data("invalid modern engine file"));
                        }
                        return Ok(Self {
                            metadata: None,
                            pattern: legacy.evaluator.into_model()?,
                            mpc: MpcConfig::default(),
                        });
                    }
                };
                let HumanEvaluator::Pattern(data) = file.evaluator;
                (file.format_version, file.metadata, data, file.mpc)
            } else {
                let file: BinaryFile = bincode::DefaultOptions::new()
                    .with_fixint_encoding()
                    .with_limit(bytes.len() as u64)
                    .reject_trailing_bytes()
                    .deserialize(bytes)
                    .map_err(invalid_data)?;
                let BinaryEvaluator::Pattern(data) = file.evaluator;
                (file.format_version, file.metadata, data, file.mpc)
            };
        if !matches!(version, 3 | 4)
            || metadata.score_scale != 128
            || metadata.n_phases != 60
            || metadata.n_patterns != 11
            || metadata.pattern_schema_version != 1
            || metadata.eval_name.is_empty()
            || metadata.eval_version.is_empty()
        {
            return Err(invalid_data("unsupported pattern engine metadata/schema"));
        }
        mpc.validate()?;
        Ok(Self {
            metadata: Some(metadata),
            pattern: data.into_model()?,
            mpc,
        })
    }
}
impl PatternData {
    fn into_model(self) -> io::Result<PatternModel> {
        let phases = self
            .phases
            .into_iter()
            .map(|phase| {
                let mobility = phase
                    .mobility_weights
                    .try_into()
                    .map_err(|_| invalid_data("requires 128 mobility weights"))?;
                PatternPhase::new(phase.pattern_weights, mobility, phase.bias).map_err(invalid_data)
            })
            .collect::<io::Result<Vec<_>>>()?;
        PatternModel::new(phases).map_err(invalid_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Board, Engine, EngineOptions};
    #[test]
    fn reference_binary_v3_v4_and_json_load_weights_metadata_and_mpc_together() {
        let metadata = ModelMetadata {
            eval_name: "test".into(),
            eval_version: "1".into(),
            trained_at: String::new(),
            engine_version: String::new(),
            git_commit: String::new(),
            n_data_set: 5,
            n_iteration: 7,
            score_scale: 128,
            n_phases: 60,
            n_patterns: 11,
            pattern_schema_version: 1,
        };
        let mut mpc = MpcConfig::default();
        mpc.eval_search.b.constant = 4.25;
        mpc.final_search.a.constant = 1.125;
        let sizes = [10u32, 10, 10, 8, 8, 9, 10, 9, 6, 7, 8].map(|n| 3usize.pow(n));
        let data = PatternData {
            phases: (0..60)
                .map(|phase| PhaseData {
                    pattern_weights: sizes.iter().map(|&n| vec![0; n]).collect(),
                    mobility_weights: vec![0; 128],
                    bias: (phase + 1) * 128,
                })
                .collect(),
        };
        let mut file = BinaryFile {
            format_version: 4,
            metadata: metadata.clone(),
            evaluator: BinaryEvaluator::Pattern(data),
            mpc: mpc.clone(),
        };
        let mut bytes = bincode::serialize(&file).unwrap();
        for version in [3u32, 4] {
            bytes[..4].copy_from_slice(&version.to_le_bytes());
            let engine = Engine::new(
                Model::from_bytes(&bytes).unwrap(),
                EngineOptions::default().tt_mib(1),
            )
            .unwrap();
            assert_eq!(engine.metadata(), Some(&metadata));
            assert_eq!(engine.mpc_config(), Some(&mpc));
            let model = Model::from_bytes(&bytes).unwrap();
            let board = Board::new();
            assert_eq!(model.pattern.evaluate(&board), 1);
            assert_eq!(model.pattern.evaluate(&board.board_after_move(1 << 19)), 2);
        }
        file.mpc.eval_search.search_lv_by_depth[4] = 4;
        assert!(Model::from_bytes(&bincode::serialize(&file).unwrap()).is_err());
        file.mpc = mpc.clone();
        bytes.push(0);
        assert!(Model::from_bytes(&bytes).is_err());
        drop(bytes);
        let BinaryEvaluator::Pattern(data) = file.evaluator;
        let mut file = HumanFile {
            format_version: 4,
            metadata,
            evaluator: HumanEvaluator::Pattern(data),
            mpc: mpc.clone(),
        };
        let json = serde_json::to_vec(&file).unwrap();
        let decoded = Model::from_bytes(&json).unwrap();
        assert_eq!(decoded.mpc.eval_search.b.constant, 4.25);
        assert_eq!(decoded.mpc.final_search.a.constant, 1.125);
        // JSON's parser follows the reference serde_json configuration; allow
        // its decimal-to-f64 rounding while requiring the same regression.
        for empties in 12..=60 {
            let actual = decoded.mpc.final_search.params(empties);
            let expected = mpc.final_search.params(empties);
            assert_eq!(actual.lv, expected.lv);
            assert!((actual.a - expected.a).abs() < 1e-12);
            assert!((actual.b - expected.b).abs() < 1e-12);
            assert!((actual.e_std - expected.e_std).abs() < 1e-12);
        }
        assert_eq!(decoded.pattern.evaluate(&Board::new()), 1);
        file.mpc.final_search.search_lv_by_empties[12] = 12;
        assert!(Model::from_bytes(&serde_json::to_vec(&file).unwrap()).is_err());
    }
    #[test]
    fn mpc_settings_reject_invalid_depth_tables_and_preserve_signed_regression() {
        let mut config = MpcConfig::default();
        config.eval_search.search_lv_by_depth.pop();
        assert!(config.validate().is_err());
        let mut config = MpcConfig::default();
        config.final_search.e_std.constant = -100.0;
        assert!(
            config
                .rules()
                .iter()
                .any(|rule| rule.exact && rule.deviation < 0.0)
        );
    }
}
