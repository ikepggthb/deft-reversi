//! CLI display for recorded Book evaluations and lines.
use crate::display;
use deft_reversi_book::{
    Book, BookMove, BookPlayer, EvaluationSource, MovePolicy, ReadLimits, Score, SearchConditions,
    UsePolicy,
};
use deft_reversi_engine::{Engine, Position, Selectivity};
use std::{path::Path, sync::Arc};

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
