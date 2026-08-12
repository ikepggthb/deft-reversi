use crate::eval::evaluator::{evaluator_from_data, validate_evaluator_data, Evaluator};
use crate::eval::evaluator_const::{
    N_MOBILITY_MAX, N_PATTERNS, N_PHASES, PATTERN_TABLE_SIZES, SCORE_SCALE,
};
use crate::eval::nnue_features::{
    NNUE_ACCUMULATOR_SIZE, NNUE_ACTIVATION_SCALE, NNUE_DEFAULT_PATTERN_SET,
    NNUE_DEFAULT_SHARE_ROTATIONS, NNUE_DEFAULT_WEIGHT_SCALE, NNUE_DENSE_INPUT_SIZE,
    NNUE_DENSE_LAYER_SIZES, NNUE_INPUT_SIZE, NNUE_TOWER_COUNT,
};
use crate::search::mpc::MpcConfig;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::io::{self, BufReader, BufWriter, Write};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone)]
pub struct EngineFile {
    pub format_version: u32,
    pub metadata: Metadata,
    pub evaluator: EvaluatorData,
    pub mpc: MpcConfig,
}

/// engine file に埋め込むメタデータ。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
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

#[derive(Clone)]
pub enum EvaluatorData {
    Pattern(PatternEvaluatorData),
    Nnue(NnueEvaluatorData),
}

impl Serialize for EvaluatorData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            #[derive(Serialize)]
            #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
            enum Human<'a> {
                Pattern(&'a PatternEvaluatorData),
                Nnue(&'a NnueEvaluatorData),
            }
            match self {
                Self::Pattern(data) => Human::Pattern(data).serialize(serializer),
                Self::Nnue(data) => Human::Nnue(data).serialize(serializer),
            }
        } else {
            #[derive(Serialize)]
            enum Binary<'a> {
                Pattern(&'a PatternEvaluatorData),
                Nnue(&'a NnueEvaluatorData),
            }
            match self {
                Self::Pattern(data) => Binary::Pattern(data).serialize(serializer),
                Self::Nnue(data) => Binary::Nnue(data).serialize(serializer),
            }
        }
    }
}

