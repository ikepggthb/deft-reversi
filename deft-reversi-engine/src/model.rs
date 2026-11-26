//! 評価モデルファイル（DPR2 形式）の読み書きを行うモジュールです。
//!
//! DPR2（Deft Pattern Reversi v2）はこのエンジン専用のバイナリ形式で、
//! 局面の評価重み、補助ニューラルネット（盤面パターンと局面指標から評価を補正する小型ニューラルネットワーク）、
//! および ProbCut（浅い探索の結果から深い探索での枝刈り可否を統計的に予測する手法）の較正データを格納します。
//!
//! バイナリレイアウト（リトルエンディアン）:
//! - 先頭にマジックナンバー `DPR2`、続いてバージョン番号 `u32`（1〜4）
//! - 60 フェーズ（空きマス数ごとの段階）分の `i16` 重み（パターンごとの重み表、128 個の着手可能数重み、バイアス）
//! - バージョン 2 のみ：`MPC2` タグ、選択率ステージ数（`u8`）、各ステージのパーセント値（`u8`）と
//!   中盤用・終盤完全読み用の標準偏差倍率（`f64` × 2）、続いてルール数（`u32`）と各ルール
//!   （`exact: u8`, `depth: u8`, `empties: u8`, `shallow_depth: u8`, `slope/intercept/deviation: f64`）。
//!   ステージ列はエンジンの [`ProbCutSigmas::STAGES`] と一致していなければなりません。
//!   旧形式の `MPC1` は過去エンジンのステージ構成用であり、このバージョンでは読み込みません。
//! - バージョン 3：60 フェーズ重みに続き、`PEN1` タグと 60 フェーズ分の補助ニューラルネットパラメータ
//!   （`evaluation/pen.rs` 参照）、その後ろに任意でバージョン 2 と同じ ProbCut 較正データが続きます。
//! - バージョン 4：バージョン 3 と同様ですが `PEN2` タグを使用し、石数差入力を含むネットワーク
//!   （隠れユニットあたり `PEN_INPUTS` 個の重み）を格納します。
use crate::{
    evaluation::PatternModel,
    search::{ProbCutRule, ProbCutSigmas, validate_calibration},
};
use std::{
    io::{self, Read, Write},
    path::Path,
};

/// `InvalidData` 種別の `io::Error` を生成するヘルパー関数です。
pub(crate) fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

/// モデルに付随する ProbCut ルール群と各選択率ステージの偏差倍率です。
#[derive(Clone, Debug)]
pub(crate) struct Calibration {
    /// 適用条件（探索深さ・空きマス数・完全読みフラグ）ごとの線形回帰パラメータです。
    pub rules: Vec<ProbCutRule>,
    /// 各選択率ステージにおける中盤・終盤用の偏差倍率（標準偏差 σ の倍率）です。
    pub sigmas: ProbCutSigmas,
}

