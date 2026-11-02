//! The controller: commands in, state changes and effects out. It does not
//! touch GPUI, so it is tested without a window.
use crate::domain::{Grouping, History};
use deft_reversi_engine::{Action, Position, Square};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    NewGame,
    Play(Square),
    Undo,
    Redo,
    /// Moves back (negative) or forward through the record.
    Step(isize),
    First,
    Last,
    /// Shows the position after that many actions.
    GoTo(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Place,
    Pass,
    GameOver,
}

/// Work for the services; sounds are played from P6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Sound(Sound),
}

pub struct Controller {
    history: History,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            history: History::new(Position::initial()),
        }
    }
}

impl Controller {
    pub fn history(&self) -> &History {
        &self.history
    }

    /// Returns no effects when the command changes nothing.
    pub fn apply(&mut self, command: Command) -> Vec<Effect> {
        let history = &mut self.history;
        match command {
            Command::NewGame => *history = History::new(Position::initial()),
            Command::Play(square) => {
                if history
                    .play(Action::Place(square), Grouping::NewGroup)
                    .is_err()
                {
                    return Vec::new();
                }
                // The move is the last action unless it forced a pass.
                return vec![Effect::Sound(if history.current().is_terminal() {
                    Sound::GameOver
                } else if history.actions().last() == Some(&Action::Pass) {
                    Sound::Pass
                } else {
                    Sound::Place
                })];
            }
            Command::Undo => {
                history.undo();
            }
            Command::Redo => {
                history.redo();
            }
            Command::Step(steps) => {
                history.step(steps);
            }
            Command::First => {
                history.go_to(0);
            }
            Command::Last => {
                history.go_to(usize::MAX);
            }
            Command::GoTo(index) => {
                history.go_to(index);
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(controller: &mut Controller, name: &str) -> Vec<Effect> {
        controller.apply(Command::Play(name.parse().unwrap()))
    }

    #[test]
    fn local_game_with_undo_and_navigation() {
        let mut c = Controller::default();
        assert_eq!(play(&mut c, "f5"), vec![Effect::Sound(Sound::Place)]);
        assert!(play(&mut c, "a1").is_empty());
        play(&mut c, "d6");
        play(&mut c, "c3");
        c.apply(Command::Undo);
        assert_eq!(c.history().cursor(), 2);
        c.apply(Command::First);
        assert_eq!(c.history().cursor(), 0);
        c.apply(Command::Step(2));
        assert_eq!(c.history().cursor(), 2);
        c.apply(Command::Last);
        assert_eq!(c.history().cursor(), 3);
        c.apply(Command::GoTo(1));
        play(&mut c, "f4");
        assert_eq!(c.history().actions().len(), 2);
        c.apply(Command::NewGame);
        assert_eq!(c.history().actions().len(), 0);
    }
}
