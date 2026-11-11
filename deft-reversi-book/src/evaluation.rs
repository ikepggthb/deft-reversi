//! 現在の評価。点数・探索条件・出所を直接読む。
//! 旧形式の「導出」「投影」などの表現はlegacy内で検査・変換し、ここへ持ち込まない。
use super::*;

/// 値の出所。計算した局面や手は直接記録し、分類を何段もたどらせない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationSource {
    /// 指定した手集合の最大値。全合法手を含むとは限らない。
    Search(CompletedSearch),
    /// 同じ探索から得た最良手の点評価、または他の手の上限。点数の種類で区別する。
    SearchMove(CompletedSearch),
    Position {
        root: [u64; 2],
        conditions: SearchConditions,
    },
    Child {
        root: [u64; 2],
        placement: bool,
        conditions: SearchConditions,
    },
    Move {
        root: [u64; 2],
        action: Action,
        conditions: SearchConditions,
    },
    Terminal {
        root: [u64; 2],
    },
    /// 深さ等は旧入力の表示情報を保持するための欄。探索の実績ではない。
    Manual {
        depth: u8,
        selectivity: Selectivity,
    },
    /// 探索方法が残っていない入力。深さから中盤・終盤を推測しない。
    Imported {
        depth: u8,
        selectivity: Selectivity,
        recorded_origin: legacy::ValueOrigin,
    },
}

