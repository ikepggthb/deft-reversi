use super::*;
use crate::{SearchRequest, SearchResult};
use std::{sync::Arc, time::Instant};
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
#[derive(Clone, Debug)]
pub enum PlaySource {
    Book(BookChoice),
    Search(SearchResult),
}
#[derive(Clone, Debug)]
pub struct PlayResult {
    pub source: PlaySource,
    pub elapsed: Duration,
}
/// Owns its Engine so identity cannot silently become stale through model mutation.
/// Use replace_engine to switch evaluation/calibration; search APIs remain untouched.
pub struct BookPlayer {
    engine: Engine,
    identity: AnalysisIdentity,
    book: Option<Arc<Book>>,
    policy: UsePolicy,
    rng: u64,
}
impl BookPlayer {
    pub fn new(
        engine: Engine,
        book: Option<Arc<Book>>,
        policy: UsePolicy,
        seed: u64,
    ) -> Result<Self> {
        validate_policy(policy)?;
        let identity = engine.book_identity()?;
        Ok(Self {
            engine,
            identity,
            book,
            policy,
            rng: seed,
        })
    }
    pub fn replace_engine(&mut self, engine: Engine) -> Result<Engine> {
        let id = engine.book_identity()?;
        self.identity = id;
        Ok(std::mem::replace(&mut self.engine, engine))
    }
    pub fn into_engine(self) -> Engine {
        self.engine
    }
    pub fn set_book(&mut self, book: Option<Arc<Book>>) {
        self.book = book;
    }
    pub fn set_policy(&mut self, policy: UsePolicy) -> Result<()> {
        validate_policy(policy)?;
        self.policy = policy;
        Ok(())
    }
    /// The supplied request describes fallback search; book choice is explicitly tagged.
    pub fn choose_move(&mut self, mut fallback: SearchRequest) -> Result<PlayResult> {
        if fallback.board.empty_count() > 60 {
            return Err(invalid(
                "book player position must have at least four stones",
            ));
        }
        let started = Instant::now();
        if !fallback.stop.is_stopped()
            && !fallback.board.is_terminal()
            && !fallback.time_limit.is_some_and(|d| d.is_zero())
        {
            let selected = self
                .book
                .as_ref()
                .and_then(|b| choose(b, fallback.board, self.identity, self.policy, &mut self.rng));
            if let Some(choice) = selected
                && !fallback.stop.is_stopped()
                && fallback.time_limit.is_none_or(|d| started.elapsed() < d)
            {
                return Ok(PlayResult {
                    source: PlaySource::Book(choice),
                    elapsed: started.elapsed(),
                });
            }
        }
        if let Some(d) = fallback.time_limit {
            fallback.time_limit = Some(d.saturating_sub(started.elapsed()));
        }
        let result = self.engine.search(fallback)?;
        Ok(PlayResult {
            source: PlaySource::Search(result),
            elapsed: started.elapsed(),
        })
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
    let eligible = |c: &BookCandidate| {
        c.registered
            && c.value
                .is_some_and(|v| v.eligible(identity, policy.min_depth, policy.min_selectivity))
    };
    if policy.require_all_evaluated
        && (!entry.all_moves_evaluated()
            || !entry.candidates().all(|c| {
                c.value
                    .is_some_and(|v| v.eligible(identity, policy.min_depth, policy.min_selectivity))
            }))
    {
        return None;
    }
    let best = entry
        .candidates()
        .filter(eligible)
        .filter_map(|c| c.value.map(|v| v.score))
        .max()?;
    let threshold = best
        - match policy.moves {
            MovePolicy::Best => 0,
            MovePolicy::RandomWithinLoss { max_loss } => i16::from(max_loss),
        };
    let qualifies = |c: &BookCandidate| eligible(c) && c.value.unwrap().score >= threshold;
    let count = entry.candidates().filter(qualifies).count() as u64;
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
