//! Model files: DPR2 pattern weights and, from version 2, their ProbCut
//! calibration.
//!
//! Layout, little endian:
//! - `DPR2`, then the version as u32 (1 or 2)
//! - 60 phases of i16 weights: every pattern table back to back, 128
//!   mobility weights, the bias
//! - version 2 only: `MPC2`, the number of selective stages (u8) and for
//!   every stage its probability in percent (u8) and its deviation multiples
//!   for midgame and for exact rules (f64); then the rule count as u32, and
//!   every rule as exact (u8), depth (u8), empties (u8), shallow depth (u8),
//!   slope, intercept and deviation (f64). The stages must be the engine's
//!   (`ProbCutSigmas::STAGES`). `MPC1` held the multiples of the stages of
//!   earlier engines and is not read.
//! - version 3: the phases, `PEN1` and the 60 phases' pattern embedding
//!   networks (see `evaluation/pen.rs`), then optionally the calibration as
//!   in version 2
use crate::{
    evaluation::PatternModel,
    search::{ProbCutRule, ProbCutSigmas, validate_calibration},
};
use std::{
    io::{self, Read, Write},
    path::Path,
};

pub(crate) fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

/// The ProbCut rules of a model and the deviation multiple of each stage.
#[derive(Clone, Debug)]
pub(crate) struct Calibration {
    pub rules: Vec<ProbCutRule>,
    pub sigmas: ProbCutSigmas,
}

/// Evaluation weights with their ProbCut calibration, if the file has one.
/// Without a calibration the engine searches selectively only after rules
/// are installed (`tuning`); full-width searches work either way.
#[derive(Clone)]
pub struct Model {
    pub(crate) pattern: PatternModel,
    pub(crate) calibration: Option<Calibration>,
}
impl Model {
    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let (pattern, mut rest, calibrated) = read_weights(bytes)?;
        let calibration = if calibrated {
            Some(read_calibration(&mut rest)?)
        } else {
            None
        };
        if !rest.is_empty() {
            return Err(invalid_data("trailing model data"));
        }
        Ok(Self {
            pattern,
            calibration,
        })
    }
    /// The weights of a model file without its calibration, which is not
    /// read: to fit a calibration again, also one an earlier engine wrote.
    #[cfg(feature = "tuning")]
    pub fn weights_from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let (pattern, _, _) = read_weights(&bytes)?;
        Ok(Self::from_pattern(pattern))
    }
    /// Whether the file carried a ProbCut calibration.
    pub fn has_calibration(&self) -> bool {
        self.calibration.is_some()
    }
    /// Writes the model: version 3 with a pattern embedding network, else
    /// version 2 with its calibration and version 1 without.
    pub fn write_to(&self, mut out: impl Write) -> io::Result<()> {
        let version: u32 = if self.pattern.has_pen() {
            3
        } else if self.calibration.is_some() {
            2
        } else {
            1
        };
        out.write_all(b"DPR2")?;
        out.write_all(&version.to_le_bytes())?;
        self.pattern.write_phases(&mut out)?;
        if self.pattern.has_pen() {
            out.write_all(b"PEN1")?;
            self.pattern.write_pen(&mut out)?;
        }
        if let Some(c) = &self.calibration {
            out.write_all(b"MPC2")?;
            out.write_all(&[ProbCutSigmas::STAGES.len() as u8])?;
            for (i, stage) in ProbCutSigmas::STAGES.iter().enumerate() {
                out.write_all(&[stage.percent()])?;
                out.write_all(&c.sigmas.midgame[i].to_le_bytes())?;
                out.write_all(&c.sigmas.exact[i].to_le_bytes())?;
            }
            out.write_all(&(c.rules.len() as u32).to_le_bytes())?;
            for r in &c.rules {
                out.write_all(&[u8::from(r.exact), r.depth, r.empties, r.shallow_depth])?;
                for v in [r.slope, r.intercept, r.deviation] {
                    out.write_all(&v.to_le_bytes())?;
                }
            }
        }
        Ok(())
    }
    /// Weights alone, without a calibration.
    #[cfg(feature = "tuning")]
    pub fn from_pattern(pattern: PatternModel) -> Self {
        Self {
            pattern,
            calibration: None,
        }
    }
    #[cfg(feature = "tuning")]
    pub fn pattern(&self) -> &PatternModel {
        &self.pattern
    }
    /// The model with this calibration, checked as `Engine` checks rules.
    #[cfg(feature = "tuning")]
    pub fn with_calibration(
        mut self,
        rules: Vec<ProbCutRule>,
        sigmas: ProbCutSigmas,
    ) -> Result<Self, crate::EngineError> {
        validate_calibration(&rules, &sigmas)?;
        self.calibration = Some(Calibration { rules, sigmas });
        Ok(self)
    }
    /// The calibration's rules and multiples, if the model has one.
    #[cfg(feature = "tuning")]
    pub fn calibration(&self) -> Option<(&[ProbCutRule], ProbCutSigmas)> {
        self.calibration.as_ref().map(|c| (&c.rules[..], c.sigmas))
    }
}

