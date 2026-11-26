//! 1 局分の着手列・各手後の局面列・現在表示位置（カーソル）・Undo/Redo グループを管理するモジュール。
use deft_reversi_engine::{Action, MoveError, Position, Square};

/// 着手を追加する際、直前の Undo 単位（「元に戻す」1 回で戻る範囲）にまとめるかどうかの指定。
///
/// 例えば人間対 AI 戦では、人間が 1 手打った後に AI が 1 手応手するため、
/// 「元に戻す」1 回で自分の手番まで戻れるよう、AI の応手は `Join` で前の手に結合する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouping {
    /// 新しい Undo ステップを開始する（通常の人間・AI の着手）。
    NewGroup,
    /// 直前のステップに結合する（連続する AI の着手や強制パスなど）。
    Join,
}

/// 1 局分の棋譜履歴。
///
/// 常に `positions[i + 1] == positions[i].play(actions[i])` が成り立つことを不変条件として保つ。
/// `positions[cursor]` が現在画面に表示している局面であり、`actions[cursor..]` は「やり直し（Redo）」用の未来の手順を保持している。
/// リバーシのルール上、合法手がない場合は強制的にパスとなるため、強制パスはそれを引き起こした着手の直後に自動で記録される。
#[derive(Clone, Debug)]
pub struct History {
    positions: Vec<Position>,
    actions: Vec<Action>,
    /// 各 Undo ステップが開始する `actions` 内のインデックス（狭義単調増加）。
    groups: Vec<usize>,
    cursor: usize,
}

impl History {
    /// 指定された初期局面から新しい履歴を作成する。
    ///
    /// 開始時点で手番側に合法手がない場合は即座に強制パスを記録する。
    pub fn new(initial: Position) -> Self {
        let mut history = Self {
            positions: vec![initial],
            actions: Vec::new(),
            groups: Vec::new(),
            cursor: 0,
        };
        history.pass_if_forced();
        history
    }

