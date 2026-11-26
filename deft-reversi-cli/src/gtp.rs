//! CLI ベースの自動対局向けに GTP（Go Text Protocol v2）を提供するモジュール。
//!
//! 固定レベル探索（`--level`、`--endgame` で終盤の段階を上書きできる）または持ち時間制（`--game-ms` / `time_settings` / `time_left`）で動作し、
//! 8x8 盤面・コミ 0・切れ負けルールのコマンドを受け付ける。

use crate::{
    EngineArgs, Result,
    game_clock::{self, Budget},
    stop,
};
use clap::Args;
use deft_reversi_engine::{
    Action, Color, EngineError, Goal, Position, ResultKind, SearchRequest, SearchResult, StopReason,
};
use std::{
    io::{self, BufRead, Write},
    time::{Duration, Instant},
};

/// GTP セッションの時間／探索制御モード。
enum Mode {
    /// 固定エンジンレベル（1〜60。終盤の段階を上書きした探索計画を含む）で探索するモード。
    Level(Goal),
    /// 黒番・白番の残り持ち時間に応じて時間配分するモード。
    Time {
        /// 黒番（添字 0）と白番（添字 1）の残り時間。
        remaining: [Duration; 2],
    },
}

/// 手番の色を `remaining` 配列の添字（黒: 0、白: 1）へ変換する。
fn clock(side: Color) -> usize {
    match side {
        Color::Black => 0,
        Color::White => 1,
    }
}

/// GTP の秒数引数を [`Duration`] に変換する。負値・無限大・NaN は構文エラーとする。
fn seconds(text: Option<&&str>) -> std::result::Result<Duration, String> {
    text.and_then(|s| s.parse::<f64>().ok())
        .and_then(|s| Duration::try_from_secs_f64(s).ok())
        .ok_or_else(|| "syntax error".into())
}

/// GTP の色指定文字列（`b`/`black`/`w`/`white`、大文字小文字無視）を [`Color`] に変換する。
fn color(s: &str) -> Option<Color> {
    match s.to_ascii_lowercase().as_str() {
        "b" | "black" => Some(Color::Black),
        "w" | "white" => Some(Color::White),
        _ => None,
    }
}

/// `side` の着手 `action` を `pos` に適用する。
///
/// GTP クライアントは相手側の強制パスを送信しない場合があるため、
/// 手番が異なっていて相手側がパス必須であれば先にパスを 1 回補ってから適用する。
fn apply(pos: &mut Position, side: Color, action: Action) -> std::result::Result<(), String> {
    let mut next = *pos;
    if next.side_to_move() != side {
        if next.must_pass() {
            next = next.play(Action::Pass).map_err(|e| e.to_string())?;
        } else {
            return Err("not this side's turn".into());
        }
    }
    next = next.play(action).map_err(|e| e.to_string())?;
    *pos = next;
    Ok(())
}

/// `gtp` サブコマンドの引数。
#[derive(Args, Debug)]
pub struct GtpArgs {
    #[command(flatten)]
    engine: EngineArgs,
    /// 固定エンジンレベル（1〜60）。`--game-ms` とは排他。
    #[arg(long, required_unless_present = "game_ms", conflicts_with = "game_ms", value_parser = clap::value_parser!(u8).range(1..=60))]
    level: Option<u8>,
    /// 1 人あたりの総持ち時間（ミリ秒）。
    #[arg(long)]
    game_ms: Option<u64>,
    /// `--level` の終盤の段階を上書きする `PERCENT:EMPTIES`（複数回またはカンマ区切りで指定可。0 で無効化）。
    #[arg(long, conflicts_with = "game_ms", value_delimiter = ',', value_parser = crate::settings::endgame_stage)]
    endgame: Vec<crate::settings::EndgameStage>,
}

/// `--level` と `--endgame` から固定レベルの探索目標を作る。上書きがなければレベルそのまま。
fn level_goal(level: u8, stages: &[crate::settings::EndgameStage]) -> Goal {
    if stages.is_empty() {
        return Goal::Level(level);
    }
    let mut plan = crate::settings::plan(Goal::Level(level));
    for stage in stages {
        plan = plan.endgame(stage.selectivity, stage.empties);
    }
    Goal::Plan(plan)
}

