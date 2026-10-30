//! Game history: the positions played so far, undo and redo.
use deft_reversi_engine::{Action, MoveError, Position};

pub struct Game {
    /// Every position from the start, including those after a pass.
    positions: Vec<Position>,
    /// `actions[i]` leads from `positions[i]` to `positions[i + 1]`.
    actions: Vec<Action>,
    /// Undone placements, most recent last.
    redo: Vec<Action>,
}

impl Game {
    pub fn new() -> Self {
        Self {
            positions: vec![Position::initial()],
            actions: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn current(&self) -> Position {
        *self.positions.last().unwrap()
    }

    /// Plays `action` and any pass forced after it.
    pub fn play(&mut self, action: Action) -> Result<(), MoveError> {
        self.push(action)?;
        self.redo.clear();
        Ok(())
    }

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

    /// Plays a record such as `f5d6c3`. On an illegal move the game is left
    /// as it was.
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
                self.positions.truncate(saved.0);
                self.actions.truncate(saved.0 - 1);
                self.redo = saved.1;
                return Err(e);
            }
        }
        Ok(())
    }

    /// Takes back the last placement and the passes around it.
    pub fn undo(&mut self) -> bool {
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

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(action) => self.push(action).is_ok(),
            None => false,
        }
    }

    fn pop(&mut self) -> Option<Action> {
        let action = self.actions.pop()?;
        self.positions.pop();
        Some(action)
    }

    /// The placements so far, such as `f5d6c3`.
    pub fn record(&self) -> String {
        self.actions
            .iter()
            .filter(|a| **a != Action::Pass)
            .map(|a| a.to_string().to_ascii_lowercase())
            .collect()
    }

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
