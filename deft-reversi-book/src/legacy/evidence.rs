//! 旧ファイルの評価表現。通常の編集・生成・対局では使わない。
use super::*;

/// その探索条件での推定値の意味。最終石差の保証には使わない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EstimateBound {
    Point,
    Lower,
    Upper,
}
/// 推定値と最終石差の情報を区別する。Certifiedでも片側だけなら読み切った点数ではない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueClaim {
    Estimate { score: i16, bound: EstimateBound },
    Certified(ScoreInterval),
}
impl ValueClaim {
    pub fn certificate(self) -> Option<ScoreInterval> {
        if let Self::Certified(v) = self {
            Some(v)
        } else {
            None
        }
    }
    fn negated(self) -> Self {
        match self {
            Self::Certified(v) => Self::Certified(v.negated()),
            Self::Estimate { score, bound } => Self::Estimate {
                score: -score,
                bound: match bound {
                    EstimateBound::Point => EstimateBound::Point,
                    EstimateBound::Lower => EstimateBound::Upper,
                    EstimateBound::Upper => EstimateBound::Lower,
                },
            },
        }
    }
}
/// 一回の探索結果をどの対象に使うか。同じCompletedSearchを共有できる。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchProjection {
    /// trace.movesに含む手の最大値。全合法手を含むとは限らない。
    Maximum,
    /// trace.bestだけの点評価。
    BestMove,
    /// trace.moves内の最良手以外に付ける上限。各手の点評価は分からない。
    OtherMoveUpper,
}
/// The board in a calculated value is either the position itself, the child
/// whose value was negated, or the parent of a particular candidate move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalculationSubject {
    Position,
    FromChild { placement: bool },
    Candidate(Action),
}
impl CalculationSubject {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Position => 0,
            Self::FromChild { placement } => 1 + u8::from(placement),
            Self::Candidate(a) => 3 + action_code(a),
        }
    }
    pub(crate) fn from_code(n: u8) -> Result<Self> {
        match n {
            0 => Ok(Self::Position),
            1 | 2 => Ok(Self::FromChild { placement: n == 2 }),
            3..=67 => Ok(Self::Candidate(action_from(n - 3)?)),
            _ => Err(invalid("calculation subject")),
        }
    }
}
impl Ord for CalculationSubject {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.code().cmp(&other.code())
    }
}
impl PartialOrd for CalculationSubject {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
/// 現在の計算の対象を記録する。旧3形式の入力ハッシュは旧ファイル検査用にのみ残す。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Derivation {
    /// Current calculation, with no historical input references.
    Current {
        root: [u64; 2],
        subject: CalculationSubject,
    },
    #[doc(hidden)] // Old-file decoder only.
    Maximum {
        root: [u64; 2],
        policy: u8,
        inputs: [u8; 32],
    },
    #[doc(hidden)] // Old-file decoder only.
    Intersection {
        root: [u64; 2],
        action: u8,
        inputs: [[u8; 32]; 2],
    },
    #[doc(hidden)] // Old-file decoder only.
    Child {
        child: [u64; 2],
        input: [u8; 32],
        placement: bool,
    },
}
/// 値の得られ方。Derivedは現在値の計算結果であり、過去の入力をたどる履歴ではない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueAnalysis {
    Search {
        trace: CompletedSearch,
        projection: SearchProjection,
    },
    Derived {
        basis: Derivation,
        summary: QualitySummary,
    },
    Rules {
        root: [u64; 2],
    },
    Manual {
        depth: u8,
        selectivity: Selectivity,
    },
    /// The recorded quality survives; the lost search/derivation kind does not get invented.
    LegacyUnknown {
        depth: u8,
        selectivity: Selectivity,
        recorded_origin: ValueOrigin,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnalysisCategory {
    Midgame,
    Endgame,
    Derived,
    Rules,
    Manual,
    LegacyUnknown,
}
impl ValueAnalysis {
    pub fn category(self) -> AnalysisCategory {
        match self {
            Self::Search { trace, .. } => match trace.completed {
                SearchKind::Midgame { .. } => AnalysisCategory::Midgame,
                SearchKind::Endgame { .. } => AnalysisCategory::Endgame,
            },
            Self::Derived { .. } => AnalysisCategory::Derived,
            Self::Rules { .. } => AnalysisCategory::Rules,
            Self::Manual { .. } => AnalysisCategory::Manual,
            Self::LegacyUnknown { .. } => AnalysisCategory::LegacyUnknown,
        }
    }

    pub fn summary(self) -> QualitySummary {
        match self {
            Self::Search { trace, .. } => QualitySummary::search(trace),
            Self::Derived { summary, .. } => summary,
            Self::Rules { root } => QualitySummary {
                families: QualitySummary::RULES,
                horizon: Board::from_bits(root[0], root[1]).unwrap().empty_count(),
                ..QualitySummary::empty()
            },
            Self::Manual { depth, selectivity } => QualitySummary {
                families: QualitySummary::MANUAL,
                horizon: depth,
                legacy_depth: depth,
                legacy_sel: selectivity as u8,
                ..QualitySummary::empty()
            },
            Self::LegacyUnknown {
                depth, selectivity, ..
            } => QualitySummary::legacy(depth, selectivity),
        }
    }
    pub(crate) fn origin(self) -> ValueOrigin {
        match self {
            Self::Search { .. } => ValueOrigin::Search,
            Self::Derived { .. } => ValueOrigin::Derived,
            Self::Rules { .. } => ValueOrigin::Rules,
            Self::Manual { .. } => ValueOrigin::Manual,
            Self::LegacyUnknown {
                recorded_origin, ..
            } => recorded_origin,
        }
    }
    pub(crate) fn validate(self) -> Result<()> {
        match self {
            Self::Search { trace, .. } => trace.validate(),
            Self::Derived { basis, summary } => {
                summary.validate()?;
                let root = match basis {
                    Derivation::Maximum { root, .. } | Derivation::Intersection { root, .. } => {
                        root
                    }
                    Derivation::Child { child, .. } => child,
                    Derivation::Current { root, .. } => root,
                };
                let b =
                    Board::from_bits(root[0], root[1]).ok_or_else(|| invalid("derived root"))?;
                if b.empty_count() > 60 || Key::new(b).0.board() != b {
                    return Err(invalid("derived canonical root"));
                }
                Ok(())
            }
            Self::Rules { root } => {
                let b = Board::from_bits(root[0], root[1]).ok_or_else(|| invalid("rules root"))?;
                if b.empty_count() > 60 || Key::new(b).0.board() != b || !b.is_terminal() {
                    return Err(invalid("nonterminal or noncanonical rules value"));
                }
                Ok(())
            }
            Self::LegacyUnknown {
                recorded_origin: ValueOrigin::Manual,
                ..
            } => Err(invalid("manual value must retain manual provenance")),
            Self::Manual { depth, .. } | Self::LegacyUnknown { depth, .. } => {
                if depth > 60 {
                    Err(invalid("legacy depth"))
                } else {
                    Ok(())
                }
            }
        }
    }
}
/// Explicit external input. Missing search provenance remains unknown; callers
/// are responsible for the truth of imported final-score assertions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportedValue {
    pub score: i16,
    pub quality: AnalysisQuality,
    pub bound: Bound,
    pub identity: AnalysisIdentity,
    pub origin: ValueOrigin,
}
/// Validated evaluation returned by book queries. Accessors are read-only;
/// actual search conditions are available through analysis(), not an editable flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Assessment {
    claim: ValueClaim,
    analysis: ValueAnalysis,
    view: ImportedValue,
}
impl Assessment {
    #[inline]
    /// 点評価なら点数、区間なら上限（なければ下限）。点の確定はclaim/boundも確認する。
    pub fn score(self) -> i16 {
        self.view.score
    }
    #[inline]
    pub fn quality(self) -> AnalysisQuality {
        self.view.quality
    }
    #[inline]
    pub fn bound(self) -> Bound {
        self.view.bound
    }
    #[inline]
    pub fn identity(self) -> AnalysisIdentity {
        self.view.identity
    }
    #[inline]
    pub fn origin(self) -> ValueOrigin {
        self.view.origin
    }
    /// 外部から申告された値を検査する。探索方法が欠けていれば不明のまま保持する。
    /// 再探索して正しさを確かめる処理ではなく、確定石差の申告の真偽は入力側の責任。
    pub fn import(value: ImportedValue) -> Result<Self> {
        let claim = match value.bound {
            Bound::Exact if value.quality.solved => {
                ValueClaim::Certified(ScoreInterval::point(value.score)?)
            }
            Bound::ProvenLower => {
                ValueClaim::Certified(ScoreInterval::new(Some(value.score), None)?)
            }
            Bound::ProvenUpper => {
                ValueClaim::Certified(ScoreInterval::new(None, Some(value.score))?)
            }
            Bound::Exact => ValueClaim::Estimate {
                score: value.score,
                bound: EstimateBound::Point,
            },
            Bound::Lower => ValueClaim::Estimate {
                score: value.score,
                bound: EstimateBound::Lower,
            },
            Bound::Upper => ValueClaim::Estimate {
                score: value.score,
                bound: EstimateBound::Upper,
            },
        };
        if value.quality.solved
            && (value.bound != Bound::Exact
                || value.quality.selectivity != Selectivity::Full
                || value.origin == ValueOrigin::Manual)
            || matches!(value.bound, Bound::ProvenUpper | Bound::ProvenLower)
                && (value.quality.solved
                    || value.quality.selectivity != Selectivity::Full
                    || value.origin == ValueOrigin::Manual)
        {
            return Err(invalid("legacy certificate metadata"));
        }
        let analysis = if value.origin == ValueOrigin::Manual {
            ValueAnalysis::Manual {
                depth: value.quality.depth,
                selectivity: value.quality.selectivity,
            }
        } else {
            ValueAnalysis::LegacyUnknown {
                depth: value.quality.depth,
                selectivity: value.quality.selectivity,
                recorded_origin: value.origin,
            }
        };
        Self::new(claim, analysis, value.identity)
    }
    /// Records only the engine's last completed iteration. The requested target
    /// remains separate even if the search stopped before reaching it.
    pub fn from_search_result(
        board: Board,
        requested: RequestedAnalysis,
        planned: SearchKind,
        result: &deft_reversi_engine::SearchResult,
        identity: AnalysisIdentity,
    ) -> Result<Self> {
        if Key::new(board).0.board() != board {
            return Err(invalid("search result requires canonical root coordinates"));
        }
        let score = i16::try_from(
            result
                .score
                .ok_or_else(|| invalid("no completed search iteration"))?,
        )
        .map_err(|_| invalid("search score range"))?;
        let completed = SearchKind::from_result(
            result
                .kind
                .ok_or_else(|| invalid("no completed search iteration"))?,
        );
        let claim = if result.is_subset_solved() {
            ValueClaim::Certified(ScoreInterval::point(score)?)
        } else {
            ValueClaim::Estimate {
                score,
                bound: EstimateBound::Point,
            }
        };
        Self::new(
            claim,
            ValueAnalysis::Search {
                trace: CompletedSearch {
                    requested,
                    planned,
                    completed,
                    root: [board.player_bits(), board.opponent_bits()],
                    moves: result.root_moves,
                    best: result.best_move.map_or(65, action_code),
                    result_score: score,
                    run: None,
                    nodes: result.stats.nodes,
                    elapsed_micros: result.elapsed.as_micros().min(u128::from(u64::MAX)) as u64,
                },
                projection: SearchProjection::Maximum,
            },
            identity,
        )
    }
    pub(crate) fn validate_candidate(self, parent: Key, action: Action) -> Result<()> {
        self.validate_at(parent.board())?;
        match self.analysis {
            ValueAnalysis::Search { trace, projection } => {
                let code = action_code(action);
                if trace.root != [parent.0, parent.1]
                    || projection == SearchProjection::Maximum
                    || code < 64 && trace.moves & (1u64 << code) == 0
                    || (projection == SearchProjection::BestMove) != (trace.best == code)
                {
                    return Err(invalid("candidate search subject"));
                }
            }
            ValueAnalysis::Derived {
                basis: Derivation::Current { root, subject },
                ..
            } => match subject {
                CalculationSubject::Position => {
                    return Err(invalid("position value used as candidate"));
                }
                CalculationSubject::FromChild { placement } => {
                    let child = Key::new(
                        parent
                            .board()
                            .play(action)
                            .map_err(|_| invalid("derived candidate"))?,
                    )
                    .0;
                    if root != [child.0, child.1] || placement != matches!(action, Action::Place(_))
                    {
                        return Err(invalid("derived candidate subject"));
                    }
                }
                CalculationSubject::Candidate(a) => {
                    if root != [parent.0, parent.1] || a != action {
                        return Err(invalid("retained bounds subject"));
                    }
                }
            },
            ValueAnalysis::Derived {
                basis:
                    Derivation::Child {
                        child, placement, ..
                    },
                ..
            } => {
                let expected = Key::new(
                    parent
                        .board()
                        .play(action)
                        .map_err(|_| invalid("derived candidate"))?,
                )
                .0;
                if child != [expected.0, expected.1]
                    || placement != matches!(action, Action::Place(_))
                {
                    return Err(invalid("derived candidate subject"));
                }
            }
            ValueAnalysis::Derived {
                basis: Derivation::Maximum { .. },
                ..
            }
            | ValueAnalysis::Rules { .. } => {
                return Err(invalid("position value used as candidate"));
            }
            ValueAnalysis::Derived {
                basis:
                    Derivation::Intersection {
                        root, action: a, ..
                    },
                ..
            } if root != [parent.0, parent.1] || a != action_code(action) => {
                return Err(invalid("intersection subject"));
            }
            _ => {}
        }
        Ok(())
    }
    pub fn claim(self) -> ValueClaim {
        self.claim
    }
    pub fn analysis(self) -> ValueAnalysis {
        self.analysis
    }
    pub(crate) fn content_id(self) -> [u8; 32] {
        super::value_codec::fingerprint(&self).expect("valid legacy fingerprint")
    }
    pub fn as_imported(self) -> ImportedValue {
        self.view
    }
    // ValueStore内で検査済みの情報を読取用の型へ展開する経路。
    // 外部入力はこの経路を通さず、new/import/from_search_resultで検査する。
    pub(crate) fn stored(
        claim: ValueClaim,
        analysis: ValueAnalysis,
        identity: AnalysisIdentity,
    ) -> Self {
        let (score, bound, solved) = match claim {
            ValueClaim::Estimate { score, bound } => (
                score,
                match bound {
                    EstimateBound::Point => Bound::Exact,
                    EstimateBound::Lower => Bound::Lower,
                    EstimateBound::Upper => Bound::Upper,
                },
                false,
            ),
            ValueClaim::Certified(v) => {
                if let Some(score) = v.exact() {
                    (score, Bound::Exact, true)
                } else if let Some(score) = v.upper() {
                    (score, Bound::ProvenUpper, false)
                } else {
                    (v.lower().unwrap(), Bound::ProvenLower, false)
                }
            }
        };
        let q = analysis.summary();
        Self {
            claim,
            analysis,
            view: ImportedValue {
                score,
                bound,
                identity,
                origin: analysis.origin(),
                quality: AnalysisQuality {
                    depth: q.horizon,
                    selectivity: if matches!(claim, ValueClaim::Certified(_)) {
                        Selectivity::Full
                    } else {
                        q.minimum_selectivity()
                    },
                    solved,
                },
            },
        }
    }
    // claimとanalysisの組合せを検査する中心。例えば選択的終盤探索は
    // Certifiedにできず、他候補への投影は必ず上限になる。
    pub(crate) fn new(
        claim: ValueClaim,
        analysis: ValueAnalysis,
        identity: AnalysisIdentity,
    ) -> Result<Self> {
        analysis.validate()?;
        let (score, bound, solved) = match claim {
            ValueClaim::Estimate { score, bound } => (
                score,
                match bound {
                    EstimateBound::Point => Bound::Exact,
                    EstimateBound::Lower => Bound::Lower,
                    EstimateBound::Upper => Bound::Upper,
                },
                false,
            ),
            ValueClaim::Certified(v) => {
                if let Some(score) = v.exact() {
                    (score, Bound::Exact, true)
                } else if let Some(score) = v.upper() {
                    (score, Bound::ProvenUpper, false)
                } else {
                    (v.lower().unwrap(), Bound::ProvenLower, false)
                }
            }
        };
        if !(-64..=64).contains(&score)
            || matches!(analysis, ValueAnalysis::Manual { .. })
                && matches!(claim, ValueClaim::Certified(_))
        {
            return Err(invalid("value claim"));
        }
        if matches!(claim, ValueClaim::Certified(_))
            && matches!(analysis,ValueAnalysis::LegacyUnknown{selectivity,..} if selectivity!=Selectivity::Full)
        {
            return Err(invalid("legacy selective estimate cannot certify a score"));
        }
        if matches!(claim, ValueClaim::Certified(_))
            && matches!(analysis,ValueAnalysis::Search{trace,..} if trace.completed!=(SearchKind::Endgame{selectivity:Selectivity::Full}))
        {
            return Err(invalid("estimated search cannot certify final score"));
        }
        if let ValueAnalysis::Search { trace, projection } = analysis {
            let expected = if trace.completed
                == (SearchKind::Endgame {
                    selectivity: Selectivity::Full,
                }) {
                ValueClaim::Certified(if projection == SearchProjection::OtherMoveUpper {
                    ScoreInterval::new(None, Some(trace.result_score))?
                } else {
                    ScoreInterval::point(trace.result_score)?
                })
            } else {
                ValueClaim::Estimate {
                    score: trace.result_score,
                    bound: if projection == SearchProjection::OtherMoveUpper {
                        EstimateBound::Upper
                    } else {
                        EstimateBound::Point
                    },
                }
            };
            if claim != expected {
                return Err(invalid("search result and claim disagree"));
            }
        }
        if let ValueAnalysis::Rules { root } = analysis
            && claim
                != ValueClaim::Certified(ScoreInterval::point(
                    Board::from_bits(root[0], root[1]).unwrap().final_score() as i16,
                )?)
        {
            return Err(invalid("rules score"));
        }
        let summary = analysis.summary();
        let quality = AnalysisQuality {
            depth: summary.horizon,
            selectivity: if matches!(claim, ValueClaim::Certified(_)) {
                Selectivity::Full
            } else {
                summary.minimum_selectivity()
            },
            solved,
        };
        let result = Self {
            claim,
            analysis,
            view: ImportedValue {
                score,
                quality,
                bound,
                identity,
                origin: analysis.origin(),
            },
        };
        Ok(result)
    }
    pub(crate) fn proven(self) -> bool {
        matches!(self.claim, ValueClaim::Certified(_))
    }
    pub(crate) fn validate(self) -> Result<()> {
        self.analysis.validate()?;
        Ok(())
    }
    pub(crate) fn validate_at(self, board: Board) -> Result<()> {
        self.validate()?;
        if self.proven() && self.quality().depth < board.empty_count() {
            return Err(invalid("certificate horizon"));
        }
        Ok(())
    }
    pub(crate) fn whole_position(self) -> bool {
        match self.analysis {
            ValueAnalysis::Rules { .. }
            | ValueAnalysis::Derived {
                basis:
                    Derivation::Maximum { .. }
                    | Derivation::Current {
                        subject: CalculationSubject::Position,
                        ..
                    },
                ..
            } => true,
            ValueAnalysis::Search {
                trace,
                projection: SearchProjection::Maximum,
            } => trace.board().is_some_and(|b| {
                trace.moves
                    == b.legal_moves()
                        .fold(0, |mask, s| mask | (1u64 << s.index()))
            }),
            _ => false,
        }
    }
    pub(crate) fn child_to_parent(self, placement: bool) -> Self {
        self.try_child_to_parent(placement)
            .expect("validated whole child value")
    }
    // 子の一部の手の最大値を反転すると親の手の値を誤るため、局面全体の値だけ許す。
    // 区間は[l,u]→[-u,-l]へ変換し、パスでも符号は反転する。
    pub(crate) fn try_child_to_parent(self, placement: bool) -> Result<Self> {
        if !self.whole_position() && !matches!(self.analysis, ValueAnalysis::LegacyUnknown { .. }) {
            return Err(invalid(
                "whole-position value required for child conversion",
            ));
        }
        let child = match self.analysis {
            ValueAnalysis::Derived {
                basis:
                    Derivation::Maximum { root, .. }
                    | Derivation::Current {
                        root,
                        subject: CalculationSubject::Position,
                    },
                ..
            }
            | ValueAnalysis::Rules { root } => root,
            ValueAnalysis::Search { trace, .. } => trace.root,
            _ => [0, 0],
        };
        // Legacy-only callers have no recoverable child anchor. Preserve that
        // unknown status instead of manufacturing a derivation reference.
        let analysis = if child == [0, 0] {
            ValueAnalysis::LegacyUnknown {
                depth: (self.quality().depth + u8::from(placement)).min(60),
                selectivity: self.quality().selectivity,
                recorded_origin: self.origin(),
            }
        } else {
            ValueAnalysis::Derived {
                basis: Derivation::Current {
                    root: child,
                    subject: CalculationSubject::FromChild { placement },
                },
                summary: self.analysis.summary().shifted(placement),
            }
        };
        Self::new(self.claim.negated(), analysis, self.identity())
    }
    /// Project a completed search maximum onto its best candidate or the
    /// remaining candidates. Other subjects cannot be silently reinterpreted.
    pub fn project(self, projection: SearchProjection) -> Result<Self> {
        if !matches!(
            self.analysis,
            ValueAnalysis::Search {
                projection: SearchProjection::Maximum,
                ..
            }
        ) {
            return Err(invalid("search maximum required for projection"));
        }
        match projection {
            SearchProjection::Maximum => Ok(self),
            SearchProjection::BestMove => self.project_subset(false),
            SearchProjection::OtherMoveUpper => self.project_subset(true),
        }
    }
    pub(super) fn project_subset(self, upper: bool) -> Result<Self> {
        if matches!(self.analysis,ValueAnalysis::Search{trace,..} if trace.best==65) {
            return Err(invalid("terminal maximum has no candidate projection"));
        }
        if self.bound() != Bound::Exact
            || !matches!(
                self.analysis,
                ValueAnalysis::Search {
                    projection: SearchProjection::Maximum,
                    ..
                } | ValueAnalysis::LegacyUnknown { .. }
            )
        {
            return Err(invalid("point-valued subset maximum required"));
        }
        let analysis = match self.analysis {
            ValueAnalysis::Search { trace, .. } => ValueAnalysis::Search {
                trace,
                projection: if upper {
                    SearchProjection::OtherMoveUpper
                } else {
                    SearchProjection::BestMove
                },
            },
            other => other,
        };
        let claim = if upper {
            if self.proven() {
                ValueClaim::Certified(ScoreInterval::new(None, Some(self.score()))?)
            } else {
                ValueClaim::Estimate {
                    score: self.score(),
                    bound: EstimateBound::Upper,
                }
            }
        } else {
            self.claim
        };
        Self::new(claim, analysis, self.identity())
    }
    pub(crate) fn rules(board: Board) -> Self {
        let board = Key::new(board).0.board();
        Self::new(
            ValueClaim::Certified(ScoreInterval::point(board.final_score() as i16).unwrap()),
            ValueAnalysis::Rules {
                root: [board.player_bits(), board.opponent_bits()],
            },
            [0; 32],
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_codec_rejects_manual_and_selective_legacy_certificates() {
        let value = Assessment::import(ImportedValue {
            score: 0,
            quality: AnalysisQuality {
                depth: 60,
                selectivity: Selectivity::Full,
                solved: true,
            },
            bound: Bound::Exact,
            identity: [7; 32],
            origin: ValueOrigin::Imported,
        })
        .unwrap();
        assert!(value.project(SearchProjection::OtherMoveUpper).is_err());
        assert!(
            Assessment::rules(Board::from_bits(u64::MAX, 0).unwrap())
                .project(SearchProjection::OtherMoveUpper)
                .is_err()
        );
        let lower = Assessment::import(ImportedValue {
            bound: Bound::ProvenLower,
            quality: AnalysisQuality {
                solved: false,
                ..value.quality()
            },
            ..value.as_imported()
        })
        .unwrap();
        assert!(lower.project_subset(true).is_err());
        let kind = SearchKind::Endgame {
            selectivity: Selectivity::Full,
        };
        let terminal = Assessment::new(
            ValueClaim::Certified(ScoreInterval::point(64).unwrap()),
            ValueAnalysis::Search {
                trace: CompletedSearch {
                    requested: RequestedAnalysis::External,
                    planned: kind,
                    completed: kind,
                    root: [u64::MAX, 0],
                    moves: 0,
                    best: 65,
                    result_score: 64,
                    run: None,
                    nodes: 0,
                    elapsed_micros: 0,
                },
                projection: SearchProjection::Maximum,
            },
            [7; 32],
        )
        .unwrap();
        assert!(terminal.project(SearchProjection::BestMove).is_err());
        assert!(terminal.project(SearchProjection::Maximum).is_ok());
        let mut bytes = Vec::new();
        value_codec::write(&mut bytes, &value).unwrap();
        // Certified interval(5), identity(32), legacy tag(1), depth(1).
        assert_eq!(bytes[39], Selectivity::Full as u8);
        let original = bytes.clone();
        bytes[39] = Selectivity::Percent85 as u8;
        assert!(value_codec::read(&mut bytes.as_slice()).is_err());
        let mut bytes = original;
        bytes[40] = 1;
        assert!(value_codec::read(&mut bytes.as_slice()).is_err());
    }
    #[test]
    fn quality_summary_rejects_partial_fields_without_their_family() {
        let rules = QualitySummary {
            families: QualitySummary::RULES,
            ..QualitySummary::empty()
        };
        assert!(rules.validate().is_ok());
        for q in [
            QualitySummary {
                mid_depth: 8,
                ..rules
            },
            QualitySummary {
                mid_sel: 6,
                ..rules
            },
            QualitySummary {
                legacy_depth: 8,
                ..rules
            },
            QualitySummary {
                legacy_sel: 6,
                ..rules
            },
        ] {
            assert!(q.validate().is_err());
        }
    }
}
