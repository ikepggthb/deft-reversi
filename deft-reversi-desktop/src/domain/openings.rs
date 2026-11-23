//! Opening names (§7.4 of the plan) from the web app's list: each line is
//! `name = F5D6C3…`, indented by depth, and `//` lines are comments. Every
//! line starts with F5, so a game opened elsewhere is turned to F5 first.
use deft_reversi_engine::{Action, Square};
use std::sync::OnceLock;

const LIST: &str = include_str!("../../assets/openings.txt");

struct Opening {
    name: &'static str,
    moves: Vec<Square>,
}

fn openings() -> &'static [Opening] {
    static OPENINGS: OnceLock<Vec<Opening>> = OnceLock::new();
    OPENINGS.get_or_init(|| LIST.lines().filter_map(parse_line).collect())
}

fn parse_line(line: &'static str) -> Option<Opening> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("//") {
        return None;
    }
    let (name, moves) = line.rsplit_once('=')?;
    let moves = moves.trim();
    if moves.len() % 2 != 0 {
        return None;
    }
    let moves = (0..moves.len())
        .step_by(2)
        .map(|i| moves.get(i..i + 2)?.parse::<Square>().ok())
        .collect::<Option<Vec<_>>>()?;
    Some(Opening {
        name: name.trim(),
        moves,
    })
}

/// The symmetry of the start position that takes `first` to F5; each is its
/// own inverse.
fn to_f5(first: Square) -> Option<fn(Square) -> Square> {
    match (first.x(), first.y()) {
        (5, 4) => Some(|s| s),
        // C4: half turn.
        (2, 3) => Some(|s| Square::from_xy(7 - s.x(), 7 - s.y()).expect("on the board")),
        // E6: the main diagonal.
        (4, 5) => Some(|s| Square::from_xy(s.y(), s.x()).expect("on the board")),
        // D3: the other diagonal.
        (3, 2) => Some(|s| Square::from_xy(7 - s.y(), 7 - s.x()).expect("on the board")),
        _ => None,
    }
}

/// The name of the longest listed opening that the game's moves follow from
/// the start. Only games from the standard start have one.
pub fn name(actions: &[Action]) -> Option<&'static str> {
    let Action::Place(first) = *actions.first()? else {
        return None;
    };
    let turn = to_f5(first)?;
    let moves: Vec<Square> = actions
        .iter()
        .map_while(|a| match a {
            Action::Place(s) => Some(turn(*s)),
            Action::Pass => None,
        })
        .collect();
    openings()
        .iter()
        .filter(|o| moves.starts_with(&o.moves))
        .max_by_key(|o| o.moves.len())
        .map(|o| o.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions(moves: &str) -> Vec<Action> {
        (0..moves.len())
            .step_by(2)
            .map(|i| Action::Place(moves[i..i + 2].parse().unwrap()))
            .collect()
    }

    #[test]
    fn names_match_the_longest_line_in_every_orientation() {
        assert!(openings().len() > 250);
        assert_eq!(name(&actions("F5D6")), Some("縦取り"));
        assert_eq!(name(&actions("F5D6C3D3C4")), Some("虎定石"));
        // Moves past the listed line keep its name.
        assert_eq!(name(&actions("F5D6C3D3C4A1")), Some("虎定石"));
        // The same opening turned three ways.
        assert_eq!(name(&actions("C4E3F6E6F5")), Some("虎定石"));
        assert_eq!(name(&actions("E6F4C3C4D3")), Some("虎定石"));
        assert_eq!(name(&actions("D3C5F6F5E6")), Some("虎定石"));
        assert_eq!(name(&[]), None);
    }
}
