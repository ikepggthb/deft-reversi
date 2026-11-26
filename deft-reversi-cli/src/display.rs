//! 各コマンドで共通して使う表示用文字列の整形モジュール。
//!
//! 盤面図、手番、探索種別、PV（予想手順）、評価値、ノード数、経過時間、停止理由を整形する。

use deft_reversi_engine::{
    Action, Color, Position, Progress, ResultKind, SearchResult, Selectivity, Square, StopReason,
};
use std::time::Duration;

/// 現在の盤面と合法手（`*`）を A1〜H8 の座標付き ASCII 文字列に整形する。
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

/// 手番の色を石記号付きの文字列（`Black (X)` または `White (O)`）で返す。
pub fn color(color: Color) -> &'static str {
    match color {
        Color::Black => "Black (X)",
        Color::White => "White (O)",
    }
}

/// 探索結果の種類（深さと選択率）を `d12`、`d12@95%`、`exact`、`end@95%` の形式で返す。
///
/// 選択率（MPC＝Multi-ProbCut の信頼度）が 100% の終盤探索は `exact`（完全読み）になる。
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

/// PV（Principal Variation＝双方最善を仮定した予想手順）を空白区切りの文字列にする。
pub fn pv(pv: &[Action]) -> String {
    pv.iter()
        .map(Action::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// PV の先頭 `limit` 手だけを取り出し、残りの手がある場合は末尾に ` ...` を付ける。
pub fn pv_prefix(line: &[Action], limit: usize) -> String {
    let mut preview = pv(&line[..line.len().min(limit)]);
    if line.len() > limit {
        preview.push_str(" ...");
    }
    preview
}

/// 着手（またはパス）を文字列にする。`None` のときは `-` を返す。
pub fn action(action: Option<Action>) -> String {
    action.map_or_else(|| "-".into(), |a| a.to_string())
}

/// 評価値（石差）を符号付き文字列にする。`None` のときは `?` を返す。
pub fn score(score: Option<i32>) -> String {
    score.map_or_else(|| "?".into(), |s| format!("{s:+}"))
}

/// 探索ノード数を読みやすい単位（`12.3M`、`1.23G` など）に整形する。
pub fn nodes(nodes: u64) -> String {
    match nodes {
        0..1_000_000 => nodes.to_string(),
        1_000_000..1_000_000_000 => format!("{:.1}M", nodes as f64 / 1e6),
        _ => format!("{:.2}G", nodes as f64 / 1e9),
    }
}

/// 経過時間を `hh:mm:ss.mmm` 形式の文字列にする。
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

/// 反復深化の途中経過 1 回分を 1 行の表示文字列に整形する。
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

/// 探索が通常完了以外で止まった場合に、行末へ添える注記文字列を返す。
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
