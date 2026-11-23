//! Shared text formatting helpers for the desktop UI.
use crate::{domain::Strength, i18n::Strings, services::engine::Reading};
use deft_reversi_engine::Action;
use gpui::SharedString;

pub(super) fn action_text(action: Action) -> String {
    action.to_string().to_lowercase()
}

pub(super) fn reading_text(s: &Strings, reading: Reading) -> String {
    match reading {
        Reading::Depth {
            depth,
            percent: 100,
        } => s.depth(depth),
        Reading::Depth { depth, percent } => format!("{} @ {percent}%", s.depth(depth)),
        Reading::Endgame { percent: 100 } => s.exact.into(),
        Reading::Endgame { percent } => format!("{} @ {percent}%", s.selective_endgame),
        Reading::Book => s.book.into(),
    }
}

/// How a value was read, in a few characters: "d24@83%", "END@95%",
/// "END@100%" or "Book".
pub(super) fn reading_mark(s: &Strings, reading: Reading) -> String {
    match reading {
        Reading::Depth { depth, percent } => format!("d{depth}@{percent}%"),
        Reading::Endgame { percent } => format!("END@{percent}%"),
        Reading::Book => s.book.into(),
    }
}

pub(super) fn nodes_text(nodes: u64) -> String {
    match nodes {
        n if n >= 1_000_000_000 => format!("{:.1} G", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.1} M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.0} K", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// "+6", "−6" or "0".
pub(super) fn signed(value: i32) -> String {
    format!("{value:+}").replace('-', "−").replace("+0", "0")
}

/// How a strength reads in a short label: a preset's name, a level, or a
/// time per move.
pub(super) fn strength_label(s: &Strings, strength: Strength) -> SharedString {
    match strength {
        Strength::Preset(preset) => s.preset(preset).into(),
        Strength::Level(level) => format!("Lv {level}").into(),
        Strength::TimePerMove(limit) => s.seconds(limit.as_secs()).into(),
    }
}

/// The principal variation after the first move, space-separated.
pub(super) fn pv_tail(pv: &[Action]) -> String {
    pv.iter()
        .skip(1)
        .map(|&a| action_text(a))
        .collect::<Vec<_>>()
        .join(" ")
}
