//! GTP interface for CLI-based matches.
use crate::{
    EngineArgs, Result,
    game_clock::{self, Budget, ClockState, GameContext, Protocol},
    stop,
};
use clap::Args;
use deft_reversi_engine::{
    Action, Color, EngineError, Goal, Position, SearchRequest, SearchResult, StopReason,
};
use std::{
    io::{self, BufRead, Write},
    time::{Duration, Instant},
};

enum Mode {
    Level(u8),
    /// The time left of black and of white.
    Time {
        remaining: [Duration; 2],
    },
}

fn clock(side: Color) -> usize {
    match side {
        Color::Black => 0,
        Color::White => 1,
    }
}

/// Seconds from GTP as a duration; negative, infinite or NaN values are
/// errors.
fn seconds(text: Option<&&str>) -> std::result::Result<Duration, String> {
    text.and_then(|s| s.parse::<f64>().ok())
        .and_then(|s| Duration::try_from_secs_f64(s).ok())
        .ok_or_else(|| "syntax error".into())
}

fn color(s: &str) -> Option<Color> {
    match s.to_ascii_lowercase().as_str() {
        "b" | "black" => Some(Color::Black),
        "w" | "white" => Some(Color::White),
        _ => None,
    }
}

/// Plays `action` for `side`, passing first for the other side if it has no
/// move (GTP clients do not always send passes).
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

#[derive(Args, Debug)]
pub struct GtpArgs {
    #[command(flatten)]
    engine: EngineArgs,
    /// Fixed engine level, 1-60.
    #[arg(long, required_unless_present = "game_ms", conflicts_with = "game_ms", value_parser = clap::value_parser!(u8).range(1..=60))]
    level: Option<u8>,
    /// Total time per side in milliseconds.
    #[arg(long)]
    game_ms: Option<u64>,
}

pub fn run(args: GtpArgs) -> Result<()> {
    let mut mode = match args.level {
        Some(level) => Mode::Level(level),
        None => Mode::Time {
            remaining: [Duration::from_millis(args.game_ms.unwrap()); 2],
        },
    };
    let mut initial_time = match mode {
        Mode::Time { remaining } => remaining,
        Mode::Level(_) => [Duration::ZERO; 2],
    };
    let mut engine = args.engine.engine()?;
    // Each side remembers its own turns (one process may play both).
    let mut ctx = [GameContext::default(), GameContext::default()];
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
                        ctx.iter_mut().for_each(GameContext::new_game);
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
                    Some(side) => generate(
                        &mut engine,
                        &mut pos,
                        &mut mode,
                        &mut ctx[clock(side)],
                        side,
                    ),
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

fn generate(
    engine: &mut crate::Engine,
    pos: &mut Position,
    mode: &mut Mode,
    ctx: &mut GameContext,
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
    let log = std::env::var_os("DEFT_GTP_LOG").is_some();
    let mut last = None;
    let mut steps = String::new();
    let tag = game_clock::tag;
    let mut report = |p: &deft_reversi_engine::Progress| {
        if log {
            last = Some((p.elapsed, p.kind, p.next));
            steps.push_str(&format!(" {}:{:.4}", tag(&p.kind), p.elapsed.as_secs_f64()));
        }
    };
    let mut limit = Duration::ZERO;
    let action = match mode {
        Mode::Level(l) => {
            let request = SearchRequest::new(Goal::Level(*l));
            let _armed = stop::arm(request.stop_handle());
            searched_action(engine.search_with(next, &request, report))
        }
        Mode::Time { remaining } => {
            let mut moves = next.legal_moves();
            let left = remaining[clock(side)].saturating_sub(Duration::from_millis(10));
            ctx.begin_turn(next);
            let clock = ClockState {
                empties: next.empty_count(),
                remaining: left,
                cap: left,
                protocol: Protocol::Gtp,
                threads: engine.options().threads,
            };
            let budget = Budget::new(clock, ctx);
            limit = budget.hard();
            if moves.len() == 1 {
                let action = Action::Place(moves.next().unwrap());
                game_clock::unsearched_turn(
                    ctx,
                    next,
                    start.elapsed(),
                    Some(&budget),
                    Some(action),
                );
                Ok(action)
            } else if limit.is_zero() {
                let action = searched_action(game_clock::reflex(engine, next));
                let played = action.as_ref().ok().copied();
                game_clock::unsearched_turn(ctx, next, start.elapsed(), Some(&budget), played);
                action
            } else {
                searched_action(budget.search(engine, next, ctx, &mut report))
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
        // `next` is the iteration that was running (or would have run) when
        // the search stopped; `steps` lists each completed iteration.
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
}
