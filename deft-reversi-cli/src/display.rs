//! Text shared by the commands: boards, search kinds, principal variations.
use deft_reversi_engine::{
    Action, Color, Position, Progress, ResultKind, SearchResult, Selectivity, Square, StopReason,
};
use std::time::Duration;

pub fn board(position: Position) -> String {
    let moves = position.legal_moves();
    let mut out = String::from("  A B C D E F G H\n");
    for y in 0..8 {
        out.push(char::from(b'1' + y));
        for x in 0..8 {
            let square = Square::from_xy(x, y).unwrap();
            let c = if position.black() & square.bit() != 0 {
                'X'
            } else if position.white() & square.bit() != 0 {
                'O'
            } else if moves.contains(square) {
                '*'
            } else {
                '.'
            };
            out.push(' ');
            out.push(c);
        }
        out.push('\n');
    }
    out
}

pub fn color(color: Color) -> &'static str {
    match color {
        Color::Black => "Black (X)",
        Color::White => "White (O)",
    }
}

/// `d12`, `d12@95%`, `exact` or `end@95%`.
pub fn kind(kind: Option<ResultKind>) -> String {
    let percent = |s: Selectivity| match s {
        Selectivity::Full => String::new(),
        s => format!("@{}%", s.percent()),
    };
    match kind {
        None => "-".into(),
        Some(ResultKind::Depth { depth, selectivity }) => {
            format!("d{depth}{}", percent(selectivity))
        }
        Some(ResultKind::Endgame {
            selectivity: Selectivity::Full,
        }) => "exact".into(),
        Some(ResultKind::Endgame { selectivity }) => format!("end{}", percent(selectivity)),
    }
}

pub fn pv(pv: &[Action]) -> String {
    pv.iter()
        .map(Action::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn action(action: Option<Action>) -> String {
    action.map_or_else(|| "-".into(), |a| a.to_string())
}

pub fn score(score: Option<i32>) -> String {
    score.map_or_else(|| "?".into(), |s| format!("{s:+}"))
}

pub fn nodes(nodes: u64) -> String {
    match nodes {
        0..1_000_000 => nodes.to_string(),
        1_000_000..1_000_000_000 => format!("{:.1}M", nodes as f64 / 1e6),
        _ => format!("{:.2}G", nodes as f64 / 1e9),
    }
}

/// hh:mm:ss.mmm
pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        s / 3600,
        s / 60 % 60,
        s % 60,
        d.subsec_millis()
    )
}

pub fn progress(p: &Progress) -> String {
    format!(
        "  {:<8} {:>4}  {:>8} nodes  {:>7.2}s  pv {}",
        kind(Some(p.kind)),
        format!("{:+}", p.score),
        nodes(p.nodes),
        p.elapsed.as_secs_f64(),
        pv(&p.pv)
    )
}

pub fn stop_note(result: &SearchResult) -> &'static str {
    match result.stop_reason {
        StopReason::Completed => "",
        StopReason::Requested => " (stopped)",
        StopReason::TimeLimit => " (time limit)",
        StopReason::NodeLimit => " (node limit)",
        _ => " (search failed)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(
            kind(Some(ResultKind::Depth {
                depth: 12,
                selectivity: Selectivity::Percent95
            })),
            "d12@95%"
        );
        assert_eq!(
            kind(Some(ResultKind::Endgame {
                selectivity: Selectivity::Full
            })),
            "exact"
        );
        assert_eq!(pv(&["f5".parse().unwrap(), Action::Pass]), "F5 pass");
        assert_eq!(duration(Duration::from_millis(3_723_004)), "01:02:03.004");
        assert_eq!(nodes(12_345_678), "12.3M");
        assert!(board(Position::initial()).contains("4 . . * O X . . ."));
    }
}