/// 標準入力から GTP コマンドを 1 行ずつ読み取り、応答（`=id ...` / `?id ...`）を標準出力へ返す。
pub fn run(args: GtpArgs) -> Result<()> {
    let mut mode = match args.level {
        Some(level) => Mode::Level(level_goal(level, &args.endgame)),
        None => Mode::Time {
            remaining: [Duration::from_millis(args.game_ms.unwrap()); 2],
        },
    };
    let mut initial_time = match mode {
        Mode::Time { remaining } => remaining,
        Mode::Level(_) => [Duration::ZERO; 2],
    };
    let mut engine = args.engine.engine()?;
    eprintln!("{}", crate::settings::versions());
    let mut pos = Position::initial();
    let stdin = io::stdin();
    let mut out = io::stdout();
    const COMMANDS: &[&str] = &[
        "protocol_version",
        "name",
        "version",
        "known_command",
        "list_commands",
        "boardsize",
        "clear_board",
        "komi",
        "play",
        "genmove",
        "time_settings",
        "time_left",
        "showboard",
        "final_score",
        "quit",
    ];
    for line in stdin.lock().lines() {
        let line = line?;
        // GTP 仕様に従い、水平タブ以外の制御文字と `#` 以降のコメントを取り除く。
        let clean: String = line
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        let clean = clean.split('#').next().unwrap_or("");
        let mut words: Vec<&str> = clean.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        let id = if words[0].bytes().all(|b| b.is_ascii_digit()) {
            words.remove(0)
        } else {
            ""
        };
        let cmd = words.first().copied().unwrap_or("");
        let expected = match cmd {
            "known_command" | "boardsize" | "komi" | "genmove" => 2,
            "play" => 3,
            "time_settings" | "time_left" => 4,
            _ => 1,
        };
        let reply: std::result::Result<String, String> = if words.len() != expected {
            Err("syntax error: wrong number of arguments".into())
        } else {
            match cmd {
                "protocol_version" => Ok("2".into()),
                "name" => Ok("Deft".into()),
                "version" => Ok(crate::VERSION.into()),
                "known_command" => Ok(COMMANDS.contains(&words[1]).to_string()),
                "list_commands" => Ok(COMMANDS.join("\n")),
                "boardsize" if words[1].parse::<u8>() != Ok(8) => {
                    Err("only boardsize 8 is supported".into())
                }
                "boardsize" | "clear_board" => {
                    engine.clear_tt().map_err(|e| e.to_string()).map(|_| {
                        pos = Position::initial();
                        if let Mode::Time { remaining } = &mut mode {
                            *remaining = initial_time;
                        }
                        String::new()
                    })
                }
                "komi" => match words[1].parse::<f64>() {
                    Ok(0.0) => Ok(String::new()),
                    _ => Err("only komi 0 is supported".into()),
                },
                "showboard" => Ok(format!("\n{}", crate::display::board(pos).trim_end())),
                "final_score" if !pos.is_terminal() => Err("game is not over".into()),
                "final_score" => {
                    let score = pos.final_score(Color::Black);
                    Ok(if score == 0 {
                        "0".into()
                    } else {
                        format!("{}+{}", if score > 0 { "B" } else { "W" }, score.abs())
                    })
                }
                "time_settings" => {
                    if words[2].parse::<f64>() != Ok(0.0) || words[3].parse::<u32>() != Ok(0) {
                        Err("byo-yomi is not supported".into())
                    } else {
                        seconds(words.get(1)).map(|main| {
                            if let Mode::Time { remaining } = &mut mode {
                                *remaining = [main; 2];
                                initial_time = *remaining;
                            }
                            String::new()
                        })
                    }
                }
                "time_left" => match (color(words[1]), words[3].parse::<u32>()) {
                    (Some(side), Ok(0)) => seconds(words.get(2)).map(|left| {
                        if let Mode::Time { remaining } = &mut mode {
                            remaining[clock(side)] = left;
                        }
                        String::new()
                    }),
                    _ => Err("invalid color or unsupported byo-yomi stones".into()),
                },
                "play" => match color(words[1]) {
                    Some(side) => words[2]
                        .parse::<Action>()
                        .map_err(str::to_string)
                        .and_then(|a| apply(&mut pos, side, a))
                        .map(|_| String::new()),
                    None => Err("invalid color".into()),
                },
                "genmove" => match color(words[1]) {
                    None => Err("invalid color".into()),
                    Some(side) => generate(&mut engine, &mut pos, &mut mode, side),
                },
                "quit" => Ok(String::new()),
                _ => Err("unknown command".into()),
            }
        };
        let quitting = cmd == "quit" && reply.is_ok();
        match reply {
            Ok(r) => writeln!(out, "={id} {r}\n")?,
            Err(e) => writeln!(out, "?{id} {e}\n")?,
        }
        out.flush()?;
        if quitting {
            break;
        }
    }
    Ok(())
}

/// 探索結果から最善手を検証して取り出す。探索ワーカー失敗や着手なしはエラーにする。
fn searched_action(
    result: std::result::Result<SearchResult, EngineError>,
) -> std::result::Result<Action, String> {
    let result = result.map_err(|e| e.to_string())?;
    if result.stop_reason == StopReason::WorkerFailed {
        return Err("search worker failed".into());
    }
    result
        .best_move
        .ok_or_else(|| "search returned no legal move".into())
}