    /// 保存された棋譜データから全手順をリプレイして [`History`] を復元する。
    ///
    /// 非合法手・不要または欠落したパス・不正なグループ境界やカーソル位置を検出した場合はエラーを返す。
    pub fn restore(
        initial: Position,
        actions: Vec<Action>,
        groups: Vec<usize>,
        cursor: usize,
    ) -> Result<Self, &'static str> {
        let mut positions = Vec::with_capacity(actions.len() + 1);
        positions.push(initial);
        for &action in &actions {
            let position = *positions.last().expect("not empty");
            let forced = !position.is_terminal() && position.must_pass();
            if forced != (action == Action::Pass) {
                return Err("a pass where a move was possible, or a missing pass");
            }
            positions.push(position.play(action).map_err(|_| "an illegal move")?);
        }
        let last = *positions.last().expect("not empty");
        if !last.is_terminal() && last.must_pass() {
            return Err("the record ends before a forced pass");
        }
        let ascending = groups.windows(2).all(|w| w[0] < w[1]);
        let in_range = groups.iter().all(|&g| g < actions.len());
        let starts = actions.is_empty() || groups.first() == Some(&0);
        if !ascending || !in_range || !starts {
            return Err("bad undo groups");
        }
        if cursor > actions.len() {
            return Err("the cursor is past the record");
        }
        Ok(Self {
            positions,
            actions,
            groups,
            cursor,
        })
    }

    /// Undo グループの開始インデックス一覧を返す。
    pub fn groups(&self) -> &[usize] {
        &self.groups
    }

    /// 現在カーソルが指している局面を返す。
    pub fn current(&self) -> Position {
        self.positions[self.cursor]
    }

    /// `index` 手進んだ後の局面を返す。
    pub fn position(&self, index: usize) -> Position {
        self.positions[index]
    }

    /// 現在のカーソル位置（`actions` 内の手数）を返す。
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 記録されている全着手列を返す。
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    /// 現在表示中の盤面における直近の着手マスを返す（強制パスを挟む場合はその手前の着手を探す）。
    pub fn last_move(&self) -> Option<Square> {
        self.actions[..self.cursor]
            .iter()
            .rev()
            .take(2)
            .find_map(|action| match action {
                Action::Place(square) => Some(*square),
                Action::Pass => None,
            })
    }

    /// カーソル位置で手を打ち、それ以降の Redo 履歴を切り捨てて、必要なら強制パスを追記する。
    pub fn play(&mut self, action: Action, grouping: Grouping) -> Result<(), MoveError> {
        let next = self.current().play(action)?;
        self.actions.truncate(self.cursor);
        self.positions.truncate(self.cursor + 1);
        let cursor = self.cursor;
        self.groups.retain(|&start| start < cursor);
        self.push(action, next, grouping);
        self.pass_if_forced();
        Ok(())
    }

    /// 終局しておらず手番側に合法手がない場合、強制パスを現在のステップに結合して追加する。
    fn pass_if_forced(&mut self) {
        let position = self.current();
        if !position.is_terminal() && position.must_pass() {
            let next = position.play(Action::Pass).expect("a forced pass is legal");
            self.push(Action::Pass, next, Grouping::Join);
        }
    }

    /// 1 手分の着手と結果局面を末尾に追加し、カーソルを最新位置へ進める。
    fn push(&mut self, action: Action, next: Position, grouping: Grouping) {
        if grouping == Grouping::NewGroup || self.groups.is_empty() {
            self.groups.push(self.actions.len());
        }
        self.actions.push(action);
        self.positions.push(next);
        self.cursor = self.actions.len();
    }

    /// カーソルを直前の Undo ステップの開始位置まで戻す。
    pub fn undo(&mut self) -> bool {
        match self.groups.iter().rev().find(|&&start| start < self.cursor) {
            Some(&start) => {
                self.cursor = start;
                true
            }
            None => false,
        }
    }

    /// カーソルを次の Undo ステップの終了位置まで進める。
    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        self.cursor = self
            .groups
            .iter()
            .find(|&&start| start > self.cursor)
            .copied()
            .unwrap_or(self.actions.len());
        true
    }

    /// 指定手数だけカーソルを前後に移動する。
    ///
    /// パス直後の局面はパス前と盤面の石配置が同じであるため、棋譜の末尾（対局続行位置）を除き
    /// 1 回のステップでパスを飛び越えて次の着手位置まで移動する。
    pub fn step(&mut self, steps: isize) -> bool {
        let start = self.cursor;
        for _ in 0..steps.unsigned_abs() {
            if steps < 0 {
                while self.cursor > 0 {
                    self.cursor -= 1;
                    if !self.after_pass(self.cursor) {
                        break;
                    }
                }
            } else {
                while self.cursor < self.actions.len() {
                    self.cursor += 1;
                    if !self.after_pass(self.cursor) || self.cursor == self.actions.len() {
                        break;
                    }
                }
            }
        }
        self.cursor != start
    }

    fn after_pass(&self, index: usize) -> bool {
        index > 0 && self.actions[index - 1] == Action::Pass
    }

    /// 履歴を変更せず、カーソル直前に記録済みの強制パスを 1 手分だけ進める。
    pub fn advance_pass(&mut self) -> bool {
        if self.actions.get(self.cursor) != Some(&Action::Pass) {
            return false;
        }
        self.cursor += 1;
        true
    }

    /// カーソルを指定インデックスへ直接移動する（範囲外は末尾にクランプ）。
    pub fn go_to(&mut self, index: usize) -> bool {
        let index = index.min(self.actions.len());
        let moved = index != self.cursor;
        self.cursor = index;
        moved
    }

    /// Redo 可能な未来の手順が残っているかを返す。
    pub fn can_redo(&self) -> bool {
        self.cursor < self.actions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::Color;

    fn square(name: &str) -> Square {
        name.parse().unwrap()
    }

    fn place(name: &str) -> Action {
        Action::Place(square(name))
    }

    fn bits(names: &[&str]) -> u64 {
        names.iter().fold(0, |acc, name| acc | square(name).bit())
    }

    fn played(moves: &[&str]) -> History {
        let mut history = History::new(Position::initial());
        for m in moves {
            history.play(place(m), Grouping::NewGroup).unwrap();
            check_invariants(&history);
        }
        history
    }

    fn check_invariants(history: &History) {
        assert_eq!(history.positions.len(), history.actions.len() + 1);
        for (i, action) in history.actions.iter().enumerate() {
            assert_eq!(
                history.positions[i].play(*action),
                Ok(history.positions[i + 1])
            );
        }
        assert!(history.cursor <= history.actions.len());
        assert!(history.groups.windows(2).all(|w| w[0] < w[1]));
        assert!(history.groups.iter().all(|&g| g < history.actions.len()));
        let last = history.positions[history.actions.len()];
        assert!(last.is_terminal() || !last.must_pass());
    }

    #[test]
    fn illegal_move_changes_nothing() {
        let mut history = played(&["f5"]);
        assert!(history.play(place("a1"), Grouping::NewGroup).is_err());
        assert_eq!(history.cursor(), 1);
        assert_eq!(history.actions().len(), 1);
    }

    #[test]
    fn undo_redo_and_truncate() {
        let mut history = played(&["f5", "d6", "c3"]);
        assert!(history.undo());
        assert_eq!(history.cursor(), 2);
        assert!(history.undo());
        assert!(history.redo());
        assert_eq!(history.cursor(), 2);
        assert!(history.redo());
        assert!(!history.redo());

        history.go_to(1);
        history.play(place("f4"), Grouping::NewGroup).unwrap();
        check_invariants(&history);
        assert_eq!(history.actions().len(), 2);
        assert!(!history.can_redo());
        assert!(history.undo() && history.undo());
        assert!(!history.undo());
        assert_eq!(history.cursor(), 0);
    }

    #[test]
    fn joined_actions_undo_together() {
        let mut history = played(&["f5"]);
        history.play(place("d6"), Grouping::Join).unwrap();
        assert!(history.undo());
        assert_eq!(history.cursor(), 0);
        assert!(history.redo());
        assert_eq!(history.cursor(), 2);
    }

    #[test]
    fn no_action_after_game_over() {
        // Taking the only white disc ends the game.
        let start = Position::from_colors(bits(&["d4"]), bits(&["e4"]), Color::Black).unwrap();
        let mut history = History::new(start);
        history.play(place("f4"), Grouping::NewGroup).unwrap();
        assert!(history.current().is_terminal());
        assert_eq!(history.actions(), &[place("f4")]);
    }

    /// White to move with no move; Black can play C1 or A3.
    fn white_must_pass() -> Position {
        let start =
            Position::from_colors(bits(&["a1", "d1"]), bits(&["b1", "a2"]), Color::White).unwrap();
        assert!(start.must_pass() && !start.is_terminal());
        start
    }

    #[test]
    fn forced_passes_are_recorded() {
        let mut history = History::new(white_must_pass());
        assert_eq!(history.actions(), &[Action::Pass]);
        assert_eq!(history.cursor(), 1);
        assert!(history.current().side_to_move() == Color::Black);

        // After C1 White still cannot move.
        history.play(place("c1"), Grouping::NewGroup).unwrap();
        check_invariants(&history);
        assert_eq!(
            history.actions(),
            &[Action::Pass, place("c1"), Action::Pass]
        );
        assert_eq!(history.last_move(), Some(square("c1")));

        // The pass joins C1's Undo step.
        assert!(history.undo());
        assert_eq!(history.cursor(), 1);
    }

    #[test]
    fn steps_go_past_positions_after_a_pass() {
        let mut history = History::new(white_must_pass());
        history.play(place("c1"), Grouping::NewGroup).unwrap();
        // 0 start, 1 after pass, 2 after C1, 3 after pass (the end).
        assert_eq!(history.cursor(), 3);
        assert!(history.step(-1));
        assert_eq!(history.cursor(), 2);
        assert!(history.step(-1));
        assert_eq!(history.cursor(), 0);
        assert!(!history.step(-1));
        assert!(history.step(1));
        assert_eq!(history.cursor(), 2);
        assert!(history.step(1));
        assert_eq!(history.cursor(), 3);
        assert!(!history.step(1));
        history.go_to(0);
        assert!(history.step(2));
        assert_eq!(history.cursor(), 3);
    }
}
