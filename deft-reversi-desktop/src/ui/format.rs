//! デスクトップ UI 全体で共有するテキスト整形ヘルパー関数群。
use crate::{domain::Strength, i18n::Strings, services::engine::Reading};
use deft_reversi_engine::Action;
use gpui::SharedString;

/// 着手を小文字の座標文字列（例: `"f5"`, `"pass"`）へ整形する。
pub(super) fn action_text(action: Action) -> String {
    action.to_string().to_lowercase()
}

/// 読みの深さ・精度情報（[`Reading`]）をパネル表示用の文字列へ整形する。
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

/// 読みの深さ・精度を盤面上の狭いスペース向けの短縮表記（例: `"d24@83%"`, `"END@95%"`, `"END@100%"`, `"Book"`）へ整形する。
pub(super) fn reading_mark(s: &Strings, reading: Reading) -> String {
    match reading {
        Reading::Depth { depth, percent } => format!("d{depth}@{percent}%"),
        Reading::Endgame { percent } => format!("END@{percent}%"),
        Reading::Book => s.book.into(),
    }
}

/// 探索ノード数を `K` / `M` / `G` 単位の読みやすい文字列へ整形する。
pub(super) fn nodes_text(nodes: u64) -> String {
    match nodes {
        n if n >= 1_000_000_000 => format!("{:.1} G", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.1} M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.0} K", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// 符号付き整数を `"+6"` / `"−6"` / `"0"` 形式（マイナスは全角マイナス記号）へ整形する。
pub(super) fn signed(value: i32) -> String {
    format!("{value:+}").replace('-', "−").replace("+0", "0")
}

/// AI の強さ設定を短いラベル（プリセット名・`"Lv N"`・`"N 秒"`）へ整形する。
pub(super) fn strength_label(s: &Strings, strength: Strength) -> SharedString {
    match strength {
        Strength::Preset(preset) => s.preset(preset).into(),
        Strength::Level(level) => format!("Lv {level}").into(),
        Strength::TimePerMove(limit) => s.seconds(limit.as_secs()).into(),
    }
}

/// 読み筋（PV）の 2 手目以降を空白区切りの文字列へ整形する。
pub(super) fn pv_tail(pv: &[Action]) -> String {
    pv.iter()
        .skip(1)
        .map(|&a| action_text(a))
        .collect::<Vec<_>>()
        .join(" ")
}
