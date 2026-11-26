//! 評価スコアの種類（点評価・上界・下界）と、終局までの確定石差区間を表すモジュール。
use super::*;
/// スコアが「ちょうどその値（点評価）」か、「その値以上（下界）」か、「その値以下（上界）」かを表す。
///
/// α-β 探索や部分的な枝刈りでは、正確な点数の代わりに上界や下界が得られることがある。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    /// 正確な 1 点のスコア。
    Point,
    /// 少なくともこの値以上である（下界）。
    Lower,
    /// 多くともこの値以下である（上界）。
    Upper,
}

/// 評価スコアの意味を一か所で表現する型。
/// 終局まで読み切った結果（`Final`）であっても、幅のある区間なら 1 点の完全読み切りではない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Score {
    /// 探索による推定スコア（点評価または上下界）。
    Estimate { score: i16, bound: Bound },
    /// 終局までの完全読みで保証された最終石差の区間。
    Final(ScoreInterval),
}
impl Score {
    pub fn point(score: i16) -> Self {
        Self::Estimate {
            score,
            bound: Bound::Point,
        }
    }
    /// 点評価ならその点数、区間なら上界（上界がなければ下界）を返す。
    /// 実際に着手として選ぶ際は、点評価であることを確かめるため `point_value` も確認する。
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
/// 完全読みで保証された最終石差の範囲。
/// `None` の側は限界が不明であることを表し、上下が同じ値なら石差が 1 点に確定する。
/// 点数の規約は `Engine` に合わせ、`-64..=64` の偶数だけを受け付ける。
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