/// `genmove` コマンドを実行し、選択した着手を盤面に反映して小文字座標（または `pass`）で返す。
fn generate(
    engine: &mut crate::Engine,
    pos: &mut Position,
    mode: &mut Mode,
    side: Color,
) -> std::result::Result<String, String> {
    let mut next = *pos;
    if next.is_terminal() {
        return Ok("pass".into());
    }
    if next.side_to_move() != side && next.must_pass() {
        next = next.play(Action::Pass).map_err(|e| e.to_string())?;
    }
    if next.side_to_move() != side {
        return Err("not this side's turn".into());
    }
    if next.must_pass() {
        *pos = next.play(Action::Pass).map_err(|e| e.to_string())?;
        return Ok("pass".into());
    }
    let start = Instant::now();
    let log = cfg!(feature = "trace");
    let mut last = None;
    let mut steps = String::new();
    let tag = |kind: &ResultKind| match kind {
        ResultKind::Depth { depth, .. } => format!("d{depth}"),
        ResultKind::Endgame { selectivity } => format!("e{}", selectivity.percent()),
    };
    let mut report = |p: &deft_reversi_engine::Progress| {
        if log {
            last = Some((p.elapsed, p.kind, p.next));
            steps.push_str(&format!(" {}:{:.4}", tag(&p.kind), p.elapsed.as_secs_f64()));
        }
    };
    let mut limit = Duration::ZERO;
    let action = match mode {
        Mode::Level(goal) => {
            let request = SearchRequest::new(*goal);
            let _armed = stop::arm(request.stop_handle());
            searched_action(engine.search_with(next, &request, report))
        }
        Mode::Time { remaining } => {
            let mut moves = next.legal_moves();
            // 10 ms は応答・時計処理の余裕として差し引く。
            let left = remaining[clock(side)].saturating_sub(Duration::from_millis(10));
            let budget = Budget::new(next.empty_count(), left, left);
            limit = budget.hard;
            if moves.len() == 1 {
                Ok(Action::Place(moves.next().unwrap()))
            } else if limit.is_zero() {
                searched_action(game_clock::reflex(engine, next))
            } else {
                searched_action(budget.search(engine, next, true, &mut report))
            }
        }
    };
    if log {
        let (elapsed, kind, pending) =
            last.map_or((Duration::ZERO, String::new(), "-".into()), |(t, k, n)| {
                (
                    t,
                    format!("{k:?}").replace(' ', ""),
                    n.as_ref().map_or("-".into(), tag),
                )
            });
        // `next` は探索停止時に実行中（または次に実行予定）だった反復、
        // `steps` は完了した各反復の経過秒数を表す。
        eprintln!(
            "empties {} budget {:.4} used {:.4} last_done {:.4} kind {} next {pending} steps{steps}",
            next.empty_count(),
            limit.as_secs_f64(),
            start.elapsed().as_secs_f64(),
            elapsed.as_secs_f64(),
            kind
        );
    }
    if let Mode::Time { remaining } = mode {
        remaining[clock(side)] = remaining[clock(side)].saturating_sub(start.elapsed());
    }
    let action = action?;
    apply(&mut next, side, action)?;
    *pos = next;
    Ok(action.to_string().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_move_after_implicit_pass_is_atomic() {
        let mut pos: Position = format!("{}{}-- X", "O".repeat(32), "X".repeat(30))
            .parse()
            .unwrap();
        let before = pos;
        assert!(pos.must_pass());
        assert!(apply(&mut pos, Color::White, "a1".parse().unwrap()).is_err());
        assert_eq!(pos, before);
    }
    #[test]
    fn invalid_time_values_are_rejected() {
        for text in ["NaN", "inf", "-1", "1e500"] {
            assert!(seconds(Some(&text)).is_err());
        }
        assert_eq!(seconds(Some(&"0.125")).unwrap(), Duration::from_millis(125));
    }
    #[test]
    fn endgame_stages_override_the_level() {
        use deft_reversi_engine::Selectivity;
        assert_eq!(level_goal(5, &[]), Goal::Level(5));
        let stages: Vec<_> = ["85:0", "95:0", "98:0", "99:0", "100:10"]
            .iter()
            .map(|s| crate::settings::endgame_stage(s).unwrap())
            .collect();
        let plan = crate::settings::plan(level_goal(5, &stages));
        let level = crate::settings::plan(Goal::Level(5));
        assert_eq!(plan.midgame_depth(), level.midgame_depth());
        assert_eq!(plan.midgame_selectivity(), level.midgame_selectivity());
        assert_eq!(plan.endgame_limit(Selectivity::Full), 10);
        for selectivity in [
            Selectivity::Percent85,
            Selectivity::Percent95,
            Selectivity::Percent98,
            Selectivity::Percent99,
        ] {
            assert_eq!(plan.endgame_limit(selectivity), 0);
        }
    }
}
