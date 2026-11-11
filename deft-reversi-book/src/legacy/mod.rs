//! 旧book・外部の旧評価を取り込む境界。現在の評価モデルから独立させる。
//! 編集や対局を読む際は、このモジュールを理解する必要はない。
pub(crate) use crate::SearchConditions as QualitySummary;
use crate::{
    Action, AnalysisIdentity, Board, BookError, CompletedSearch, Key, ReadLimits,
    RequestedAnalysis, Result, ScoreInterval, SearchKind, Selectivity, action_code, action_from,
    invalid, selectivity,
};
mod convert;
mod validate;
pub(crate) use validate::{
    ReadCatalog, legacy_summary, rebuild_legacy_play_quality, validate_catalog,
};
mod evidence;
pub(crate) mod value_codec;
mod values;
pub(crate) use convert::dominates;
pub use evidence::ImportedValue;
pub(crate) use evidence::*;
pub(crate) use values::ValueStore;
/// What a stored score is known to be: the value itself, or a bound on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    /// その探索条件での点評価。最終石差が確定したことは意味しない。
    Exact,
    /// 探索条件内での下限。最終石差の保証とは区別する。
    Lower,
    /// 探索条件内での上限。この値の点数を得られる手とは限らない。
    Upper,
    /// Certified bound on final game score, not a point-valued solution.
    ProvenLower,
    /// 最終石差の確定した上限。
    ProvenUpper,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueOrigin {
    Search,
    Manual,
    Imported,
    Rules,
    Derived,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// 簡易表示・比較用の条件。中盤／終盤／混在の区別はValueAnalysis側で確認する。
pub struct AnalysisQuality {
    /// Effective depth in placements, measured from the candidate's parent.
    pub depth: u8,
    pub selectivity: Selectivity,
    /// 最終石差が一点に確定しているか。`Bound::Exact`だけではtrueにならない。
    pub solved: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum PropagationPolicy {
    #[default]
    Quality,
    Book,
}
/// 候補の値から局面全体の最大値を求める。
/// 例：A=+4、B<=+2なら局面=+4と分かるが、全候補に点評価があるわけではない。
/// 戻り値のboolは後者の「全候補が点評価」であり、局面値の有無とは独立する。
/// 下限だけの候補や不明な候補があれば、その候補が最大かもしれず推定値を決めない。
/// 全候補が最終石差の区間を持つ場合は、区間の最大値として別に計算する。
pub(crate) fn combine_values(
    values: impl IntoIterator<Item = Option<Assessment>>,
    identity: AnalysisIdentity,
    root: Key,
    _policy: PropagationPolicy,
) -> (Option<Assessment>, bool) {
    let mut summary = QualitySummary::empty();
    let mut best: Option<Assessment> = None;
    let mut upper = -65;
    let mut all_certified = true;
    let mut certified_lower: Option<i16> = None;
    let mut certified_upper: Option<i16> = Some(-64);
    let mut point_coverage = true;
    let mut complete = true;
    let mut count = 0;
    for value in values {
        let Some(v) = value else {
            return (None, false);
        };
        count += 1;
        if !v.proven() && v.identity() != identity {
            return (None, false);
        }
        summary.include(v.analysis().summary());
        if let Some(interval) = v.claim().certificate() {
            if let Some(l) = interval.lower() {
                certified_lower = Some(certified_lower.map_or(l, |old| old.max(l)));
            }
            certified_upper = match (certified_upper, interval.upper()) {
                (Some(a), Some(b)) => Some(a.max(b)),
                _ => None,
            };
        } else {
            all_certified = false;
        }
        match v.bound() {
            Bound::Exact => {
                if best.is_none_or(|b| v.score() > b.score()) {
                    best = Some(v);
                }
            }
            Bound::Upper | Bound::ProvenUpper => {
                upper = upper.max(v.score());
                complete = false;
            }
            Bound::Lower | Bound::ProvenLower => {
                point_coverage = false;
                complete = false;
            }
        }
    }
    if count == 0 {
        return (None, false);
    }
    let claim = if all_certified {
        ValueClaim::Certified(
            ScoreInterval::new(certified_lower, certified_upper).expect("maximum interval"),
        )
    } else {
        let Some(value) = best else {
            return (None, false);
        };
        if !point_coverage || value.score() < upper {
            return (None, complete);
        }
        ValueClaim::Estimate {
            score: value.score(),
            bound: EstimateBound::Point,
        }
    };
    let analysis = ValueAnalysis::Derived {
        basis: Derivation::Current {
            root: [root.0, root.1],
            subject: CalculationSubject::Position,
        },
        summary,
    };
    (
        Some(Assessment::new(claim, analysis, identity).expect("valid maximum")),
        complete,
    )
}
