//! リバーシの Book の編集・自動生成・対局利用を担うクレート。
//!
//! 用途に応じて大きく 3 つの型に分かれています：
//! - [`BookEditor`]：Book の作成・編集・生成の途中状態を管理する作業用ワークスペース（`.dbook-work`）。
//! - [`Book`]：対局時に高速に読み取るためのコンパクトな読み取り専用スナップショット（`.dbook`）。
//! - [`BookPlayer`]：[`Book`] を参照し、設定された品質条件（深さ・選択率など）を満たす登録手を選ぶ。
//!   条件を満たす手がないときは `None` を返し、通常の探索（`Engine`）へフォールバックする設計になっており、
//!   `Engine` の探索自体は内部で Book を読みません。
//!
//! 探索結果を登録して読み取る最小例：
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

/// デフト・リバーシのワークスペースに含まれる Book ライブラリのバージョン。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 評価モデル・校正・探索仕様の識別値。推定値を再利用できるかの照合に使う。
pub type AnalysisIdentity = [u8; 32];
/// Book 操作・読込・探索で発生しうるエラー。
#[derive(Debug)]
pub enum BookError {
    /// ファイル入出力エラー。
    Io(io::Error),
    /// エンジン探索エラー。
    Search(EngineError),
    /// データ形式や引数・不変条件の不正。
    Invalid(String),
    /// 読み込みサイズ・件数等の制限超過。
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
/// `.dbook` / `.dbook-work` ファイル読み込み時のサイズ・件数上限。
/// 生成中の Book 全体や `Engine` のメモリ予算とは独立して、破損・巨大ファイルによる過剰確保を防ぐ。
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
// Key は「手番側の石」と「相手側の石」を回転・反転で正規化した盤面を表す。
// 同じ形の盤面を 1 つにまとめるため黒白の絶対色は持たず、生成時の黒番・白番ごとの損失条件には `generate::Context` を使う。
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
/// A1 から H8 方向の合法手一覧（パス必須時はパスのみ）を返す。
/// 格納される候補手順序はこの並び順に依存する。
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

fn selectivity_code(value: Selectivity) -> u8 {
    match value {
        Selectivity::Percent83 => 7,
        _ => value as u8 + 1,
    }
}
fn selectivity(code: u8) -> Result<Selectivity> {
    match code {
        7 => Ok(Selectivity::Percent83),
        2..=6 => Ok(Selectivity::ALL[(code - 1) as usize]),
        _ => Err(invalid("unsupported selectivity")),
    }
}

// 古い互換リーダーで読み書きしても失われないよう保持する未解釈の拡張セクション。
#[derive(Clone, Debug)]
struct Extension {
    kind: u32,
    bytes: Vec<u8>,
}
fn extension_bytes(es: &[Extension]) -> u64 {
    es.iter().map(|e| e.bytes.capacity() as u64).sum::<u64>() + std::mem::size_of_val(es) as u64
}

/// 評価重み・校正パラメータ・探索仕様を識別するハッシュ値（[`AnalysisIdentity`]）を計算する。
/// 推定値は同一識別値の下でのみ比較可能であり、異なる識別値の結果を再利用するには
/// 明示的な再目標化・再解析が必要となる（確定した終局結果は常に再利用可能）。
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

/// 候補手ごとの評価値から、その局面全体で期待できる最大スコアを求める。
///
/// たとえば候補 A が `+4`、候補 B が「`+2` 以下（上界）」と分かっていれば、
/// 局面の最善スコアは `+4` と確定できる（ただし全候補に点評価が揃っているわけではない）。
/// 戻り値の `bool` は後者の「全候補が点評価を持っているか」を表し、局面値が決まるかどうかとは独立している。
/// 下界（`Bound::Lower`）しか分からない候補や未評価の候補がある場合は、その手が最善になる可能性が残るため局面値は決めない。
/// 全候補が終局までの確定石差区間（`Score::Final`）を持つ場合は、それらを統合した石差区間として計算する。
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
