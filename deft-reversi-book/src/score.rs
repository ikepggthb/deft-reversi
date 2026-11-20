//! 推定の点数・上下限と、最終石差の確定範囲。
use super::*;
/// 推定の点数が一点か、下限か、上限か。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    Point,
    Lower,
    Upper,
}

/// 点数の意味を一か所で表す。Finalも、範囲なら一点の読み切り結果ではない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Score {
    Estimate { score: i16, bound: Bound },
    Final(ScoreInterval),
}
impl Score {
    pub fn point(score: i16) -> Self {
        Self::Estimate {
            score,
            bound: Bound::Point,
        }
    }
    /// 点評価なら点数、区間なら上限（なければ下限）。着手選択にはpoint_valueも確認する。
    pub fn value(self) -> i16 {
        match self {
            Self::Estimate { score, .. } => score,
            Self::Final(v) => v.upper().or(v.lower()).expect("nonempty score range"),
        }
    }
    pub fn point_value(self) -> Option<i16> {
        (self.bound() == Bound::Point).then(|| self.value())
    }
    pub fn bound(self) -> Bound {
        match self {
            Self::Estimate { bound, .. } => bound,
            Self::Final(v) if v.exact().is_some() => Bound::Point,
            Self::Final(v) if v.upper().is_some() => Bound::Upper,
            Self::Final(_) => Bound::Lower,
        }
    }
    pub fn final_bounds(self) -> Option<ScoreInterval> {
        match self {
            Self::Final(v) => Some(v),
            _ => None,
        }
    }
    pub fn is_solved(self) -> bool {
        self.final_bounds().is_some_and(|v| v.exact().is_some())
    }
    pub(crate) fn negated(self) -> Self {
        match self {
            Self::Final(v) => Self::Final(v.negated()),
            Self::Estimate { score, bound } => Self::Estimate {
                score: -score,
                bound: match bound {
                    Bound::Point => Bound::Point,
                    Bound::Lower => Bound::Upper,
                    Bound::Upper => Bound::Lower,
                },
            },
        }
    }
}
/// 最終石差の確定した範囲。Noneの側は不明で、両端が同じなら石差が確定する。
/// 点数の規約はEngineに合わせ、-64..=64の偶数だけを受け付ける。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScoreInterval {
    lower: Option<i16>,
    upper: Option<i16>,
}
impl ScoreInterval {
    pub fn new(lower: Option<i16>, upper: Option<i16>) -> Result<Self> {
        if lower.is_none() && upper.is_none()
            || lower
                .into_iter()
                .chain(upper)
                .any(|v| !(-64..=64).contains(&v) || v % 2 != 0)
            || matches!((lower,upper),(Some(l),Some(u)) if l>u)
        {
            return Err(invalid("invalid certified score interval"));
        }
        Ok(Self { lower, upper })
    }
    pub fn point(score: i16) -> Result<Self> {
        Self::new(Some(score), Some(score))
    }
    pub fn lower(self) -> Option<i16> {
        self.lower
    }
    pub fn upper(self) -> Option<i16> {
        self.upper
    }
    pub fn exact(self) -> Option<i16> {
        self.lower.filter(|v| Some(*v) == self.upper)
    }
    pub fn intersect(self, other: Self) -> Result<Self> {
        Self::new(
            match (self.lower, other.lower) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            },
            match (self.upper, other.upper) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        )
    }
    pub(crate) fn negated(self) -> Self {
        Self {
            lower: self.upper.map(|v| -v),
            upper: self.lower.map(|v| -v),
        }
    }
}
