//! 一回の探索で要求・予定・完了した条件と、探索対象の記録。
use super::*;
use deft_reversi_engine::ResultKind;
/// 探索の種類。深さが空きマス数と同じでも、MidgameをEndgameへ読み替えない。
/// Endgameも選択率がFullでなければ最終石差の確定にはならない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchKind {
    Midgame { depth: u8, selectivity: Selectivity },
    Endgame { selectivity: Selectivity },
}
impl SearchKind {
    pub fn selectivity(self) -> Selectivity {
        match self {
            Self::Midgame { selectivity, .. } | Self::Endgame { selectivity } => selectivity,
        }
    }
    pub fn from_result(kind: ResultKind) -> Self {
        match kind {
            ResultKind::Depth { depth, selectivity } => Self::Midgame { depth, selectivity },
            ResultKind::Endgame { selectivity } => Self::Endgame { selectivity },
        }
    }
    pub fn horizon(self, empties: u8) -> u8 {
        match self {
            Self::Midgame { depth, .. } => depth,
            Self::Endgame { .. } => empties,
        }
    }
    pub(crate) fn validate(self) -> Result<()> {
        if matches!(self,Self::Midgame{depth,..} if depth>60) {
            return Err(invalid("search depth"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestedAnalysis {
    /// Levels are resolved after the candidate move, then shifted to its parent.
    ChildLevel(u8),
    ParentDepth {
        depth: u8,
        selectivity: Selectivity,
    },
    Endgame {
        selectivity: Selectivity,
    },
    External,
}
/// 一回の探索の記録。requested=利用者の指定、planned=実行予定、completed=完了条件。
/// 途中停止した外部結果では、予定より浅い完了済み反復だけが記録される場合がある。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompletedSearch {
    pub requested: RequestedAnalysis,
    pub planned: SearchKind,
    pub completed: SearchKind,
    /// Canonical search root; not a guessed parent horizon.
    pub root: [u64; 2],
    /// Root square mask. Zero is valid for a forced pass or terminal root.
    pub moves: u64,
    /// 0..=63はマス、64はパス、65は終局で着手なし。
    pub best: u8,
    pub result_score: i16,
    pub run: Option<u32>,
    pub nodes: u64,
    pub elapsed_micros: u64,
}
impl CompletedSearch {
    pub fn board(self) -> Option<Board> {
        Board::from_bits(self.root[0], self.root[1])
    }
    pub(crate) fn validate(self) -> Result<()> {
        let b =
            Board::from_bits(self.root[0], self.root[1]).ok_or_else(|| invalid("search root"))?;
        self.planned.validate()?;
        self.completed.validate()?;
        if b.empty_count() > 60
            || Key::new(b).0.board() != b
            || self.moves & !b.legal_moves().fold(0u64, |m, s| m | (1u64 << s.index())) != 0
            || (b.legal_moves().fold(0u64, |m, s| m | (1u64 << s.index())) != 0 && self.moves == 0)
            || !(-64..=64).contains(&self.result_score)
            || (b.legal_moves().fold(0u64, |m, s| m | (1u64 << s.index())) != 0
                && (self.best >= 64 || self.moves & (1u64 << self.best) == 0))
            || (b.must_pass() && self.best != 64)
            || (b.is_terminal() && self.best != 65)
            || matches!(self.requested,RequestedAnalysis::ChildLevel(n) if !(1..=60).contains(&n))
            || matches!(self.requested,RequestedAnalysis::ParentDepth{depth,..} if depth>60)
            || self.completed.horizon(b.empty_count()) > b.empty_count()
            || self.planned.horizon(b.empty_count()) > b.empty_count()
            || self.completed.horizon(b.empty_count()) > self.planned.horizon(b.empty_count())
            || matches!((self.planned,self.completed),(SearchKind::Midgame{selectivity:a,..},SearchKind::Midgame{selectivity:b,..}) if a!=b)
            || matches!(
                (self.planned, self.completed),
                (SearchKind::Midgame { .. }, SearchKind::Endgame { .. })
            )
        {
            return Err(invalid("completed search metadata"));
        }
        Ok(())
    }
}
