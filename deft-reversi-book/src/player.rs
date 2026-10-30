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
#[derive(Clone, Copy, Debug)]
pub struct UsePolicy {
    pub moves: MovePolicy,
    pub min_depth: u8,
    pub min_selectivity: Selectivity,
    pub require_all_evaluated: bool,
}
impl Default for UsePolicy {
    fn default() -> Self {
        Self {
            moves: MovePolicy::Best,
            min_depth: 12,
            min_selectivity: Selectivity::Percent95,
            require_all_evaluated: false,
        }
    }
}
#[derive(Clone, Debug)]
pub struct BookChoice {
    pub action: Action,
    pub value: Assessment,
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
        let board = position.into();
        if board.empty_count() > 60 || board.is_terminal() {
            return None;
        }
        choose(
            self.book.as_ref()?,
            board,
            self.identity,
            self.policy,
            &mut self.rng,
        )
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
) -> Option<BookChoice> {
    let entry = book.probe(board)?;
    let usable = |c: &BookCandidate| {
        c.value
            .is_some_and(|v| v.eligible(identity, policy.min_depth, policy.min_selectivity))
    };
    if policy.require_all_evaluated
        && (!entry.all_moves_evaluated() || !entry.candidates().all(|c| usable(&c)))
    {
        return None;
    }
    // Only registered moves keep the game in the book, but the threshold is
    // set by every usable value: a registered move that is known to be worse
    // than an unregistered one by more than the allowed loss is not played.
    let best = entry
        .candidates()
        .filter(usable)
        .filter_map(|c| c.value.map(|v| v.score))
        .max()?;
    let threshold = best
        - match policy.moves {
            MovePolicy::Best => 0,
            MovePolicy::RandomWithinLoss { max_loss } => i16::from(max_loss),
        };
    let qualifies =
        |c: &BookCandidate| c.registered && usable(c) && c.value.unwrap().score >= threshold;
    let count = entry.candidates().filter(qualifies).count() as u64;
    if count == 0 {
        return None;
    }
    let index = match policy.moves {
        MovePolicy::Best => 0,
        _ => uniform(rng, count),
    };
    let c = entry.candidates().filter(qualifies).nth(index as usize)?;
    Some(BookChoice {
        action: c.action,
        value: c.value.unwrap(),
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
