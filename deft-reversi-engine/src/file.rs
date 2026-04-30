use crate::eval::evaluator::Evaluator;
use crate::eval::evaluator_const::{
    N_MOBILITY_MAX, N_PATTERNS, N_PHASES, PATTERN_TABLE_SIZES, SCORE_SCALE,
};
use crate::search::mpc::MpcConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufReader, BufWriter, Write};

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

#[derive(Serialize, Deserialize, Clone)]
pub struct EvaluatorData {
    pub phases: Vec<PhaseData>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PhaseData {
    pub pattern_weights: Vec<Vec<i16>>,
    pub mobility_weights: Vec<i16>,
    pub bias: i16,
}

impl EngineFile {
    pub fn read_file(path: &str) -> io::Result<Self> {
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let file: EngineFile = bincode::deserialize_from(reader).map_err(invalid_data)?;
        file.validate()?;
        Ok(file)
    }

    pub fn read_string(input: &str) -> io::Result<Self> {
        let file: EngineFile = serde_json::from_str(input).map_err(invalid_data)?;
        file.validate()?;
        Ok(file)
    }

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

    pub fn into_parts(self) -> io::Result<(Metadata, Evaluator, MpcConfig)> {
        let evaluator = Evaluator::from_data(self.evaluator)?;
        Ok((self.metadata, evaluator, self.mpc))
    }

    pub fn from_parts(metadata: Metadata, evaluator: &Evaluator, mpc: &MpcConfig) -> Self {
        Self {
            format_version: 2,
            metadata,
            evaluator: evaluator.to_data(),
            mpc: mpc.clone(),
        }
    }

    fn validate(&self) -> io::Result<()> {
        if self.format_version != 2 {
            return Err(invalid_data(format!(
                "format_version must be 2, got {}",
                self.format_version
            )));
        }
        self.metadata.validate()?;
        self.mpc.validate()?;
        Evaluator::validate_data(&self.evaluator)?;
        Ok(())
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
    use crate::eval::{Evaluator, FeatureIndexes};

    const D3: u64 = 1u64 << 19;

    fn played_board() -> Board {
        let mut board = Board::new();
        let flip = board.flip_bit(D3);
        board.make_move_from_flip_bit(D3, flip);
        board
    }

    #[test]
    fn metadata_default_uses_empty_strings() {
        let metadata = Metadata::default();
        assert_eq!(metadata.trained_at, "");
        assert_eq!(metadata.engine_version, "");
        assert_eq!(metadata.git_commit, "");
    }

    #[test]
    fn engine_file_round_trip_uses_new_schema() {
        let engine_file = EngineFile::from_parts(
            Metadata::default(),
            &Evaluator::default(),
            &MpcConfig::default(),
        );
        let json = serde_json::to_string(&engine_file).unwrap();
        assert!(json.contains("\"format_version\""));
        assert!(json.contains("\"metadata\""));
        assert!(json.contains("\"evaluator\""));
        assert!(json.contains("\"mpc\""));

        let loaded = EngineFile::read_string(&json).unwrap();
        let (_, evaluator, _) = loaded.into_parts().unwrap();
        let board = played_board();
        assert_eq!(
            Evaluator::default().evaluate_board_slow(&board),
            evaluator.evaluate_board_slow(&board)
        );
    }

    #[test]
    fn bundled_engine_file_loads() {
        let engine_file = EngineFile::read_file("../data/eval/eval.bin").unwrap();
        let (_, evaluator, _) = engine_file.into_parts().unwrap();
        let board = played_board();
        let state = FeatureIndexes::from_board(&board);

        assert_eq!(
            evaluator.evaluate_board_slow(&board),
            evaluator.evaluate(&board, &state)
        );
    }

    #[test]
    fn invalid_engine_file_schema_returns_invalid_data() {
        match EngineFile::read_string(r#"{"format_version":2}"#) {
            Ok(_) => panic!("invalid schema should fail"),
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidData),
        }
    }

    #[test]
    fn metadata_round_trip_matches_default() {
        let engine_file = EngineFile::from_parts(
            Metadata::default(),
            &Evaluator::default(),
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
            &Evaluator::default(),
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
