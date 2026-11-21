//! What the opening book knows of a position, for the analysis panel and
//! the board (§24, B2). The engine thread loads the book and shares it.
use super::engine::Reading;
use deft_reversi_book::{Book, Bound, EvaluationRef, Score, UsePolicy};
use deft_reversi_engine::{Action, Position, Selectivity};

/// The conditions a book value must meet to be played; the analysis shows
/// the others faintly.
pub fn use_policy() -> UsePolicy {
    UsePolicy {
        min_selectivity: Selectivity::Percent83,
        ..UsePolicy::default()
    }
}

/// One move the book has for a position, from the side to move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookLine {
    pub action: Action,
    /// The book goes on with this move.
    pub registered: bool,
    pub value: Option<BookValue>,
    /// The AI would play from this value.
    pub usable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookValue {
    pub score: i32,
    /// Only a bound: at least or at most `score`.
    pub bound: Bound,
    /// How the value was read; None for a value set by hand or imported.
    pub reading: Option<Reading>,
}

impl BookValue {
    /// The score alone, when it is one.
    pub fn point(self) -> Option<i32> {
        (self.bound == Bound::Point).then_some(self.score)
    }
}

fn value(v: EvaluationRef<'_>) -> BookValue {
    let conditions = v.conditions();
    let reading = match (
        v.result(),
        conditions.endgame_selectivity(),
        conditions.midgame_depth(),
        conditions.midgame_selectivity(),
    ) {
        (Score::Final(range), ..) if range.exact().is_some() => {
            Some(Reading::Endgame { percent: 100 })
        }
        (_, Some(selectivity), ..) => Some(Reading::Endgame {
            percent: selectivity.percent(),
        }),
        (_, None, Some(depth), Some(selectivity)) => Some(Reading::Depth {
            depth,
            percent: selectivity.percent(),
        }),
        _ => None,
    };
    BookValue {
        score: i32::from(v.result().value()),
        bound: v.result().bound(),
        reading,
    }
}

/// The book's moves for `position`, best first; None when it is not in
/// the book.
pub fn lines(book: &Book, position: Position) -> Option<Vec<BookLine>> {
    let entry = book.probe(position.board())?;
    let policy = use_policy();
    let mut lines: Vec<BookLine> = entry
        .moves()
        .map(|m| {
            let shown = m.value.or(m.final_score);
            BookLine {
                action: m.action,
                registered: m.registered,
                value: shown.map(value),
                usable: m.value.is_some_and(|v| {
                    v.result().point_value().is_some()
                        && policy.check_ref(v, book.identity()).is_ok()
                }),
            }
        })
        .collect();
    // Moves with a value first, the best of them first.
    lines.sort_by_key(|line| std::cmp::Reverse(line.value.map_or(i32::MIN, |v| v.score)));
    Some(lines)
}

/// The loaded book, shared by the engine thread and the views.
#[derive(Clone)]
pub struct SharedBook(pub std::sync::Arc<Book>);

impl std::fmt::Debug for SharedBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SharedBook({} positions)", self.0.stats().positions)
    }
}

impl PartialEq for SharedBook {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedBook {}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_book::{BookEditor, RequestedAnalysis, SearchKind};
    use deft_reversi_engine::{Engine, EngineOptions, Goal, Model, PatternModel, SearchRequest};

    #[test]
    fn a_registered_move_shows_with_its_reading() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let start = Position::initial();
        let board = start.board().canonical();
        let request = SearchRequest::new(Goal::Depth(2));
        let planned = SearchKind::from_result(engine.search_plan(board, &request).unwrap());
        let result = engine.search(board, &request).unwrap();
        let mut editor = BookEditor::for_engine(&engine).unwrap();
        editor
            .record_search(
                board,
                RequestedAnalysis::ParentDepth {
                    depth: 2,
                    selectivity: Selectivity::Full,
                },
                planned,
                &result,
            )
            .unwrap();
        editor
            .register_move(board, result.best_move.unwrap())
            .unwrap();
        let book = editor.export().unwrap();

        let lines = lines(&book, start).expect("the start is in the book");
        let first = &lines[0];
        assert!(first.registered);
        assert!(
            start
                .legal_moves()
                .any(|s| Action::Place(s) == first.action)
        );
        let value = first.value.expect("a searched value");
        assert_eq!(value.point(), Some(value.score));
        assert_eq!(
            value.reading,
            Some(Reading::Depth {
                depth: 2,
                percent: 100
            })
        );
        // Depth 2 is below what the AI plays from.
        assert!(!first.usable);
    }
}
