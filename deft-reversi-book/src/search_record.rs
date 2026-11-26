//! 1 回の探索で「何を要求し・何を予定し・どこまで完了したか」と、探索対象となった盤面を記録するモジュール。
use super::*;
use deft_reversi_engine::ResultKind;
/// エンジンが行う探索の種類。
/// 残り空きマス数と同じ深さであっても、中盤探索（`Midgame`）を終盤完全読み（`Endgame`）へ勝手に読み替えない。
/// また `Endgame` であっても、選択率が `Full`（枝刈りなしの 100% 探索）でなければ最終石差の確定にはならない。
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
/// 利用者・設定が要求する探索の粒度・仕様。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestedAnalysis {
    /// 候補手を打った後の子局面側でレベルを解決し、親局面側へ深さを +1 シフトする。
    ChildLevel(u8),
    /// 探索ルート局面側でエンジンレベルを直接解決する。
    RootLevel(u8),
    /// 親局面側での固定深さ・選択率による中盤探索。
    ParentDepth { depth: u8, selectivity: Selectivity },
    /// 指定選択率による終盤完全読み。
    Endgame { selectivity: Selectivity },
    /// 外部から取り込まれた探索結果。
    External,
}
/// 1 回の探索の完了記録。
/// `requested` は利用者の要求、`planned` は実行予定だった条件、`completed` は実際に完了した条件を表す。
/// 途中で打ち切られた外部結果では、予定より浅い深さまでしか完了していない場合がある。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompletedSearch {
    pub requested: RequestedAnalysis,
    pub planned: SearchKind,
    pub completed: SearchKind,
    /// 正規化済みの探索ルート盤面（推測された親ホライズンではない）。
    pub root: [u64; 2],
    /// 探索対象としたルート着手マスク（パス強制や終局ルートでは 0 が有効）。
    pub moves: u64,
    /// 0..=63はマス、64はパス、65は終局で着手なし。
    /// 集合全体の上限を記録する場合は、探索対象から選んだ代表手。
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
            || matches!(self.requested,RequestedAnalysis::ChildLevel(n) | RequestedAnalysis::RootLevel(n) if !(1..=60).contains(&n))
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