/// 評価関数の重みと、含まれる場合は ProbCut 較正データを保持するモデルです。
///
/// 較正データを持たないモデルでは、そのままでは枝刈りなしの全幅探索のみが動作します。
/// 選択探索（ProbCut による枝刈り）を有効にするには、較正済みモデルを使うか、
/// `tuning` 機能を有効にしてルールを組み込む必要があります。
#[derive(Clone)]
pub struct Model {
    pub(crate) pattern: PatternModel,
    pub(crate) calibration: Option<Calibration>,
}
impl Model {
    /// 指定パスの DPR2 モデルファイルを読み込みます。
    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }
    /// メモリ上のバイト列から DPR2 モデルをデコードします。
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
    /// モデルファイルから ProbCut 較正部分を無視して評価重みだけを読み込みます。
    /// 旧エンジンの較正を含むモデルに対して、新しいルールで較正をやり直す用途で使います。
    #[cfg(feature = "tuning")]
    pub fn weights_from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let (pattern, _, _) = read_weights(&bytes)?;
        Ok(Self::from_pattern(pattern))
    }
    /// モデルが ProbCut 較正データを保持しているかを返します。
    pub fn has_calibration(&self) -> bool {
        self.calibration.is_some()
    }
    /// モデルを DPR2 形式で書き出します。
    /// 補助ニューラルネットを持つ場合はバージョン 3 または 4（`PatternModel::version` に準拠）として、
    /// 持たない場合は較正の有無に応じてバージョン 2 または 1 として書き出します。
    pub fn write_to(&self, mut out: impl Write) -> io::Result<()> {
        let version = match self.pattern.version() {
            1 if self.calibration.is_some() => 2,
            v => v,
        };
        out.write_all(b"DPR2")?;
        out.write_all(&version.to_le_bytes())?;
        self.pattern.write_phases(&mut out)?;
        if self.pattern.has_pen() {
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
    /// 較正データを持たない評価パターンモデルだけから [`Model`] を構築します。
    #[cfg(feature = "tuning")]
    pub fn from_pattern(pattern: PatternModel) -> Self {
        Self {
            pattern,
            calibration: None,
        }
    }
    /// 内部の [`PatternModel`] への参照を返します。
    #[cfg(feature = "tuning")]
    pub fn pattern(&self) -> &PatternModel {
        &self.pattern
    }
    /// 指定した ProbCut 較正データを検証のうえモデルへ組み込みます（[`Engine`](crate::Engine) と同じ検証を行います）。
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
    /// モデルが持つ ProbCut ルールと偏差倍率を返します（未較正なら `None`）。
    #[cfg(feature = "tuning")]
    pub fn calibration(&self) -> Option<(&[ProbCutRule], ProbCutSigmas)> {
        self.calibration.as_ref().map(|c| (&c.rules[..], c.sigmas))
    }
}

/// 評価重み（および v3/v4 の補助ニューラルネット）を読み込み、残りのバイト列と較正データの有無フラグを返します。
fn read_weights(mut bytes: &[u8]) -> io::Result<(PatternModel, &[u8], bool)> {
    let mut header = [0; 8];
    bytes.read_exact(&mut header)?;
    if &header[..4] != b"DPR2" {
        return Err(invalid_data("not a DPR2 model"));
    }
    let version = u32::from_le_bytes(header[4..].try_into().unwrap());
    if !matches!(version, 1..=4) {
        return Err(invalid_data(format!("unsupported DPR2 version {version}")));
    }
    let mut pattern = PatternModel::read_phases(&mut bytes)?;
    if version >= 3 {
        pattern = pattern.read_pen(&mut bytes, version)?;
    }
    // v2 は較正必須、v3 以降は末尾にデータが残っていれば較正ありとみなす。
    let calibrated = version == 2 || (version >= 3 && !bytes.is_empty());
    Ok((pattern, bytes, calibrated))
}

/// `MPC2` ブロックから ProbCut ステージ倍率とルール群を読み込んで検証します。
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
    // 各 (exact, depth, empties) の組み合わせは最大 1 回だけ現れます。
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

        // 途中で切れたデータ、末尾に余分なバイトがあるデータ、未知のバージョン、不整合なデータは拒否します。
        assert!(Model::from_bytes(&v2[..v2.len() - 1]).is_err());
        let mut longer = v2.clone();
        longer.push(0);
        assert!(Model::from_bytes(&longer).is_err());
        let mut v9 = v2.clone();
        v9[4] = 9;
        assert!(Model::from_bytes(&v9).is_err());
        assert!(Model::from_bytes(b"{\"format_version\":4}").is_err());
        // 重みデータの後ろには、タグ、ステージ数、そして各ステージのパーセント値と 2 つの倍率が並びます。
        let stages = v1.len() + 5;
        let mut bad_sigma = v2.clone();
        bad_sigma[stages + 1..stages + 9].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(Model::from_bytes(&bad_sigma).is_err());
        // 較正データには対象ステージ列が記録されており、現在のエンジンのステージと一致するものだけを読み込みます。
        assert_eq!(v2[stages], 83);
        let mut other_stage = v2.clone();
        other_stage[stages] = 77;
        let error = Model::from_bytes(&other_stage).err().unwrap().to_string();
        assert!(error.contains("[77, 85, 95, 98, 99]"), "{error}");
        let mut earlier = v2.clone();
        earlier[v1.len()..v1.len() + 4].copy_from_slice(b"MPC1");
        let error = Model::from_bytes(&earlier).err().unwrap().to_string();
        assert!(error.contains("earlier engine"), "{error}");
        // 旧形式の較正が含まれていても、評価重みだけは読み込んで較正をやり直せます。
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
