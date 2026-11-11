//! リバーシのbookの編集・生成・対局利用。
//!
//! [`BookEditor`]が局面・評価・生成の再開状態を所有し、[`Book`]は対局用に
//! 読み取るデータを持つ。[`BookPlayer`]は利用条件を満たす登録手を選ぶ。
//! 手が選べないときの通常探索は呼出し側が行い、Engineの探索中はbookを読まない。
//!
//! ソースの読む順序と確認事項はリポジトリの `docs/book-v2/source-review.md`、
//! 評価の意味は `docs/book-v2/value-model.md` を参照。
//!
//! 探索結果を登録して読む最小例：
//! ```
//! use deft_reversi_book::{BookEditor, RequestedAnalysis, SearchKind};
//! use deft_reversi_engine::{Board, Engine, EngineOptions, Goal, Model, PatternModel, SearchRequest, Selectivity};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut engine = Engine::new(Model::from_pattern(PatternModel::zero()), EngineOptions::default().tt_mib(1))?;
//! let board = Board::new().canonical();
//! let request = SearchRequest::new(Goal::Depth(2));
//! let planned = SearchKind::from_result(engine.search_plan(board, &request)?);
//! let result = engine.search(board, &request)?;
//! let mut editor = BookEditor::for_engine(&engine)?;
//! editor.record_search(board, RequestedAnalysis::ParentDepth { depth: 2, selectivity: Selectivity::Full }, planned, &result)?;
//! editor.register_move(board, result.best_move.unwrap())?;
//! let book = editor.export()?;
//! let entry = book.probe(board).unwrap();
//! let chosen = entry.moves().find(|m| m.registered).unwrap();
//! let value = chosen.value.unwrap();
//! assert!(value.result().point_value().is_some());
//! assert_eq!(value.conditions().midgame_depth(), Some(2));
//! # Ok(())
//! # }
//! ```
use deft_reversi_engine::{Action, Board, Engine, EngineError, Selectivity};
use std::{fmt, io, time::Duration};
mod catalog;
mod editor;
pub use catalog::{AnalysisProfile, GenerationRun, InterruptedAnalysis, RunStatus};
mod score;
pub use score::{Bound, Score, ScoreInterval};
mod search_record;
pub use search_record::{CompletedSearch, RequestedAnalysis, SearchKind};
mod conditions;
pub use conditions::SearchConditions;
mod evaluation;
pub use evaluation::{AnalysisCategory, Evaluation, EvaluationSource};
pub mod legacy;
mod values;
pub use values::EvaluationRef;
use values::{ValueId, ValueStore};
mod format;
mod generate;
mod grow;
mod player;
mod schedule;
mod snapshot;
mod stats;
mod symmetry;
pub use editor::{BookEditor, MigrationReport, MoveValues, SelectedFrom};
pub use generate::{
    AnalysisSchedule, BookSide, GenerationConfig, GenerationLimits, GenerationProfile,
    GenerationReport, GenerationRoot, GenerationStop, Stage,
};
pub use player::{BookChoice, BookDecision, BookMiss, BookPlayer, MovePolicy, UsePolicy};
pub use schedule::SchedulePolicy;
pub use snapshot::{Book, BookCandidate, BookEntry, BookMove, BookStats};
pub use stats::{QualityBucket, ValueKindBucket, WorkspaceStats};
pub use symmetry::Symmetry;

/// 評価モデル・校正・探索仕様の識別値。推定値を再利用できるかの照合に使う。
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

#[derive(Clone, Copy, Debug)]
/// ファイル読込の件数・割当制限。生成中のbook全体やEngineの資源予算とは別。
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
// Keyは「手番側の石、相手側の石」を回転・反転で正規化した局面。
// 黒白の絶対色は含まない。生成の色別損失条件にはgenerate::Contextを使う。
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
/// Legal actions from A1 towards H8, or a lone pass. The stored candidate
/// order depends on this.
fn legal_actions(b: Board) -> Vec<Action> {
    let mut out: Vec<_> = b.legal_moves().map(Action::Place).collect();
    if b.must_pass() {
        out.push(Action::Pass);
    }
    out
}
fn terminal_value(b: Board) -> Evaluation {
    Evaluation::rules(b)
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

/// Identifies evaluation weights, calibration and search semantics. Estimated
/// values are comparable only under the same identity. Older identities require
/// explicit retarget/reanalysis; proven game results remain reusable.
pub fn analysis_identity(engine: &Engine) -> std::io::Result<AnalysisIdentity> {
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
        engine.write_evaluation_model(&mut buffered)?;
        std::io::Write::flush(&mut buffered)?;
    }
    writer.0.update(b"deft-book-analysis-v1");
    engine.write_analysis_settings(&mut writer)?;
    Ok(writer.0.finalize().into())
}

/// 候補の値から局面全体の最大値を求める。
/// 例：A=+4、B<=+2なら局面=+4と分かるが、全候補に点評価があるわけではない。
/// 戻り値のboolは後者の「全候補が点評価」であり、局面値の有無とは独立する。
/// 下限だけの候補や不明な候補があれば、その候補が最大かもしれず推定値を決めない。
/// 全候補が最終石差の区間を持つ場合は、区間の最大値として別に計算する。
fn combine_values(
    values: impl IntoIterator<Item = Option<Evaluation>>,
    identity: AnalysisIdentity,
    root: Key,
) -> (Option<Evaluation>, bool) {
    let mut summary = SearchConditions::empty();
    let mut best: Option<Evaluation> = None;
    let mut upper = -65;
    let mut all_certified = true;
    let mut certified_lower: Option<i16> = None;
    let mut certified_upper: Option<i16> = Some(-64);
    let mut point_coverage = true;
    let mut complete = true;
    let mut count = 0;
    for value in values {
        let Some(v) = value else {
            return (None, false);
        };
        count += 1;
        if !v.is_final() && v.identity() != identity {
            return (None, false);
        }
        summary.include(v.conditions());
        if let Some(interval) = v.result().final_bounds() {
            if let Some(l) = interval.lower() {
                certified_lower = Some(certified_lower.map_or(l, |old| old.max(l)));
            }
            certified_upper = match (certified_upper, interval.upper()) {
                (Some(a), Some(b)) => Some(a.max(b)),
                _ => None,
            };
        } else {
            all_certified = false;
        }
        match v.bound() {
            Bound::Point => {
                if best.is_none_or(|b| v.score() > b.score()) {
                    best = Some(v);
                }
            }
            Bound::Upper => {
                upper = upper.max(v.score());
                complete = false;
            }
            Bound::Lower => {
                point_coverage = false;
                complete = false;
            }
        }
    }
    if count == 0 {
        return (None, false);
    }
    let claim = if all_certified {
        Score::Final(
            ScoreInterval::new(certified_lower, certified_upper).expect("maximum interval"),
        )
    } else {
        let Some(value) = best else {
            return (None, false);
        };
        if !point_coverage || value.score() < upper {
            return (None, complete);
        }
        Score::Estimate {
            score: value.score(),
            bound: Bound::Point,
        }
    };
    let analysis = EvaluationSource::Position {
        root: [root.0, root.1],
        conditions: summary,
    };
    (
        Some(Evaluation::new(claim, analysis, identity).expect("valid maximum")),
        complete,
    )
}