impl Ord for EvaluationSource {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use EvaluationSource::*;
        let rank = |s: &Self| match s {
            Search(_) => 0,
            SearchMove(_) => 1,
            Position { .. } => 2,
            Child { .. } => 3,
            Move { .. } => 4,
            Terminal { .. } => 5,
            Manual { .. } => 6,
            Imported { .. } => 7,
        };
        rank(self)
            .cmp(&rank(other))
            .then_with(|| match (*self, *other) {
                (Search(a), Search(b)) | (SearchMove(a), SearchMove(b)) => a.cmp(&b),
                (
                    Position {
                        root: a,
                        conditions: q,
                    },
                    Position {
                        root: b,
                        conditions: r,
                    },
                ) => (a, q).cmp(&(b, r)),
                (
                    Child {
                        root: a,
                        placement: p,
                        conditions: q,
                    },
                    Child {
                        root: b,
                        placement: s,
                        conditions: r,
                    },
                ) => (a, p, q).cmp(&(b, s, r)),
                (
                    Move {
                        root: a,
                        action: p,
                        conditions: q,
                    },
                    Move {
                        root: b,
                        action: s,
                        conditions: r,
                    },
                ) => (a, action_code(p), q).cmp(&(b, action_code(s), r)),
                (Terminal { root: a }, Terminal { root: b }) => a.cmp(&b),
                (
                    Manual {
                        depth: a,
                        selectivity: p,
                    },
                    Manual {
                        depth: b,
                        selectivity: q,
                    },
                ) => (a, p).cmp(&(b, q)),
                (
                    Imported {
                        depth: a,
                        selectivity: p,
                        recorded_origin: o,
                    },
                    Imported {
                        depth: b,
                        selectivity: q,
                        recorded_origin: r,
                    },
                ) => (a, p, o).cmp(&(b, q, r)),
                _ => unreachable!("equal source ranks"),
            })
    }
}
impl PartialOrd for EvaluationSource {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
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

impl EvaluationSource {
    pub fn conditions(self) -> SearchConditions {
        match self {
            Self::Search(v) | Self::SearchMove(v) => SearchConditions::search(v),
            Self::Position { conditions, .. }
            | Self::Child { conditions, .. }
            | Self::Move { conditions, .. } => conditions,
            Self::Terminal { root } => SearchConditions {
                families: SearchConditions::RULES,
                horizon: (64 - (root[0] | root[1]).count_ones()) as u8,
                ..SearchConditions::empty()
            },
            Self::Manual { depth, selectivity } => SearchConditions {
                families: SearchConditions::MANUAL,
                horizon: depth,
                legacy_depth: depth,
                legacy_sel: selectivity as u8,
                ..SearchConditions::empty()
            },
            Self::Imported {
                depth, selectivity, ..
            } => SearchConditions::legacy(depth, selectivity),
        }
    }
    pub fn category(self) -> AnalysisCategory {
        match self {
            Self::Search(v) | Self::SearchMove(v) => match v.completed {
                SearchKind::Midgame { .. } => AnalysisCategory::Midgame,
                SearchKind::Endgame { .. } => AnalysisCategory::Endgame,
            },
            Self::Position { .. } | Self::Child { .. } | Self::Move { .. } => {
                AnalysisCategory::Derived
            }
            Self::Terminal { .. } => AnalysisCategory::Rules,
            Self::Manual { .. } => AnalysisCategory::Manual,
            Self::Imported { .. } => AnalysisCategory::LegacyUnknown,
        }
    }
    pub(crate) fn validate(self) -> Result<()> {
        match self {
            Self::Search(v) | Self::SearchMove(v) => v.validate(),
            Self::Position { root, conditions }
            | Self::Child {
                root, conditions, ..
            }
            | Self::Move {
                root, conditions, ..
            } => {
                conditions.validate()?;
                let b = Board::from_bits(root[0], root[1])
                    .ok_or_else(|| invalid("evaluation board"))?;
                if b.empty_count() > 60 || Key::new(b).0.board() != b {
                    return Err(invalid("evaluation canonical board"));
                }
                Ok(())
            }
            Self::Terminal { root } => {
                let b =
                    Board::from_bits(root[0], root[1]).ok_or_else(|| invalid("terminal board"))?;
                if !b.is_terminal() || b.empty_count() > 60 || Key::new(b).0.board() != b {
                    return Err(invalid("terminal board"));
                }
                Ok(())
            }
            Self::Manual { depth, .. } | Self::Imported { depth, .. } if depth > 60 => {
                Err(invalid("recorded depth"))
            }
            Self::Imported {
                recorded_origin: legacy::ValueOrigin::Manual,
                ..
            } => Err(invalid("manual source required")),
            _ => Ok(()),
        }
    }
}

/// 一つの評価の唯一の表現。簡易表示用に点数・深さ・確定フラグを重複保存しない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Evaluation {
    result: Score,
    source: EvaluationSource,
    identity: AnalysisIdentity,
}
impl Evaluation {
    pub fn score(self) -> i16 {
        self.result.value()
    }
    pub fn result(self) -> Score {
        self.result
    }
    pub fn source(self) -> EvaluationSource {
        self.source
    }
    pub fn identity(self) -> AnalysisIdentity {
        self.identity
    }
    pub fn bound(self) -> Bound {
        self.result.bound()
    }
    pub fn is_solved(self) -> bool {
        self.result.is_solved()
    }
    pub fn conditions(self) -> SearchConditions {
        self.source.conditions()
    }
    pub fn depth(self) -> u8 {
        self.conditions().depth()
    }
    pub fn selectivity(self) -> Selectivity {
        if self.is_final() {
            Selectivity::Full
        } else {
            self.conditions().minimum_selectivity()
        }
    }
    pub fn is_manual(self) -> bool {
        matches!(self.source, EvaluationSource::Manual { .. })
    }
    pub fn is_calculated(self) -> bool {
        matches!(
            self.source,
            EvaluationSource::Position { .. }
                | EvaluationSource::Move { .. }
                | EvaluationSource::Child { .. }
        )
    }
    pub fn import(value: legacy::ImportedValue) -> Result<Self> {
        Ok(legacy::Assessment::import(value)?.into())
    }
    pub fn manual(score: i16, identity: AnalysisIdentity) -> Result<Self> {
        Self::new(
            Score::point(score),
            EvaluationSource::Manual {
                depth: 0,
                selectivity: Selectivity::Full,
            },
            identity,
        )
    }
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
        let score = result
            .score
            .ok_or_else(|| invalid("no completed search iteration"))?;
        let score = i16::try_from(score).map_err(|_| invalid("search score range"))?;
        let completed = SearchKind::from_result(
            result
                .kind
                .ok_or_else(|| invalid("no completed search iteration"))?,
        );
        let result_score = if result.is_subset_solved() {
            Score::Final(ScoreInterval::point(score)?)
        } else {
            Score::point(score)
        };
        Self::new(
            result_score,
            EvaluationSource::Search(CompletedSearch {
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
            }),
            identity,
        )
    }
    pub fn best_move_value(self) -> Result<Self> {
        self.move_value(false)
    }
    pub fn other_moves_upper_bound(self) -> Result<Self> {
        self.move_value(true)
    }
    pub(crate) fn move_value(self, upper: bool) -> Result<Self> {
        let source = match self.source {
            EvaluationSource::Search(trace) if trace.best != 65 => {
                EvaluationSource::SearchMove(trace)
            }
            EvaluationSource::Imported { .. } if self.bound() == Bound::Point => self.source,
            _ => return Err(invalid("search maximum with a candidate required")),
        };
        let result = if !upper {
            self.result
        } else if self.is_final() {
            Score::Final(ScoreInterval::new(None, Some(self.score()))?)
        } else {
            Score::Estimate {
                score: self.score(),
                bound: Bound::Upper,
            }
        };
        Self::new(result, source, self.identity)
    }
    pub(crate) fn stored(
        result: Score,
        source: EvaluationSource,
        identity: AnalysisIdentity,
    ) -> Self {
        Self {
            result,
            source,
            identity,
        }
    }
    pub(crate) fn new(
        result: Score,
        source: EvaluationSource,
        identity: AnalysisIdentity,
    ) -> Result<Self> {
        source.validate()?;
        if !(-64..=64).contains(&result.value())
            || matches!(source, EvaluationSource::Manual { .. }) && result.final_bounds().is_some()
        {
            return Err(invalid("evaluation score"));
        }
        if result.final_bounds().is_some()
            && matches!(source, EvaluationSource::Imported { selectivity, .. } if selectivity != Selectivity::Full)
        {
            return Err(invalid("selective import cannot certify final score"));
        }
        if let EvaluationSource::Search(trace) | EvaluationSource::SearchMove(trace) = source {
            let upper =
                matches!(source, EvaluationSource::SearchMove(_)) && result.bound() == Bound::Upper;
            let expected = if trace.completed
                == (SearchKind::Endgame {
                    selectivity: Selectivity::Full,
                }) {
                Score::Final(if upper {
                    ScoreInterval::new(None, Some(trace.result_score))?
                } else {
                    ScoreInterval::point(trace.result_score)?
                })
            } else {
                Score::Estimate {
                    score: trace.result_score,
                    bound: if upper { Bound::Upper } else { Bound::Point },
                }
            };
            if result != expected {
                return Err(invalid("search result and score disagree"));
            }
        }
        if let EvaluationSource::Terminal { root } = source {
            let expected = Score::Final(ScoreInterval::point(
                Board::from_bits(root[0], root[1]).unwrap().final_score() as i16,
            )?);
            if result != expected {
                return Err(invalid("terminal score"));
            }
        }
        Ok(Self::stored(result, source, identity))
    }
    pub fn is_final(self) -> bool {
        self.result.final_bounds().is_some()
    }
    pub(crate) fn validate(self) -> Result<()> {
        Self::new(self.result, self.source, self.identity).map(|_| ())
    }
    pub(crate) fn validate_at(self, board: Board) -> Result<()> {
        self.validate()?;
        if self.is_final() && self.depth() < board.empty_count() {
            return Err(invalid("certificate horizon"));
        }
        Ok(())
    }
    pub(crate) fn validate_candidate(self, parent: Key, action: Action) -> Result<()> {
        self.validate_at(parent.board())?;
        match self.source {
            EvaluationSource::SearchMove(trace) => {
                let code = action_code(action);
                if trace.root != [parent.0, parent.1]
                    || code < 64 && trace.moves & (1u64 << code) == 0
                    || (self.bound() == Bound::Point) != (trace.best == code)
                {
                    return Err(invalid("candidate search subject"));
                }
            }
            EvaluationSource::Child {
                root, placement, ..
            } => {
                let child = Key::new(
                    parent
                        .board()
                        .play(action)
                        .map_err(|_| invalid("derived candidate"))?,
                )
                .0;
                if root != [child.0, child.1] || placement != matches!(action, Action::Place(_)) {
                    return Err(invalid("child evaluation subject"));
                }
            }
            EvaluationSource::Move {
                root, action: a, ..
            } if root != [parent.0, parent.1] || a != action => {
                return Err(invalid("move evaluation subject"));
            }
            EvaluationSource::Search(_)
            | EvaluationSource::Position { .. }
            | EvaluationSource::Terminal { .. } => {
                return Err(invalid("position value used as candidate"));
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn whole_position(self) -> bool {
        match self.source {
            EvaluationSource::Position { .. } | EvaluationSource::Terminal { .. } => true,
            EvaluationSource::Search(v) => v.board().is_some_and(|b| {
                v.moves == b.legal_moves().fold(0, |m, s| m | (1u64 << s.index()))
            }),
            _ => false,
        }
    }
    pub(crate) fn child_to_parent(self, placement: bool) -> Self {
        self.try_child_to_parent(placement)
            .expect("validated whole child value")
    }
    pub(crate) fn try_child_to_parent(self, placement: bool) -> Result<Self> {
        let source = match self.source {
            EvaluationSource::Position { root, .. } | EvaluationSource::Terminal { root } => {
                EvaluationSource::Child {
                    root,
                    placement,
                    conditions: self.conditions().shifted(placement),
                }
            }
            EvaluationSource::Search(v) if self.whole_position() => EvaluationSource::Child {
                root: v.root,
                placement,
                conditions: self.conditions().shifted(placement),
            },
            EvaluationSource::Imported {
                depth,
                selectivity,
                recorded_origin,
            } => EvaluationSource::Imported {
                depth: (depth + u8::from(placement)).min(60),
                selectivity,
                recorded_origin,
            },
            _ => {
                return Err(invalid(
                    "whole-position value required for child conversion",
                ));
            }
        };
        Self::new(self.result.negated(), source, self.identity)
    }
    pub(crate) fn meets(self, kind: SearchKind) -> bool {
        if self.is_final() {
            return true;
        }
        let q = self.conditions();
        if q.has_imported() || q.has_manual() {
            return false;
        }
        match kind {
            SearchKind::Midgame { depth, selectivity } => {
                (!q.has_midgame() || q.mid_depth >= depth && q.mid_sel >= selectivity as u8)
                    && (!q.has_endgame() || q.end_sel >= selectivity as u8)
            }
            SearchKind::Endgame { selectivity } => {
                !q.has_midgame()
                    && q.has_endgame()
                    && q.end_sel >= selectivity as u8
                    && selectivity != Selectivity::Full
            }
        }
    }
    pub(crate) fn rules(board: Board) -> Self {
        let b = Key::new(board).0.board();
        Self::new(
            Score::Final(ScoreInterval::point(b.final_score() as i16).unwrap()),
            EvaluationSource::Terminal {
                root: [b.player_bits(), b.opponent_bits()],
            },
            [0; 32],
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{EngineOptions, Goal, Model, PatternModel, SearchRequest};

    #[test]
    fn a_searched_move_or_subset_cannot_be_used_as_a_whole_child() {
        let board = Board::new().canonical();
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let kind = SearchKind::Midgame {
            depth: 2,
            selectivity: Selectivity::Full,
        };
        let request = SearchRequest::new(Goal::Depth(2));
        let whole = Evaluation::from_search_result(
            board,
            RequestedAnalysis::External,
            kind,
            &engine.search(board, &request).unwrap(),
            [3; 32],
        )
        .unwrap();
        assert!(whole.try_child_to_parent(true).is_ok());
        assert!(
            whole
                .best_move_value()
                .unwrap()
                .try_child_to_parent(true)
                .is_err()
        );
        let square = board.legal_moves().next().unwrap();
        let subset = Evaluation::from_search_result(
            board,
            RequestedAnalysis::External,
            kind,
            &engine
                .search(board, &request.root_moves(1u64 << square.index()))
                .unwrap(),
            [3; 32],
        )
        .unwrap();
        assert!(subset.try_child_to_parent(true).is_err());
    }

    #[test]
    fn full_midgame_and_selective_endgame_do_not_certify_final_scores() {
        let board = Board::new().canonical();
        for completed in [
            SearchKind::Midgame {
                depth: 60,
                selectivity: Selectivity::Full,
            },
            SearchKind::Endgame {
                selectivity: Selectivity::Percent85,
            },
        ] {
            let record = CompletedSearch {
                requested: RequestedAnalysis::External,
                planned: completed,
                completed,
                root: [board.player_bits(), board.opponent_bits()],
                moves: board.legal_moves().fold(0, |m, s| m | (1u64 << s.index())),
                best: board.legal_moves().next().unwrap().index(),
                result_score: 4,
                run: None,
                nodes: 1,
                elapsed_micros: 1,
            };
            assert!(
                Evaluation::new(
                    Score::Final(ScoreInterval::point(4).unwrap()),
                    EvaluationSource::Search(record),
                    [3; 32]
                )
                .is_err()
            );
            let estimate =
                Evaluation::new(Score::point(4), EvaluationSource::Search(record), [3; 32])
                    .unwrap();
            assert!(!estimate.is_final());
            assert!(!estimate.other_moves_upper_bound().unwrap().is_solved());
        }
    }
}
