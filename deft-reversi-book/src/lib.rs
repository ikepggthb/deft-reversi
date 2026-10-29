//! Opening books: immutable play snapshots, editable evidence, offline generation.
//! Search itself never reads a book. See docs/book-implementation.md.
use deft_reversi_engine::{Action, Board, Bound, Engine, EngineError, Selectivity};
use std::{fmt, io, time::Duration};
mod editor;
mod format;
mod generate;
mod player;
mod snapshot;
mod symmetry;
pub use editor::BookEditor;
pub use generate::{
    BookSide, GenerationConfig, GenerationLimits, GenerationReport, GenerationRoot, GenerationStop,
    Stage,
};
pub use player::{BookChoice, BookPlayer, MovePolicy, PlayResult, PlaySource, UsePolicy};
pub use snapshot::{Book, BookCandidate, BookEntry, BookStats};
pub use symmetry::Symmetry;

pub type AnalysisIdentity = [u8; 32];
#[derive(Debug)]
pub enum BookError {
    Io(io::Error),
    Search(EngineError),
    Invalid(String),
    Limit(&'static str),
}
impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "book I/O: {e}"),
            Self::Search(e) => write!(f, "book search: {e}"),
            Self::Invalid(s) => write!(f, "invalid book: {s}"),
            Self::Limit(s) => write!(f, "book limit: {s}"),
        }
    }
}
impl std::error::Error for BookError {}
impl From<io::Error> for BookError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<EngineError> for BookError {
    fn from(e: EngineError) -> Self {
        Self::Search(e)
    }
}
pub type Result<T> = std::result::Result<T, BookError>;
fn invalid(s: impl Into<String>) -> BookError {
    BookError::Invalid(s.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueOrigin {
    Search,
    Manual,
    Imported,
    Rules,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisQuality {
    /// Effective depth in placements, measured from the candidate's parent.
    pub depth: u8,
    pub selectivity: Selectivity,
    /// Proven terminal disc score, not merely a fixed-depth Bound::Exact.
    pub solved: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assessment {
    pub score: i16,
    pub quality: AnalysisQuality,
    pub bound: Bound,
    pub identity: AnalysisIdentity,
    pub origin: ValueOrigin,
}
impl Assessment {
    fn validate(self) -> Result<()> {
        if !(-64..=64).contains(&self.score) || self.quality.depth > 60 {
            return Err(invalid("score or depth"));
        }
        if self.quality.solved
            && (self.bound != Bound::Exact
                || self.quality.selectivity != Selectivity::Full
                || self.origin == ValueOrigin::Manual)
        {
            return Err(invalid("unproven solved value"));
        }
        Ok(())
    }
    fn validate_at(self, board: Board) -> Result<()> {
        self.validate()?;
        if self.quality.solved && (self.quality.depth < board.empty_count() || self.score % 2 != 0)
        {
            return Err(invalid(
                "solved score/depth inconsistent with terminal proof",
            ));
        }
        Ok(())
    }
    fn eligible(
        self,
        identity: AnalysisIdentity,
        min_depth: u8,
        min_selectivity: Selectivity,
    ) -> bool {
        self.bound == Bound::Exact
            && (self.quality.solved
                || (self.identity == identity
                    && self.quality.depth >= min_depth
                    && (self.quality.selectivity as u8) >= (min_selectivity as u8)))
    }
    fn child_to_parent(mut self, placement: bool) -> Self {
        self.score = -self.score;
        self.quality.depth = (self.quality.depth + u8::from(placement)).min(60);
        self
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ReadLimits {
    pub max_bytes: u64,
    pub max_positions: u64,
    pub max_candidates: u64,
}
impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            max_bytes: 512 * 1024 * 1024,
            max_positions: 2_000_000,
            max_candidates: 32_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Key(u64, u64);
impl Key {
    fn new(b: Board) -> (Self, Symmetry) {
        let (b, t) = symmetry::canonical(b);
        (Self(b.player_bits(), b.opponent_bits()), t)
    }
    fn board(self) -> Board {
        Board::from_bits(self.0, self.1).expect("validated book key")
    }
    fn hash(self) -> u64 {
        self.0.wrapping_mul(0x9dda1c54cfe6b6e9) ^ self.1.wrapping_mul(0xa2e6c0300831e05a)
    }
}
fn action_code(a: Action) -> u8 {
    match a {
        Action::Place(s) => s.index(),
        Action::Pass => 64,
    }
}
fn action_from(c: u8) -> Result<Action> {
    if c == 64 {
        Ok(Action::Pass)
    } else {
        deft_reversi_engine::Square::new(c)
            .map(Action::Place)
            .ok_or_else(|| invalid("action"))
    }
}
fn legal_actions(b: Board) -> Vec<Action> {
    let mut moves = b.get_moves();
    let mut out = Vec::with_capacity(moves.count_ones() as usize);
    while moves != 0 {
        let bit = moves.isolate_lowest_one();
        moves ^= bit;
        out.push(Action::Place(
            deft_reversi_engine::Square::new(bit.trailing_zeros() as u8).unwrap(),
        ));
    }
    if out.is_empty() && b.get_opponent_moves() != 0 {
        out.push(Action::Pass);
    }
    out
}
fn terminal_value(b: Board) -> Assessment {
    Assessment {
        score: b.final_score() as i16,
        quality: AnalysisQuality {
            depth: b.empty_count(),
            selectivity: Selectivity::Full,
            solved: true,
        },
        bound: Bound::Exact,
        identity: [0; 32],
        origin: ValueOrigin::Rules,
    }
}

fn selectivity(n: u8) -> Result<Selectivity> {
    [
        Selectivity::Percent68,
        Selectivity::Percent77,
        Selectivity::Percent85,
        Selectivity::Percent95,
        Selectivity::Percent98,
        Selectivity::Percent99,
        Selectivity::Full,
    ]
    .get(n as usize)
    .copied()
    .ok_or_else(|| invalid("selectivity"))
}

// Cold opaque optional sections survive a load/save by an older compatible reader.
#[derive(Clone, Debug)]
struct Extension {
    kind: u32,
    bytes: Vec<u8>,
}
fn extension_bytes(es: &[Extension]) -> u64 {
    es.iter().map(|e| e.bytes.capacity() as u64).sum::<u64>() + std::mem::size_of_val(es) as u64
}

// Kept byte-compatible with the original in-engine book analysis identity.
fn analysis_identity(engine: &Engine) -> std::io::Result<AnalysisIdentity> {
    use sha2::{Digest, Sha256};
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    {
        let mut buffered = std::io::BufWriter::with_capacity(64 * 1024, &mut writer);
        engine.evaluation_model().write_to(&mut buffered)?;
        std::io::Write::flush(&mut buffered)?;
    }
    writer.0.update(b"deft-book-analysis-v1");
    engine.write_analysis_settings(&mut writer)?;
    Ok(writer.0.finalize().into())
}
