//! 対局時の品質判定と登録手の選択。ファイル操作・生成・通常探索は行わない。
//!
//! decideは利用できない理由も返し、chooseは着手だけが必要な呼出し側向け。
//! check_partsが中盤・終盤・旧データ・手動値の条件を分けて確認する。
//! ここでいう利用可能な上限は損失の基準には使うが、着手としては点評価だけを選ぶ。
use super::*;
use std::sync::Arc;
#[derive(Clone, Copy, Debug, Default)]
pub enum MovePolicy {
    #[default]
    Best,
    RandomWithinLoss {
        max_loss: u8,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookMiss {
    NoBook,
    InvalidPosition,
    Terminal,
    MissingPosition,
    InsufficientCoverage,
    ModelMismatch,
    LegacyUnknown,
    DerivedRejected,
    ManualRejected,
    MidgameDepth,
    MidgameSelectivity,
    EndgameSelectivity,
    NoPointValue,
    NoRegisteredMove,
    OutsideLoss,
}
#[derive(Clone, Debug)]
pub enum BookDecision {
    Chosen(BookChoice),
    Fallback(BookMiss),
}
#[derive(Clone, Copy, Debug)]
pub struct UsePolicy {
    pub moves: MovePolicy,
    /// 中盤探索の最低深さ。終盤探索の利用にはmin_endgame_selectivityを使う。
    pub min_depth: u8,
    pub min_selectivity: Selectivity,
    /// 全合法手に利用条件を満たす点評価を求める。上限だけの候補は条件を満たさない。
    pub require_all_evaluated: bool,
    pub min_endgame_selectivity: Selectivity,
    pub allow_calculated: bool,
    /// Legacy values never acquire a guessed midgame/endgame kind. This
    /// compatibility option checks their recorded depth and selectivity.
    pub allow_imported: bool,
    /// 手動値、または手動値を含めて計算した値の利用を許可する。
    pub allow_manual: bool,
}
impl Default for UsePolicy {
    fn default() -> Self {
        Self {
            moves: MovePolicy::Best,
            min_depth: 12,
            min_selectivity: Selectivity::Percent95,
            require_all_evaluated: false,
            min_endgame_selectivity: Selectivity::Percent85,
            allow_calculated: true,
            allow_imported: false,
            allow_manual: false,
        }
    }
}
#[derive(Clone, Debug)]
pub struct BookChoice {
    pub action: Action,
    pub value: Evaluation,
    pub all_moves_evaluated: bool,
}
/// Picks book moves for positions in the book. It does not search: when
/// `choose` returns None, the caller searches with its `Engine` or
/// `EngineWorker` as usual.
pub struct BookPlayer {
    identity: AnalysisIdentity,
    book: Option<Arc<Book>>,
    policy: UsePolicy,
    rng: u64,
}
impl BookPlayer {
    /// Uses the values that `engine`'s model and calibration produced; see
    /// [`analysis_identity`]. Call `set_engine` after changing either.
    pub fn new(
        engine: &Engine,
        book: Option<Arc<Book>>,
        policy: UsePolicy,
        seed: u64,
    ) -> Result<Self> {
        validate_policy(policy)?;
        Ok(Self {
            identity: analysis_identity(engine)?,
            book,
            policy,
            rng: seed,
        })
    }
    pub fn set_engine(&mut self, engine: &Engine) -> Result<()> {
        self.identity = analysis_identity(engine)?;
        Ok(())
    }
    pub fn set_book(&mut self, book: Option<Arc<Book>>) {
        self.book = book;
    }
    pub fn set_policy(&mut self, policy: UsePolicy) -> Result<()> {
        validate_policy(policy)?;
        self.policy = policy;
        Ok(())
    }
    /// The book move for `position`, or None when the book has no usable one.
    /// Does not allocate.
    pub fn choose(&mut self, position: impl Into<Board>) -> Option<BookChoice> {
        match self.decide(position) {
            BookDecision::Chosen(c) => Some(c),
            BookDecision::Fallback(_) => None,
        }
    }
    pub fn decide(&mut self, position: impl Into<Board>) -> BookDecision {
        let board = position.into();
        if board.empty_count() > 60 {
            return BookDecision::Fallback(BookMiss::InvalidPosition);
        }
        if board.is_terminal() {
            return BookDecision::Fallback(BookMiss::Terminal);
        }
        let Some(book) = self.book.as_ref() else {
            return BookDecision::Fallback(BookMiss::NoBook);
        };
        match choose(book, board, self.identity, self.policy, &mut self.rng) {
            Ok(c) => BookDecision::Chosen(c),
            Err(reason) => BookDecision::Fallback(reason),
        }
    }
}
impl UsePolicy {
    pub fn check(
        self,
        value: Evaluation,
        identity: AnalysisIdentity,
    ) -> std::result::Result<(), BookMiss> {
        self.check_parts(
            value.is_calculated(),
            value.result().final_bounds().is_some(),
            value.identity(),
            value.conditions(),
            identity,
        )
    }
    /// Check a borrowed value from BookEntry::moves without expanding its trace.
    pub fn check_ref(
        self,
        value: EvaluationRef<'_>,
        identity: AnalysisIdentity,
    ) -> std::result::Result<(), BookMiss> {
        self.check_parts(
            value.is_calculated(),
            value.result().final_bounds().is_some(),
            value.identity(),
            value.conditions(),
            identity,
        )
    }
    // 合成値に中盤と終盤が混ざる場合も、それぞれの最低条件を確認する。
    // 全体のhorizonや最小の選択率一つだけで判定すると、探索方法の違いを失ってしまう。
    fn check_parts(
        self,
        derived: bool,
        proven: bool,
        profile: AnalysisIdentity,
        q: SearchConditions,
        identity: AnalysisIdentity,
    ) -> std::result::Result<(), BookMiss> {
        if derived && !self.allow_calculated {
            return Err(BookMiss::DerivedRejected);
        }
        if proven {
            return Ok(());
        }
        if profile != identity {
            return Err(BookMiss::ModelMismatch);
        }
        if q.has_manual() && !self.allow_manual {
            return Err(BookMiss::ManualRejected);
        }
        if q.has_imported() && !self.allow_imported {
            return Err(BookMiss::LegacyUnknown);
        }
        if q.has_midgame() {
            if q.midgame_depth().unwrap() < self.min_depth {
                return Err(BookMiss::MidgameDepth);
            }
            if q.midgame_selectivity().unwrap() < self.min_selectivity {
                return Err(BookMiss::MidgameSelectivity);
            }
        }
        if q.has_endgame() && q.endgame_selectivity().unwrap() < self.min_endgame_selectivity {
            return Err(BookMiss::EndgameSelectivity);
        }
        if q.has_imported() {
            if q.legacy_depth < self.min_depth {
                return Err(BookMiss::MidgameDepth);
            }
            if q.legacy_sel < self.min_selectivity as u8 {
                return Err(BookMiss::MidgameSelectivity);
            }
        }
        Ok(())
    }
}
fn validate_policy(p: UsePolicy) -> Result<()> {
    if p.min_depth > 60 {
        return Err(invalid("minimum depth"));
    }
    if matches!(p.moves,MovePolicy::RandomWithinLoss{max_loss} if max_loss>128) {
        return Err(invalid("maximum loss"));
    }
    Ok(())
}
fn choose(
    book: &Book,
    board: Board,
    identity: AnalysisIdentity,
    policy: UsePolicy,
    rng: &mut u64,
) -> std::result::Result<BookChoice, BookMiss> {
    let entry = book.probe(board).ok_or(BookMiss::MissingPosition)?;
    let usable = |c: &BookMove<'_>| {
        c.value
            .is_some_and(|v| v.bound() == Bound::Point && policy.check_ref(v, identity).is_ok())
    };
    if policy.require_all_evaluated
        && (!entry.all_moves_evaluated() || !entry.moves().all(|c| usable(&c)))
    {
        return Err(BookMiss::InsufficientCoverage);
    }
    // Only registered moves keep the game in the book, but the threshold is
    // set by every usable value: a registered move that is known to be worse
    // than an unregistered one by more than the allowed loss is not played.
    let best = entry
        .moves()
        .filter(|c| {
            usable(c)
                || c.value.is_some_and(|v| {
                    matches!(v.bound(), Bound::Upper) && policy.check_ref(v, identity).is_ok()
                })
        })
        .filter_map(|c| c.value.map(|v| v.score()))
        .max()
        .ok_or_else(|| {
            entry
                .moves()
                .filter(|c| c.registered)
                .find_map(|c| c.value.and_then(|v| policy.check_ref(v, identity).err()))
                .unwrap_or(if entry.moves().any(|c| c.registered) {
                    BookMiss::NoPointValue
                } else {
                    BookMiss::NoRegisteredMove
                })
        })?;
    // bestには未登録手や上限も含む。上限が高くて登録手の損失を保証できない場合は
    // 安易に登録手を返さず、OutsideLossなどの理由で呼出し側の通常探索へ戻す。
    let threshold = best
        - match policy.moves {
            MovePolicy::Best => 0,
            MovePolicy::RandomWithinLoss { max_loss } => i16::from(max_loss),
        };
    let qualifies =
        |c: &BookMove<'_>| c.registered && usable(c) && c.value.unwrap().score() >= threshold;
    let count = entry.moves().filter(qualifies).count() as u64;
    if count == 0 {
        return Err(if entry.moves().any(|c| c.registered && usable(&c)) {
            BookMiss::OutsideLoss
        } else if !entry.moves().any(|c| c.registered) {
            BookMiss::NoRegisteredMove
        } else {
            entry
                .moves()
                .filter(|c| c.registered)
                .find_map(|c| c.value.and_then(|v| policy.check_ref(v, identity).err()))
                .unwrap_or(BookMiss::NoPointValue)
        });
    }
    let index = match policy.moves {
        MovePolicy::Best => 0,
        _ => uniform(rng, count),
    };
    let c = entry
        .moves()
        .filter(qualifies)
        .nth(index as usize)
        .expect("counted candidate");
    Ok(BookChoice {
        action: c.action,
        value: c.value.unwrap().to_owned(),
        all_moves_evaluated: entry.all_moves_evaluated(),
    })
}
// SplitMix64 with rejection sampling: stable across platforms, no modulo bias.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}
fn uniform(state: &mut u64, n: u64) -> u64 {
    let threshold = n.wrapping_neg() % n;
    loop {
        let v = next(state);
        if v >= threshold {
            return v % n;
        }
    }
}
