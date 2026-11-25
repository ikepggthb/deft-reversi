//! Book（`.dbook`）から現在局面の登録手・評価値を照会するモジュール。
//!
//! エンジンスレッドが読み込んだブックを [`SharedBook`] として共有し、
//! 解析パネルや盤面上の Book マーカー表示に利用する。
use super::engine::Reading;
use deft_reversi_book::{Book, Bound, EvaluationRef, Score, UsePolicy};
use deft_reversi_engine::{Action, Position, Selectivity};

/// AI が Book 手を採用するための最低条件（選択率 83% 以上）を返す。
///
/// 「選択率」は探索時の枝刈りの慎重さを表す指標で、値が低いほど浅い読みで候補を絞り込んでいる。
/// AI が実戦で使うには十分な信頼性（83% 以上）を求める一方、解析パネルでは
/// 条件を満たさない手も薄い文字色で参考表示できるようにしている。
pub fn use_policy() -> UsePolicy {
    UsePolicy {
        min_selectivity: Selectivity::Percent83,
        ..UsePolicy::default()
    }
}

/// Book に登録されている 1 手分の情報。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookLine {
    /// 着手（またはパス）。
    pub action: Action,
    /// ブック内でこの手が先へ続く登録手であるか。
    pub registered: bool,
    /// 記録されている評価値。
    pub value: Option<BookValue>,
    /// AI の対局使用ポリシーを満たしているか。
    pub usable: bool,
}

/// Book エントリに記録された評価値とその探索条件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookValue {
    /// 評価値（石差）。
    pub score: i32,
    /// 点値か上下限値かの種別。
    pub bound: Bound,
    /// 探索条件（手動設定やインポート値の場合は `None`）。
    pub reading: Option<Reading>,
}

impl BookValue {
    /// 上下限の幅（「◯石以上」「◯石以下」）ではなく、確定した 1 点の評価値（[`Bound::Point`]）である場合のみ値を返す。
    pub fn point(self) -> Option<i32> {
        (self.bound == Bound::Point).then_some(self.score)
    }
}

/// Book ライブラリ側の [`EvaluationRef`] を UI 表示用の [`BookValue`] へ変換する。
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

/// 指定局面 `position` に対する Book の候補手一覧を評価値の高い順で返す。
///
/// 局面がブック未登録の場合は `None` を返す。
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
    // 評価値を持つ手を先頭に、高い順に並べる。
    lines.sort_by_key(|line| std::cmp::Reverse(line.value.map(|v| v.score)));
    Some(lines)
}

/// エンジンスレッドと UI ビュー間で共有する読み込み済み Book。
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
