//! A game as a line of squares, such as `f5d6c3d3c4`: copied out of the
//! app and typed or pasted back in. Passes are not written; they follow
//! from the moves.
use super::{Grouping, History};
use deft_reversi_engine::{Action, Position, Square};

/// The moves of the whole record, passes left out.
pub fn to_text(actions: &[Action]) -> String {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Place(square) => Some(square.to_string().to_lowercase()),
            Action::Pass => None,
        })
        .collect()
}

/// A game from the standard start. Anything but letters and digits is
/// ignored, so spaces and separators may be pasted along. On failure, the
/// number of the first move that cannot be played.
pub fn parse(text: &str) -> Result<History, usize> {
    let chars: Vec<char> = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let mut history = History::new(Position::initial());
    for (i, pair) in chars.chunks(2).enumerate() {
        let square = pair.iter().collect::<String>().parse::<Square>().ok();
        let played = square.is_some_and(|square| {
            history
                .play(Action::Place(square), Grouping::NewGroup)
                .is_ok()
        });
        if !played {
            return Err(i + 1);
        }
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_go_out_and_come_back() {
        let history = parse("F5 d6-c3 d3c4").unwrap();
        assert_eq!(history.actions().len(), 5);
        assert_eq!(to_text(history.actions()), "f5d6c3d3c4");
        assert_eq!(parse("f5f5").unwrap_err(), 2);
        assert_eq!(parse("f5d").unwrap_err(), 2);
        assert_eq!(parse("z9").unwrap_err(), 1);
        assert!(parse("").unwrap().actions().is_empty());
    }
}
