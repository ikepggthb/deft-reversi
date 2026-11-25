//! Book（`.dbook` 形式）の読み込みと評価内容・手順の CLI 表示モジュール。
//!
//! [`BookPlayer`] を通じて、局面に登録された候補手の評価値、探索条件、
//! 評価の出所（直接探索・子局面からの伝播・手動登録など）、Book ラインを整形する。

use crate::display;
use deft_reversi_book::{
    Book, BookMove, BookPlayer, EvaluationSource, MovePolicy, ReadLimits, Score, SearchConditions,
    UsePolicy,
};
use deft_reversi_engine::{Engine, Position, Selectivity};
use std::{path::Path, sync::Arc};

/// 指定パスの `.dbook` ファイルを読み込み、[`BookPlayer`]・登録局面数・モデル一致フラグを返す。
///
/// Book 作成時のエンジン／モデル識別子と現在の `engine` が一致しない場合は警告を標準エラー出力へ出す。
pub fn load(
    engine: &Engine,
    path: &Path,
    moves: MovePolicy,
    seed: u64,
) -> crate::Result<(BookPlayer, usize, bool)> {
    let book = Book::load(path, ReadLimits::default()).map_err(|e| {
        format!(
            "{}: {e}. Use the Book supplied with this CLI release.",
            path.display()
        )
    })?;
    let positions = book.stats().positions;
    let matches = book.identity() == deft_reversi_book::analysis_identity(engine)?;
    let policy = UsePolicy {
        moves,
        min_selectivity: Selectivity::Percent83,
        ..UsePolicy::default()
    };
    let player = BookPlayer::new(engine, Some(Arc::new(book)), policy, seed)?;
    if !matches {
        eprintln!("Warning: Book model mismatch; use the model and Book supplied together.");
    }
    Ok((player, positions, matches))
}

/// Book に記録された評価値・探索条件・評価の出所を 1 つの説明文字列に整形する。
pub fn value(score: Score, conditions: SearchConditions, source: EvaluationSource) -> String {
    let points = match score {
        Score::Estimate { score, bound } => {
            use deft_reversi_book::Bound::*;
            let sign = match bound {
                Point => "",
                Lower => ">=",
                Upper => "<=",
            };
            format!("{sign}{score:+}")
        }
        Score::Final(interval) => match (interval.lower(), interval.upper()) {
            (Some(lo), Some(hi)) if lo == hi => format!("{lo:+} exact"),
            (Some(lo), Some(hi)) => format!("[{lo:+}, {hi:+}] final"),
            (Some(lo), None) => format!(">={lo:+} final"),
            (None, Some(hi)) => format!("<={hi:+} final"),
            (None, None) => unreachable!("validated final interval"),
        },
    };
    let mut modes = Vec::new();
    if let (Some(depth), Some(sel)) = (conditions.midgame_depth(), conditions.midgame_selectivity())
    {
        modes.push(format!("d{depth}@{}%", sel.percent()));
    }
    if let Some(sel) = conditions.endgame_selectivity() {
        modes.push(format!("end@{}%", sel.percent()));
    }
    if conditions.has_manual() {
        modes.push("manual".into());
    }
    if let (Some(depth), Some(sel)) = (
        conditions.imported_depth(),
        conditions.imported_selectivity(),
    ) {
        modes.push(format!("imported d{depth}@{}%", sel.percent()));
    }
    let origin = match source {
        EvaluationSource::Search(_) | EvaluationSource::SearchMove(_) => "search",
        EvaluationSource::Position { .. } => "position",
        EvaluationSource::Child { .. } => "child",
        EvaluationSource::Move { .. } => "move",
        EvaluationSource::Terminal { .. } => "terminal",
        EvaluationSource::Manual { .. } => "manual",
        EvaluationSource::Imported { .. } => "imported",
    };
    modes.push(origin.into());
    format!("{points} ({})", modes.join(", "))
}

/// 候補手比較表（`analyze` コマンド）向けに、候補手の Book 評価値を 1 セル分の文字列に整形する。
///
/// 値があっても現在の [`UsePolicy`] で採用対象外になる場合は `[inactive]` を添える。
pub fn analysis_value(book: &BookPlayer, candidate: BookMove<'_>) -> String {
    let Some(v) = candidate.value.or(candidate.final_score) else {
        return "-".into();
    };
    let mut detail = value(v.result(), v.conditions(), v.source());
    if candidate.value.is_some()
        && v.result().point_value().is_some()
        && book.point_value(candidate).is_err()
    {
        detail.push_str(" [inactive]");
    }
    detail
}

/// 候補手 1 手分の登録状態（`linked` / `unlinked`）、有効評価値、確定値、使用不可理由を整形する。
pub fn candidate(book: &BookPlayer, candidate: BookMove<'_>) -> String {
    let mut parts = vec![if candidate.registered {
        "linked".into()
    } else {
        "unlinked".into()
    }];
    if let Some(v) = candidate.value {
        parts.push(value(v.result(), v.conditions(), v.source()));
    } else {
        parts.push("no evaluation".into());
    }
    if let Some(v) = candidate.final_score
        && candidate
            .value
            .is_none_or(|effective| effective.result() != v.result())
    {
        parts.push(format!(
            "certificate {}",
            value(v.result(), v.conditions(), v.source())
        ));
    }
    if let Err(reason) = book.point_value(candidate)
        && candidate.value.is_some()
    {
        parts.push(format!("unusable: {reason:?}"));
    }
    parts.join("; ")
}

/// 指定局面 `position` に対する Book 内の登録状況・評価値・各候補手の Book ラインを標準出力へ表示する。
pub fn show(book: &BookPlayer, position: Position) {
    let Some(entry) = book.probe(position) else {
        println!("Book: position absent");
        return;
    };
    let moves: Vec<_> = entry.moves().collect();
    println!(
        "Book: {} of {} candidates linked; all point-evaluated: {}",
        moves.iter().filter(|m| m.registered).count(),
        moves.len(),
        entry.all_moves_evaluated()
    );
    if let Some(v) = entry.value() {
        println!(
            "Book position value: {}",
            value(v.result(), v.conditions(), v.source())
        );
    }
    if let Some(v) = entry.final_score()
        && entry
            .value()
            .is_none_or(|effective| effective.result() != v.result())
    {
        println!(
            "Book position certificate: {}",
            value(v.result(), v.conditions(), v.source())
        );
    }
    for m in moves {
        let detail = candidate(book, m);
        if m.registered {
            let mut line = vec![m.action];
            if let Ok(next) = position.play(m.action) {
                line.extend(book.continuation(next));
            }
            println!("  {}  {detail}  Book line {}", m.action, display::pv(&line));
        } else {
            println!("  {}  {detail}", m.action);
        }
    }
}
