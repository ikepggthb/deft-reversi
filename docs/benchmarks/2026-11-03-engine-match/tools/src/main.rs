//! A minimal GTP front end for matches against Edax and Egaroucid.
//!
//! usage: deft-gtp MODEL (--level N | --game-ms MS) [--tt MIB]
//!
//! With `--level`, every move is a `Goal::Level(N)` search. With
//! `--game-ms`, each side has MS milliseconds for the game (or what
//! `time_settings`/`time_left` say); a move gets the remaining time divided
//! by this side's moves left before 20 empties (positions from there are
//! solved in a small part of the time), searched as `Goal::Level(60)` with
//! that time limit, which plays the best move of the last completed iteration.
use deft_reversi_engine::{
    Action, Color, Engine, EngineOptions, Goal, Model, Position, SearchRequest,
};
use std::{
    io::{self, BufRead, Write},
    time::{Duration, Instant},
};

enum Mode {
    Level(u8),
    Time { remaining: Duration },
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
fn apply(pos: &mut Position, side: Color, action: Action) -> Result<(), String> {
    if pos.side_to_move() != side {
        if pos.must_pass() {
            *pos = pos.play(Action::Pass).map_err(|e| e.to_string())?;
        } else {
            return Err("not this side's turn".into());
        }
    }
    if action == Action::Pass && !pos.must_pass() {
        return Err("pass with legal moves".into());
    }
    *pos = pos.play(action).map_err(|e| e.to_string())?;
    Ok(())
}

const SOLVE_EMPTIES: u8 = 20;

fn request_budget(mode: &Mode, pos: &Position) -> Duration {
    match mode {
        Mode::Level(_) => Duration::ZERO,
        // Positions with SOLVE_EMPTIES or fewer empties are solved in a small
        // part of the time, so the time is spread over this side's moves
        // before them: (empties - SOLVE_EMPTIES) / 2 + 1 moves.
        Mode::Time { remaining } => {
            let moves_left = u32::from(pos.empty_count().saturating_sub(SOLVE_EMPTIES)) / 2 + 1;
            remaining.saturating_sub(Duration::from_millis(10)) / moves_left
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let model = Model::from_file(&args[1])?;
    let mut mode = None;
    let mut tt = 64;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--level" => mode = Some(Mode::Level(args[i + 1].parse()?)),
            "--game-ms" => {
                mode = Some(Mode::Time {
                    remaining: Duration::from_millis(args[i + 1].parse()?),
                })
            }
            "--tt" => tt = args[i + 1].parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
        i += 2;
    }
    let mut mode = mode.ok_or("give --level or --game-ms")?;
    let initial_time = match mode {
        Mode::Time { remaining } => remaining,
        Mode::Level(_) => Duration::ZERO,
    };
    let mut engine = Engine::new(model, EngineOptions::default().threads(1).tt_mib(tt))?;
    let mut pos = Position::initial();
    let stdin = io::stdin();
    let mut out = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(&cmd) = words.first() else { continue };
        let reply: Result<String, String> = match cmd {
            "protocol_version" => Ok("2".into()),
            "name" => Ok("Deft".into()),
            "version" => Ok(env!("CARGO_PKG_VERSION").into()),
            "list_commands" => Ok("protocol_version\nname\nversion\nlist_commands\nboardsize\nclear_board\nkomi\nplay\ngenmove\ntime_settings\ntime_left\nquit".into()),
            "boardsize" | "komi" => Ok(String::new()),
            "clear_board" => {
                pos = Position::initial();
                if let Mode::Time { remaining } = &mut mode {
                    *remaining = initial_time;
                }
                engine.clear_tt().map(|_| String::new()).map_err(|e| e.to_string())
            }
            "time_settings" => match (&mut mode, words.get(1).and_then(|s| s.parse::<f64>().ok())) {
                (Mode::Time { remaining }, Some(main)) => {
                    *remaining = Duration::from_secs_f64(main);
                    Ok(String::new())
                }
                _ => Ok(String::new()),
            },
            "time_left" => match (&mut mode, words.get(2).and_then(|s| s.parse::<f64>().ok())) {
                (Mode::Time { remaining }, Some(left)) => {
                    *remaining = Duration::from_secs_f64(left);
                    Ok(String::new())
                }
                _ => Ok(String::new()),
            },
            "play" => match (words.get(1).and_then(|s| color(s)), words.get(2)) {
                (Some(side), Some(m)) => m
                    .parse::<Action>()
                    .map_err(|e| e.to_string())
                    .and_then(|a| apply(&mut pos, side, a))
                    .map(|_| String::new()),
                _ => Err("syntax error".into()),
            },
            "genmove" => match words.get(1).and_then(|s| color(s)) {
                None => Err("syntax error".into()),
                Some(side) => {
                    if pos.side_to_move() != side && pos.must_pass() {
                        pos = pos.play(Action::Pass).map_err(|e| e.to_string()).unwrap();
                    }
                    if pos.side_to_move() != side {
                        Err("not this side's turn".into())
                    } else if pos.must_pass() || pos.is_terminal() {
                        if !pos.is_terminal() {
                            pos = pos.play(Action::Pass).unwrap();
                        }
                        Ok("pass".into())
                    } else {
                        let start = Instant::now();
                        let request = match &mode {
                            Mode::Level(l) => SearchRequest::new(Goal::Level(*l)),
                            Mode::Time { .. } => {
                                let budget = request_budget(&mode, &pos);
                                SearchRequest::new(Goal::Level(60))
                                    .time_limit(budget.max(Duration::from_millis(1)))
                            }
                        };
                        let mut last = (Duration::ZERO, String::new());
                        let result = engine
                            .search_with(pos, &request, |p| {
                                last = (p.elapsed, format!("{:?}", p.kind));
                            })
                            .map_err(|e| e.to_string());
                        if std::env::var_os("DEFT_GTP_LOG").is_some() {
                            eprintln!(
                                "empties {} budget {:.4} used {:.4} last_done {:.4} kind {}",
                                pos.empty_count(),
                                request_budget(&mode, &pos).as_secs_f64(),
                                start.elapsed().as_secs_f64(),
                                last.0.as_secs_f64(),
                                last.1.replace(' ', "")
                            );
                        }
                        if let Mode::Time { remaining } = &mut mode {
                            *remaining = remaining.saturating_sub(start.elapsed());
                        }
                        match result {
                            Ok(r) => {
                                let action = r.best_move.unwrap_or(Action::Pass);
                                apply(&mut pos, side, action).map(|_| action.to_string().to_lowercase())
                            }
                            Err(e) => Err(e),
                        }
                    }
                }
            },
            "quit" => {
                writeln!(out, "= \n")?;
                out.flush()?;
                return Ok(());
            }
            _ => Err("unknown command".into()),
        };
        match reply {
            Ok(r) => writeln!(out, "= {r}\n")?,
            Err(e) => writeln!(out, "? {e}\n")?,
        }
        out.flush()?;
    }
    Ok(())
}
