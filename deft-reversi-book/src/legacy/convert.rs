//! 旧形式の検査を終えた値と、現在の評価との境界。
use super::*;
use crate::{Bound as CurrentBound, Evaluation, EvaluationSource as Source, Score};

impl From<Assessment> for Evaluation {
    fn from(v: Assessment) -> Self {
        let score = match v.claim() {
            ValueClaim::Certified(v) => Score::Final(v),
            ValueClaim::Estimate { score, bound } => Score::Estimate {
                score,
                bound: match bound {
                    EstimateBound::Point => CurrentBound::Point,
                    EstimateBound::Lower => CurrentBound::Lower,
                    EstimateBound::Upper => CurrentBound::Upper,
                },
            },
        };
        let source = match v.analysis() {
            ValueAnalysis::Search {
                trace,
                projection: SearchProjection::Maximum,
            } => Source::Search(trace),
            ValueAnalysis::Search { trace, .. } => Source::SearchMove(trace),
            ValueAnalysis::Derived {
                basis,
                summary: conditions,
            } => match basis {
                Derivation::Maximum { root, .. }
                | Derivation::Current {
                    root,
                    subject: CalculationSubject::Position,
                } => Source::Position { root, conditions },
                Derivation::Child {
                    child: root,
                    placement,
                    ..
                }
                | Derivation::Current {
                    root,
                    subject: CalculationSubject::FromChild { placement },
                } => Source::Child {
                    root,
                    placement,
                    conditions,
                },
                Derivation::Intersection { root, action, .. } => Source::Move {
                    root,
                    action: action_from(action).expect("validated legacy action"),
                    conditions,
                },
                Derivation::Current {
                    root,
                    subject: CalculationSubject::Candidate(action),
                } => Source::Move {
                    root,
                    action,
                    conditions,
                },
            },
            ValueAnalysis::Rules { root } => Source::Terminal { root },
            ValueAnalysis::Manual { depth, selectivity } => Source::Manual { depth, selectivity },
            ValueAnalysis::LegacyUnknown {
                depth,
                selectivity,
                recorded_origin,
            } => Source::Imported {
                depth,
                selectivity,
                recorded_origin,
            },
        };
        Self::stored(score, source, v.identity())
    }
}
impl From<Evaluation> for Assessment {
    fn from(v: Evaluation) -> Self {
        let claim = match v.result() {
            Score::Final(v) => ValueClaim::Certified(v),
            Score::Estimate { score, bound } => ValueClaim::Estimate {
                score,
                bound: match bound {
                    CurrentBound::Point => EstimateBound::Point,
                    CurrentBound::Lower => EstimateBound::Lower,
                    CurrentBound::Upper => EstimateBound::Upper,
                },
            },
        };
        let analysis = match v.source() {
            Source::Search(trace) => ValueAnalysis::Search {
                trace,
                projection: SearchProjection::Maximum,
            },
            Source::SearchMove(trace) => ValueAnalysis::Search {
                trace,
                projection: if v.bound() == CurrentBound::Point {
                    SearchProjection::BestMove
                } else {
                    SearchProjection::OtherMoveUpper
                },
            },
            Source::Position {
                root,
                conditions: summary,
            } => ValueAnalysis::Derived {
                basis: Derivation::Current {
                    root,
                    subject: CalculationSubject::Position,
                },
                summary,
            },
            Source::Child {
                root,
                placement,
                conditions: summary,
            } => ValueAnalysis::Derived {
                basis: Derivation::Current {
                    root,
                    subject: CalculationSubject::FromChild { placement },
                },
                summary,
            },
            Source::Move {
                root,
                action,
                conditions: summary,
            } => ValueAnalysis::Derived {
                basis: Derivation::Current {
                    root,
                    subject: CalculationSubject::Candidate(action),
                },
                summary,
            },
            Source::Terminal { root } => ValueAnalysis::Rules { root },
            Source::Manual { depth, selectivity } => ValueAnalysis::Manual { depth, selectivity },
            Source::Imported {
                depth,
                selectivity,
                recorded_origin,
            } => ValueAnalysis::LegacyUnknown {
                depth,
                selectivity,
                recorded_origin,
            },
        };
        Self::stored(claim, analysis, v.identity())
    }
}
pub(crate) fn dominates(new: Assessment, old: Assessment) -> bool {
    crate::editor::dominates(new.into(), old.into())
}
impl From<Evaluation> for ImportedValue {
    fn from(value: Evaluation) -> Self {
        Assessment::from(value).as_imported()
    }
}
