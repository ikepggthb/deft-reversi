//! 対局時の品質判定と、登録された手の中から実際の手を選ぶモジュール（ファイル操作・Book 生成・通常探索は行わない）。
//!
//! - [`BookPlayer::decide`] は手が選べなかった理由（[`BookMiss`]）も返すため、診断やログ出力に向いている。
//! - [`BookPlayer::choose`] は着手だけが必要な呼び出し側向けの簡易版。
//! - `UsePolicy::check_parts` が中盤・終盤・旧データ・手動値の条件をそれぞれ分けて検査する。
//!   ここでいう「上界」しか分からない候補手は損失計算の基準（これより悪い手は避ける）には使うが、
//!   実際に指す手としては正確な点評価（`Bound::Point`）を持つ登録手だけを選ぶ。
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
    /// 旧形式から取り込まれた値は中盤・終盤の種別を推測しない。
    /// この互換オプションは、記録された深さと選択率を基準に利用可否を検査する。
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
/// Book 内の局面に対して登録手を選択するプレイヤー。
/// 自身では探索を行わないため、`choose` が `None` を返した場合は呼び出し側が
/// 通常の `Engine` や `EngineWorker` で探索する。
pub struct BookPlayer {
    identity: AnalysisIdentity,
    book: Option<Arc<Book>>,
    policy: UsePolicy,
    rng: u64,
}
impl BookPlayer {
    /// `engine` の評価モデルと校正パラメータから生成される識別値（[`analysis_identity`]）を用いる。
    /// モデルや校正を変更した後は [`Self::set_engine`] を呼び出すこと。
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
    /// `position` に対する Book の手を選択する。利用可能な手がない場合は `None` を返す（メモリ確保なし）。
    pub fn choose(&mut self, position: impl Into<Board>) -> Option<BookChoice> {
        match self.decide(position) {
            BookDecision::Chosen(c) => Some(c),
            BookDecision::Fallback(_) => None,
        }
    }
    pub fn decide(&mut self, position: impl Into<Board>) -> BookDecision {
        Self::decide_with(
            self.book.as_deref(),
            position.into(),
            self.identity,
            self.policy,
            Some(&mut self.rng),
        )
    }

    /// 呼び出し側の盤面の向きで記録済み候補手を照会する（対局ポリシーで弾かれる評価値も含む）。
    pub fn probe(&self, position: impl Into<Board>) -> Option<BookEntry<'_>> {
        self.book.as_ref()?.probe(position.into())
    }

    /// 設定された探索品質条件を満たす点評価（石差）を返す。
    pub fn point_value(&self, candidate: BookMove<'_>) -> std::result::Result<i16, BookMiss> {
        let value = candidate.value.ok_or(BookMiss::NoPointValue)?;
        self.policy.check_ref(value, self.identity)?;
        value.result().point_value().ok_or(BookMiss::NoPointValue)
    }

    /// 対局用乱数を進めずに、条件を満たす最善手を返す。
    pub fn recommend(&self, position: impl Into<Board>) -> BookDecision {
        Self::decide_with(
            self.book.as_deref(),
            position.into(),
            self.identity,
            self.policy,
            None,
        )
    }

    /// 指定局面から条件を満たす Book の手を連続して辿る。
    /// 返却される手順がそのまま再生できるよう、強制パスも手順に含める。
    pub fn continuation(&self, position: impl Into<Board>) -> Vec<Action> {
        let mut board = position.into();
        let mut line = Vec::new();
        // 相手番側に有効な Book の手が見つかるまで、保留中のパスを保持する。
        while !board.is_terminal() {
            let pass = board.must_pass();
            if pass {
                board = board.play(Action::Pass).expect("forced pass");
            }
            let BookDecision::Chosen(choice) = self.recommend(board) else {
                break;
            };
            if pass {
                line.push(Action::Pass);
            }
            line.push(choice.action);
            board = board.play(choice.action).expect("legal Book move");
        }
        line
    }

    fn decide_with(
        book: Option<&Book>,
        board: Board,
        identity: AnalysisIdentity,
        policy: UsePolicy,
        rng: Option<&mut u64>,
    ) -> BookDecision {
        if board.empty_count() > 60 {
            return BookDecision::Fallback(BookMiss::InvalidPosition);
        }
        if board.is_terminal() {
            return BookDecision::Fallback(BookMiss::Terminal);
        }
        let Some(book) = book else {
            return BookDecision::Fallback(BookMiss::NoBook);
        };
        match choose(book, board, identity, policy, rng) {
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
    /// [`BookEntry::moves`] から得た借用評価値をトレース展開せずに検査する。
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
    rng: Option<&mut u64>,
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
    // Book 内に手順の流れを保つのは「登録手（`registered`）」だけだが、
    // 許容損失の基準となる最善スコアの計算には、利用可能なすべての評価（未登録手や上界 `Bound::Upper` を含む）を含める。
    // 未登録手より許容損失を超えて劣ることが分かっている登録手を誤って指さないため。
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
    // best には未登録手や上界も含まれる。上界が高いために登録手の損失が許容範囲内と保証できない場合は、
    // 安易に登録手を返さず、`OutsideLoss` などの理由で呼び出し側の通常探索へフォールバックする。
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
    let mut eligible = entry.moves().filter(qualifies);
    let c = match (rng, policy.moves) {
        (None, _) => eligible.min_by_key(|c| std::cmp::Reverse(c.value.unwrap().score())),
        (Some(_), MovePolicy::Best) => eligible.next(),
        (Some(rng), MovePolicy::RandomWithinLoss { .. }) => {
            eligible.nth(uniform(rng, count) as usize)
        }
    }
    .expect("counted candidate");
    Ok(BookChoice {
        action: c.action,
        value: c.value.unwrap().to_owned(),
        all_moves_evaluated: entry.all_moves_evaluated(),
    })
}
// 棄却サンプリング付き SplitMix64：プラットフォーム間で再現性があり剰余バイアスがない。
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
