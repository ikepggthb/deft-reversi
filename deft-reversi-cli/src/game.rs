//! 対局の局面履歴を管理し、待った（undo）とやり直し（redo）を提供するモジュール。
//!
//! 強制パスは直前の着手とまとめて記録・巻き戻しされるため、
//! 対局者や呼び出し側は石を置く手だけを意識すればよい。

use deft_reversi_engine::{Action, MoveError, Position};

/// 1 局分の局面履歴と待った／やり直し用の着手履歴を保持する構造体。
pub struct Game {
    /// 開始局面から現在までの全局面（パス後の局面も含む）。
    positions: Vec<Position>,
    /// `actions[i]` を進めると `positions[i]` から `positions[i + 1]` へ遷移する。
    actions: Vec<Action>,
    /// 取り消された着手の履歴（最後に待ったをした手が末尾）。
    redo: Vec<Action>,
}

impl Game {
    /// 平手初期局面から始まる新しい対局履歴を作成する。
    pub fn new() -> Self {
        Self::from_position(Position::initial())
    }

    /// 指定された開始局面 `position` から始まる対局履歴を作成する。
    pub fn from_position(position: Position) -> Self {
        Self {
            positions: vec![position],
            actions: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// 対局の開始局面を返す。
    pub fn start(&self) -> Position {
        self.positions[0]
    }

    /// 現在の局面を返す。
    pub fn current(&self) -> Position {
        *self.positions.last().unwrap()
    }

    /// `action` を進め、その直後に強制パスが必要なら自動でパスも進める。
    ///
    /// 新しい着手を指したため、やり直し履歴（`redo`）は消去される。
    pub fn play(&mut self, action: Action) -> Result<(), MoveError> {
        self.push(action)?;
        self.redo.clear();
        Ok(())
    }

    /// `action` と、直後に発生する強制パスを履歴へ積む内部ヘルパー。
    fn push(&mut self, action: Action) -> Result<(), MoveError> {
        let next = self.current().play(action)?;
        self.positions.push(next);
        self.actions.push(action);
        if next.must_pass() {
            self.positions.push(next.play(Action::Pass)?);
            self.actions.push(Action::Pass);
        }
        Ok(())
    }

    /// `f5d6c3` のような連続座標文字列の棋譜を順に再生する。
    ///
    /// 途中に不正な手が含まれていた場合は、エラーを返して再生前の状態へ完全に戻す。
    pub fn play_record(&mut self, record: &str) -> Result<(), String> {
        let bytes = record.as_bytes();
        if !bytes.len().is_multiple_of(2) || !record.is_ascii() {
            return Err(format!("not a record of squares: {record}"));
        }
        let saved = (self.positions.len(), self.redo.clone());
        for chunk in record.as_bytes().chunks(2) {
            let text = std::str::from_utf8(chunk).unwrap();
            let played = text
                .parse::<Action>()
                .map_err(|e| format!("{text}: {e}"))
                .and_then(|a| self.play(a).map_err(|e| format!("{text}: {e}")));
            if let Err(e) = played {
                // 途中まで積んだ局面と着手、および消去されたやり直し履歴を元に戻す。
                self.positions.truncate(saved.0);
                self.actions.truncate(saved.0 - 1);
                self.redo = saved.1;
                return Err(e);
            }
        }
        Ok(())
    }

    /// 直前の着手と、その前後に含まれる強制パスをまとめて 1 手分取り消す。
    pub fn undo(&mut self) -> bool {
        if self.actions.iter().all(|a| *a == Action::Pass) {
            return false;
        }
        // 着手後に自動追加された強制パスを先に取り除く。
        while self.actions.last() == Some(&Action::Pass) {
            self.pop();
        }
        match self.pop() {
            Some(action) => {
                self.redo.push(action);
                true
            }
            None => false,
        }
    }

    /// 直前に取り消した着手を 1 手分やり直す。
    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(action) => self.push(action).is_ok(),
            None => false,
        }
    }

    /// 履歴の末尾から 1 手分（局面と行動）を取り出す内部ヘルパー。
    fn pop(&mut self) -> Option<Action> {
        let action = self.actions.pop()?;
        self.positions.pop();
        Some(action)
    }

    /// 開始局面から現在までの `(着手前の局面, 行動)` を順番に返すイテレータ。
    pub fn steps(&self) -> impl Iterator<Item = (Position, Action)> + '_ {
        self.positions
            .iter()
            .copied()
            .zip(self.actions.iter().copied())
    }

    /// これまでの着手をパスを除いた小文字の連続座標文字列（例: `f5d6c3`）で返す。
    pub fn record(&self) -> String {
        self.actions
            .iter()
            .filter(|a| **a != Action::Pass)
            .map(|a| a.to_string().to_ascii_lowercase())
            .collect()
    }

    /// これまでに打たれた着手数（パスを除く）を返す。
    pub fn placements(&self) -> usize {
        self.actions.iter().filter(|a| **a != Action::Pass).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_undo_and_redo() {
        let mut game = Game::new();
        game.play_record("f5d6c3").unwrap();
        assert_eq!(game.record(), "f5d6c3");
        assert!(game.undo());
        assert_eq!(game.record(), "f5d6");
        assert!(game.redo());
        assert_eq!(game.record(), "f5d6c3");
        assert!(!game.redo());
        assert!(game.play_record("f5").is_err());
        assert_eq!(game.record(), "f5d6c3");
        while game.undo() {}
        assert_eq!(game.current(), Position::initial());
    }

    #[test]
    fn forced_passes_are_played_and_undone_with_the_move() {
        use rand::{Rng, SeedableRng, rngs::StdRng};
        let mut rng = StdRng::seed_from_u64(7);
        let game = loop {
            let mut game = Game::new();
            while !game.current().is_terminal() {
                let moves: Vec<_> = game.current().legal_moves().collect();
                game.play(Action::Place(moves[rng.gen_range(0..moves.len())]))
                    .unwrap();
                assert!(!game.current().must_pass());
                if game.actions.last() == Some(&Action::Pass) {
                    break;
                }
            }
            if game.actions.last() == Some(&Action::Pass) {
                break game;
            }
        };
        let mut undone = Game::new();
        let record = game.record();
        undone.play_record(&record[..record.len() - 2]).unwrap();
        let mut game = game;
        assert!(game.undo());
        assert_eq!(game.current(), undone.current());
        assert_eq!(game.actions.len(), game.positions.len() - 1);
    }
}
