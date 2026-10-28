//! Minimal verification adapter, not the application CLI.
use deft_reversi_engine::{Action, Board, Engine, EngineOptions, SearchMode, SearchRequest};
use std::{
    error::Error,
    fs::File,
    io::{self, BufRead, BufReader},
    time::Duration,
};
fn main() -> Result<(), Box<dyn Error>> {
    let mut legacy_mpc = false;
    let mut args = std::env::args().skip(1).filter(|arg| {
        if arg == "--legacy-main-mpc" {
            legacy_mpc = true;
            false
        } else {
            true
        }
    });
    let model = args
        .next()
        .ok_or("usage: solve MODEL POSITIONS [exact|DEPTH] [THREADS] [TT_MiB] [MILLISECONDS]")?;
    let positions = args.next().ok_or("positions file required")?;
    let mode = match args.next().as_deref() {
        None | Some("exact") => SearchMode::Exact,
        Some(depth) => SearchMode::Depth(depth.parse()?),
    };
    let options = EngineOptions {
        threads: args.next().map(|s| s.parse()).transpose()?.unwrap_or(1),
        tt_mib: args.next().map(|s| s.parse()).transpose()?.unwrap_or(256),
    };
    let time_limit = args
        .next()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .map(Duration::from_millis);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let mut engine = Engine::from_file(model, options)?;
    if legacy_mpc {
        engine.set_legacy_main_mpc()?;
    }
    println!(
        "position\tscore\taction\tmode\tdepth\tnodes\tleaves\tsplits\tjobs\thandoffs\tjoin_seconds\tseconds\tstop"
    );
    for (index, line) in BufReader::new(File::open(positions)?).lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let board = parse_position(line).map_err(|message| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("line {}: {message}", index + 1),
            )
        })?;
        let mut request = SearchRequest::new(board, mode);
        request.time_limit = time_limit;
        let result = engine.search(request)?;
        let action = match result.action {
            Some(Action::Place(square)) => square.to_string(),
            Some(Action::Pass) => "pass".into(),
            None => "terminal".into(),
        };
        println!(
            "{}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.9}\t{:.9}\t{:?}",
            index + 1,
            result
                .score
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into()),
            action,
            result.mode,
            result.completed_depth,
            result.stats.nodes,
            result.stats.leaves,
            result.stats.splits,
            result.stats.helper_jobs,
            result.stats.ancestor_handoffs,
            result.stats.join_time.as_secs_f64(),
            result.elapsed.as_secs_f64(),
            result.stop_reason
        );
    }
    Ok(())
}
fn parse_position(line: &str) -> Result<Board, &'static str> {
    let mut parts = line.split_whitespace();
    let cells = parts.next().ok_or("missing board")?.as_bytes();
    let turn = parts.next().ok_or("missing turn")?.trim_end_matches(';');
    if cells.len() != 64 {
        return Err("board must have 64 cells");
    }
    let mut black = 0;
    let mut white = 0;
    for (i, &cell) in cells.iter().enumerate() {
        match cell {
            b'X' | b'x' | b'*' => black |= 1u64 << i,
            b'O' | b'o' => white |= 1u64 << i,
            b'-' | b'.' => {}
            _ => return Err("invalid board character"),
        }
    }
    match turn {
        "X" | "x" | "*" => Board::from_bits(black, white),
        "O" | "o" => Board::from_bits(white, black),
        _ => return Err("turn must be X or O"),
    }
    .ok_or("overlapping stones")
}