/// The weights, the bytes after them and whether those are a calibration.
fn read_weights(mut bytes: &[u8]) -> io::Result<(PatternModel, &[u8], bool)> {
    let mut header = [0; 8];
    bytes.read_exact(&mut header)?;
    if &header[..4] != b"DPR2" {
        return Err(invalid_data("not a DPR2 model"));
    }
    let version = u32::from_le_bytes(header[4..].try_into().unwrap());
    if !matches!(version, 1..=3) {
        return Err(invalid_data(format!("unsupported DPR2 version {version}")));
    }
    let mut pattern = PatternModel::read_phases(&mut bytes)?;
    if version == 3 {
        let mut tag = [0; 4];
        bytes.read_exact(&mut tag)?;
        if &tag != b"PEN1" {
            return Err(invalid_data("missing pattern embedding network"));
        }
        pattern = pattern.read_pen(&mut bytes)?;
    }
    let calibrated = version == 2 || (version == 3 && !bytes.is_empty());
    Ok((pattern, bytes, calibrated))
}

fn read_calibration(input: &mut &[u8]) -> io::Result<Calibration> {
    const REFIT: &str = "fit the calibration again (`train mpc --fit-only`)";
    let mut tag = [0; 4];
    input.read_exact(&mut tag)?;
    if &tag == b"MPC1" {
        return Err(invalid_data(format!(
            "the ProbCut calibration is for the stages of an earlier engine; {REFIT}"
        )));
    }
    if &tag != b"MPC2" {
        return Err(invalid_data("missing ProbCut calibration"));
    }
    let mut count = [0; 1];
    input.read_exact(&mut count)?;
    let mut percents = Vec::new();
    let mut multiples = Vec::new();
    for _ in 0..count[0] {
        let mut stage = [0; 17];
        input.read_exact(&mut stage)?;
        percents.push(stage[0]);
        multiples
            .push([&stage[1..9], &stage[9..]].map(|b| f64::from_le_bytes(b.try_into().unwrap())));
    }
    let stages = ProbCutSigmas::STAGES.map(|stage| stage.percent());
    if percents != stages {
        return Err(invalid_data(format!(
            "the ProbCut calibration is for the stages {percents:?}%, the engine's are \
             {stages:?}%; {REFIT}"
        )));
    }
    let sigmas = ProbCutSigmas {
        midgame: std::array::from_fn(|i| multiples[i][0]),
        exact: std::array::from_fn(|i| multiples[i][1]),
    };
    let mut count = [0; 4];
    input.read_exact(&mut count)?;
    let count = u32::from_le_bytes(count) as usize;
    // Every (exact, depth, empties) triple at most once.
    if count > 2 * 61 * 61 {
        return Err(invalid_data("too many ProbCut rules"));
    }
    let mut rules = Vec::with_capacity(count);
    for _ in 0..count {
        let mut head = [0; 4];
        input.read_exact(&mut head)?;
        if head[0] > 1 {
            return Err(invalid_data("invalid ProbCut rule"));
        }
        let mut v = [0.0; 3];
        for x in &mut v {
            let mut b = [0; 8];
            input.read_exact(&mut b)?;
            *x = f64::from_le_bytes(b);
        }
        rules.push(ProbCutRule {
            exact: head[0] == 1,
            depth: head[1],
            empties: head[2],
            shallow_depth: head[3],
            slope: v[0],
            intercept: v[1],
            deviation: v[2],
        });
    }
    validate_calibration(&rules, &sigmas).map_err(invalid_data)?;
    Ok(Calibration { rules, sigmas })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Board;

    fn model_with_bias() -> PatternModel {
        let mut bytes = b"DPR2\x01\x00\x00\x00".to_vec();
        for phase in 0..60i16 {
            let n = crate::evaluation::PATTERN_WEIGHT_COUNT + 128;
            bytes.extend(std::iter::repeat_n(0u8, n * 2));
            bytes.extend(((phase + 1) * 128).to_le_bytes());
        }
        Model::from_bytes(&bytes).unwrap().pattern
    }

    #[test]
    fn version_one_and_two_round_trip_with_and_without_calibration() {
        let pattern = model_with_bias();
        let plain = Model {
            pattern: pattern.clone(),
            calibration: None,
        };
        let mut v1 = Vec::new();
        plain.write_to(&mut v1).unwrap();
        assert_eq!(&v1[..8], b"DPR2\x01\x00\x00\x00");
        let read = Model::from_bytes(&v1).unwrap();
        assert!(!read.has_calibration());
        let board = Board::new();
        assert_eq!(read.pattern.evaluate(&board), 1);
        assert_eq!(read.pattern.evaluate(&board.board_after_move(1 << 19)), 2);

        let mut sigmas = ProbCutSigmas::NORMAL;
        sigmas.exact[4] = 3.0;
        let rules = crate::mpc::legacy_main_rules();
        let calibrated = Model {
            pattern,
            calibration: Some(Calibration {
                rules: rules.clone(),
                sigmas,
            }),
        };
        let mut v2 = Vec::new();
        calibrated.write_to(&mut v2).unwrap();
        assert_eq!(&v2[..8], b"DPR2\x02\x00\x00\x00");
        assert_eq!(&v2[..v1.len()][8..], &v1[8..]);
        let read = Model::from_bytes(&v2).unwrap();
        let c = read.calibration.unwrap();
        assert_eq!(c.sigmas, sigmas);
        assert_eq!(c.rules.len(), rules.len());
        assert!(c.rules.iter().zip(&rules).all(|(a, b)| a.exact == b.exact
            && (a.depth, a.empties, a.shallow_depth) == (b.depth, b.empties, b.shallow_depth)
            && a.slope.to_bits() == b.slope.to_bits()
            && a.intercept.to_bits() == b.intercept.to_bits()
            && a.deviation.to_bits() == b.deviation.to_bits()));

        // Truncated, trailing, unknown and inconsistent files are rejected.
        assert!(Model::from_bytes(&v2[..v2.len() - 1]).is_err());
        let mut longer = v2.clone();
        longer.push(0);
        assert!(Model::from_bytes(&longer).is_err());
        let mut v9 = v2.clone();
        v9[4] = 9;
        assert!(Model::from_bytes(&v9).is_err());
        assert!(Model::from_bytes(b"{\"format_version\":4}").is_err());
        // After the weights: the tag, the stage count, then every stage as
        // its percent and two multiples.
        let stages = v1.len() + 5;
        let mut bad_sigma = v2.clone();
        bad_sigma[stages + 1..stages + 9].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(Model::from_bytes(&bad_sigma).is_err());
        // A calibration says which stages its multiples are for; only the
        // engine's own are read.
        assert_eq!(v2[stages], 83);
        let mut other_stage = v2.clone();
        other_stage[stages] = 77;
        let error = Model::from_bytes(&other_stage).err().unwrap().to_string();
        assert!(error.contains("[77, 85, 95, 98, 99]"), "{error}");
        let mut earlier = v2.clone();
        earlier[v1.len()..v1.len() + 4].copy_from_slice(b"MPC1");
        let error = Model::from_bytes(&earlier).err().unwrap().to_string();
        assert!(error.contains("earlier engine"), "{error}");
        // Its weights can still be read, to fit the calibration again.
        #[cfg(feature = "tuning")]
        {
            let path = std::env::temp_dir().join(format!("deft-mpc1-{}.dpr2", std::process::id()));
            std::fs::write(&path, &earlier).unwrap();
            let weights = Model::weights_from_file(&path).unwrap();
            std::fs::remove_file(&path).unwrap();
            assert!(!weights.has_calibration());
            assert_eq!(weights.pattern.evaluate(&board), 1);
        }
        let mut v2_without = v1.clone();
        v2_without[4] = 2;
        assert!(Model::from_bytes(&v2_without).is_err());
    }
}