impl<'de> Deserialize<'de> for EvaluatorData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
            enum Human {
                Pattern(PatternEvaluatorData),
                Nnue(NnueEvaluatorData),
            }
            match Human::deserialize(deserializer)? {
                Human::Pattern(data) => Ok(Self::Pattern(data)),
                Human::Nnue(data) => Ok(Self::Nnue(data)),
            }
        } else {
            #[derive(Deserialize)]
            enum Binary {
                Pattern(PatternEvaluatorData),
                Nnue(NnueEvaluatorData),
            }
            match Binary::deserialize(deserializer)? {
                Binary::Pattern(data) => Ok(Self::Pattern(data)),
                Binary::Nnue(data) => Ok(Self::Nnue(data)),
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PatternEvaluatorData {
    pub phases: Vec<PhaseData>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PhaseData {
    pub pattern_weights: Vec<Vec<i16>>,
    pub mobility_weights: Vec<i16>,
    pub bias: i16,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NnueEvaluatorData {
    pub input_size: usize,
    pub accumulator_size: usize,
    pub activation_scale: i32,
    pub weight_scale: i32,
    pub tower_count: usize,
    pub pairwise: bool,
    pub input_weights: Vec<Vec<i16>>,
    pub input_bias: Vec<i32>,
    pub towers: Vec<NnueTowerData>,
    #[serde(default = "default_nnue_acc_size")]
    pub acc_size: usize,
    #[serde(default = "default_nnue_pattern_set")]
    pub pattern_set: String,
    #[serde(default = "default_nnue_share_rotations")]
    pub share_rotations: bool,
}

fn default_nnue_acc_size() -> usize {
    NNUE_ACCUMULATOR_SIZE
}

fn default_nnue_pattern_set() -> String {
    NNUE_DEFAULT_PATTERN_SET.to_string()
}

fn default_nnue_share_rotations() -> bool {
    NNUE_DEFAULT_SHARE_ROTATIONS
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NnueTowerData {
    pub dense_layers: Vec<DenseLayerData>,
    pub output_weights: Vec<i16>,
    pub output_bias: i32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DenseLayerData {
    pub input_size: usize,
    pub output_size: usize,
    pub weights: Vec<Vec<i16>>,
    pub bias: Vec<i32>,
}

impl EngineFile {
    // File-path reader retained for engine conversion/export tooling.
    #[allow(dead_code)]
    pub fn read_file(path: &str) -> io::Result<Self> {
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let file: EngineFile = match bincode::deserialize_from(reader) {
            Ok(file) => file,
            Err(err) => read_legacy_engine_file(path).map_err(|_| invalid_data(err))?,
        };
        file.validate()?;
        Ok(file)
    }

    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let file: EngineFile = match bincode::deserialize(bytes) {
            Ok(file) => file,
            Err(err) => read_legacy_engine_bytes(bytes).map_err(|_| invalid_data(err))?,
        };
        file.validate()?;
        Ok(file)
    }

    pub fn read_string(input: &str) -> io::Result<Self> {
        let file: EngineFile = match serde_json::from_str(input) {
            Ok(file) => file,
            Err(err) => read_legacy_pattern_json(input).map_err(|_| invalid_data(err))?,
        };
        file.validate()?;
        Ok(file)
    }

    // File-path writer retained for engine conversion/export tooling.
    #[allow(dead_code)]
    pub fn write_file(&self, path: &str) -> io::Result<()> {
        self.validate()?;
        let mut file = fs::File::create(path)?;
        {
            let mut writer = BufWriter::new(&mut file);
            bincode::serialize_into(&mut writer, self).map_err(invalid_data)?;
            writer.flush()?;
        }
        file.flush()
    }

    pub fn into_parts(self) -> io::Result<(Metadata, Arc<dyn Evaluator>, MpcConfig)> {
        let evaluator = evaluator_from_data(self.evaluator)?;
        Ok((self.metadata, evaluator, self.mpc))
    }

    // Constructor retained for engine conversion/export tooling.
    #[allow(dead_code)]
    pub fn from_parts(metadata: Metadata, evaluator: EvaluatorData, mpc: &MpcConfig) -> Self {
        Self {
            format_version: 4,
            metadata,
            evaluator,
            mpc: mpc.clone(),
        }
    }

    fn validate(&self) -> io::Result<()> {
        if !matches!(self.format_version, 3 | 4) {
            return Err(invalid_data(format!(
                "format_version must be 3 or 4, got {}",
                self.format_version
            )));
        }
        self.metadata.validate()?;
        self.mpc.validate()?;
        validate_evaluator_data(&self.evaluator)?;
        Ok(())
    }
}

fn read_legacy_pattern_json(input: &str) -> io::Result<EngineFile> {
    #[derive(Deserialize)]
    struct LegacyPatternFile {
        evaluator: PatternEvaluatorData,
    }

    let legacy: LegacyPatternFile = serde_json::from_str(input).map_err(invalid_data)?;
    Ok(EngineFile {
        format_version: 4,
        metadata: Metadata {
            eval_name: "legacy-pattern".to_string(),
            ..Metadata::default()
        },
        evaluator: EvaluatorData::Pattern(legacy.evaluator),
        mpc: MpcConfig::default(),
    })
}

#[derive(Deserialize)]
// Legacy binary formats retained for conversion tooling.
#[allow(dead_code)]
struct LegacyEngineFile {
    format_version: u32,
    metadata: Metadata,
    evaluator: LegacyEvaluatorData,
    mpc: MpcConfig,
}

#[derive(Deserialize)]
// Legacy evaluator enum retained for conversion tooling.
#[allow(dead_code)]
enum LegacyEvaluatorData {
    Pattern(PatternEvaluatorData),
    Nnue(LegacyNnueEvaluatorData),
}

#[derive(Deserialize)]
// Legacy NNUE payload retained for conversion tooling.
#[allow(dead_code)]
struct LegacyNnueEvaluatorData {
    input_size: usize,
    accumulator_size: usize,
    activation_scale: i32,
    weight_scale: i32,
    psqt_buckets: usize,
    psqt_scale: i32,
    tower_count: usize,
    pairwise: bool,
    input_weights: Vec<Vec<i16>>,
    psqt_weights: Vec<Vec<i16>>,
    input_bias: Vec<i32>,
    towers: Vec<NnueTowerData>,
    #[serde(default = "default_nnue_acc_size")]
    acc_size: usize,
    #[serde(default = "default_nnue_pattern_set")]
    pattern_set: String,
    #[serde(default = "default_nnue_share_rotations")]
    share_rotations: bool,
}

// Legacy binary reader retained for conversion tooling.
#[allow(dead_code)]
fn read_legacy_engine_file(path: &str) -> io::Result<EngineFile> {
    let bytes = fs::read(path)?;
    read_legacy_engine_bytes(&bytes)
}

fn read_legacy_engine_bytes(bytes: &[u8]) -> io::Result<EngineFile> {
    let legacy: LegacyEngineFile = bincode::deserialize(bytes).map_err(invalid_data)?;
    Ok(legacy_engine_file_to_current(legacy))
}

fn legacy_engine_file_to_current(legacy: LegacyEngineFile) -> EngineFile {
    EngineFile {
        format_version: 4,
        metadata: legacy.metadata,
        evaluator: match legacy.evaluator {
            LegacyEvaluatorData::Pattern(data) => EvaluatorData::Pattern(data),
            LegacyEvaluatorData::Nnue(data) => EvaluatorData::Nnue(NnueEvaluatorData {
                input_size: data.input_size,
                accumulator_size: data.accumulator_size,
                activation_scale: data.activation_scale,
                weight_scale: data.weight_scale,
                tower_count: data.tower_count,
                pairwise: data.pairwise,
                input_weights: data.input_weights,
                input_bias: data.input_bias,
                towers: data.towers,
                acc_size: data.acc_size,
                pattern_set: data.pattern_set,
                share_rotations: data.share_rotations,
            }),
        },
        mpc: legacy.mpc,
    }
}

impl Default for PhaseData {
    fn default() -> Self {
        Self {
            pattern_weights: PATTERN_TABLE_SIZES
                .iter()
                .map(|&size| vec![0; size])
                .collect(),
            mobility_weights: vec![0; N_MOBILITY_MAX],
            bias: 0,
        }
    }
}

impl Default for PatternEvaluatorData {
    fn default() -> Self {
        Self {
            phases: (0..N_PHASES).map(|_| PhaseData::default()).collect(),
        }
    }
}

impl Default for NnueEvaluatorData {
    fn default() -> Self {
        Self {
            input_size: NNUE_INPUT_SIZE,
            acc_size: NNUE_ACCUMULATOR_SIZE,
            pattern_set: NNUE_DEFAULT_PATTERN_SET.to_string(),
            share_rotations: NNUE_DEFAULT_SHARE_ROTATIONS,
            accumulator_size: NNUE_ACCUMULATOR_SIZE,
            activation_scale: NNUE_ACTIVATION_SCALE,
            weight_scale: NNUE_DEFAULT_WEIGHT_SCALE,
            tower_count: NNUE_TOWER_COUNT,
            pairwise: true,
            input_weights: vec![vec![0; NNUE_ACCUMULATOR_SIZE]; NNUE_INPUT_SIZE],
            input_bias: vec![0; NNUE_ACCUMULATOR_SIZE],
            towers: (0..NNUE_TOWER_COUNT)
                .map(|_| NnueTowerData::default())
                .collect(),
        }
    }
}

impl Default for NnueTowerData {
    fn default() -> Self {
        let mut dense_layers = Vec::with_capacity(NNUE_DENSE_LAYER_SIZES.len());
        let mut input_size = NNUE_DENSE_INPUT_SIZE;
        for &output_size in &NNUE_DENSE_LAYER_SIZES {
            dense_layers.push(DenseLayerData {
                input_size,
                output_size,
                weights: vec![vec![0; input_size]; output_size],
                bias: vec![0; output_size],
            });
            input_size = output_size;
        }
        Self {
            dense_layers,
            output_weights: vec![0; *NNUE_DENSE_LAYER_SIZES.last().unwrap()],
            output_bias: 0,
        }
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            eval_name: "default".to_string(),
            eval_version: "0".to_string(),
            trained_at: String::new(),
            engine_version: String::new(),
            git_commit: String::new(),
            n_data_set: 0,
            n_iteration: 0,
            score_scale: SCORE_SCALE,
            n_phases: N_PHASES,
            n_patterns: N_PATTERNS,
            pattern_schema_version: 1,
        }
    }
}

impl Metadata {
    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.score_scale != SCORE_SCALE {
            return Err(invalid_data(format!(
                "score_scale must be {SCORE_SCALE}, got {}",
                self.score_scale
            )));
        }
        if self.n_phases != N_PHASES {
            return Err(invalid_data(format!(
                "n_phases must be {N_PHASES}, got {}",
                self.n_phases
            )));
        }
        if self.n_patterns != N_PATTERNS {
            return Err(invalid_data(format!(
                "n_patterns must be {N_PATTERNS}, got {}",
                self.n_patterns
            )));
        }
        if self.eval_name.is_empty() {
            return Err(invalid_data("eval_name must not be empty"));
        }
        if self.eval_version.is_empty() {
            return Err(invalid_data("eval_version must not be empty"));
        }
        Ok(())
    }
}

pub(crate) fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;
    use crate::eval::default_evaluator;
    use crate::eval::nnue_evaluator::NnueEvaluator;

    const D3: u64 = 1u64 << 19;

    fn played_board() -> Board {
        let board = Board::new();
        let flip = board.flip_bit(D3);
        board.make_move_from_flip_bit(D3, flip)
    }

    #[test]
    fn metadata_default_uses_empty_strings() {
        let metadata = Metadata::default();
        assert_eq!(metadata.trained_at, "");
        assert_eq!(metadata.engine_version, "");
        assert_eq!(metadata.git_commit, "");
    }

    #[test]
    fn engine_file_round_trip_uses_v4_pattern_schema() {
        let engine_file = EngineFile::from_parts(
            Metadata::default(),
            EvaluatorData::Pattern(PatternEvaluatorData::default()),
            &MpcConfig::default(),
        );
        let json = serde_json::to_string(&engine_file).unwrap();
        assert!(json.contains("\"format_version\":4"));
        assert!(json.contains("\"metadata\""));
        assert!(json.contains("\"evaluator\""));
        assert!(json.contains("\"kind\":\"pattern\""));
        assert!(json.contains("\"mpc\""));

        let loaded = EngineFile::read_string(&json).unwrap();
        let (_, evaluator, _) = loaded.into_parts().unwrap();
        let board = played_board();
        assert_eq!(
            default_evaluator().evaluate(&board),
            evaluator.evaluate(&board)
        );
    }

    #[test]
    fn engine_file_round_trip_uses_v4_nnue_schema() {
        let engine_file = EngineFile::from_parts(
            Metadata::default(),
            EvaluatorData::Nnue(NnueEvaluatorData::default()),
            &MpcConfig::default(),
        );
        let json = serde_json::to_string(&engine_file).unwrap();
        assert!(json.contains("\"format_version\":4"));
        assert!(json.contains("\"kind\":\"nnue\""));

        let loaded = EngineFile::read_string(&json).unwrap();
        let (_, evaluator, _) = loaded.into_parts().unwrap();
        let board = played_board();
        assert_eq!(
            NnueEvaluator::default().evaluate_board_slow(&board),
            evaluator.evaluate(&board)
        );
    }

    #[test]
    fn invalid_engine_file_schema_returns_invalid_data() {
        match EngineFile::read_string(r#"{"format_version":3}"#) {
            Ok(_) => panic!("invalid schema should fail"),
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidData),
        }
    }

    #[test]
    fn format_version_2_returns_invalid_data() {
        let mut engine_file = EngineFile::from_parts(
            Metadata::default(),
            EvaluatorData::Pattern(PatternEvaluatorData::default()),
            &MpcConfig::default(),
        );
        engine_file.format_version = 2;

        match EngineFile::read_string(&serde_json::to_string(&engine_file).unwrap()) {
            Ok(_) => panic!("format_version 2 should fail"),
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidData),
        }
    }

    #[test]
    fn metadata_round_trip_matches_default() {
        let engine_file = EngineFile::from_parts(
            Metadata::default(),
            EvaluatorData::Pattern(PatternEvaluatorData::default()),
            &MpcConfig::default(),
        );
        let loaded =
            EngineFile::read_string(&serde_json::to_string(&engine_file).unwrap()).unwrap();

        assert_eq!(loaded.metadata, engine_file.metadata);
        assert_eq!(
            loaded.mpc.eval_search.search_lv_by_depth,
            engine_file.mpc.eval_search.search_lv_by_depth
        );
        assert_eq!(
            loaded.mpc.final_search.search_lv_by_empties,
            engine_file.mpc.final_search.search_lv_by_empties
        );
        assert_eq!(
            loaded.mpc.final_search.params(12).lv,
            engine_file.mpc.final_search.params(12).lv
        );
        assert_eq!(loaded.metadata.score_scale, SCORE_SCALE);
        assert_eq!(loaded.metadata.n_phases, N_PHASES);
        assert_eq!(loaded.metadata.n_patterns, N_PATTERNS);
    }

    #[test]
    fn invalid_mpc_search_level_returns_invalid_data() {
        let mut engine_file = EngineFile::from_parts(
            Metadata::default(),
            EvaluatorData::Pattern(PatternEvaluatorData::default()),
            &MpcConfig::default(),
        );
        engine_file.mpc.eval_search.search_lv_by_depth[10] = 61;

        let json = serde_json::to_string(&engine_file).unwrap();
        match EngineFile::read_string(&json) {
            Ok(_) => panic!("invalid mpc level should fail"),
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidData),
        }
    }
}
