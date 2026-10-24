//! Normal API comparison uses level:60. Other modes are low-level diagnostics.
use std::{
    error::Error,
    fs::File,
    io::{BufRead, BufReader},
    time::Instant,
};
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(5..=6).contains(&args.len()) {
        return Err(
            "usage: deft-engine-compare OLD_MODEL NEW_MODEL POSITIONS exact|DEPTH|level:60 ROUNDS [THREADS]"
                .into(),
        );
    }
    let level = args[3]
        .strip_prefix("level:")
        .map(str::parse::<i32>)
        .transpose()?;
    if level.is_some_and(|value| value != 60) {
        return Err("normal-API comparison currently supports level:60 only".into());
    }
    if level.is_none() {
        eprintln!(
            "Diagnostic low-level reference API comparison: not normal solve(board, level) performance."
        );
    }
    let depth = if args[3] == "exact" || level.is_some() {
        None
    } else {
        Some(args[3].parse::<u8>()?)
    };
    if depth.is_some_and(|d| d > 60) {
        return Err("depth must be <=60".into());
    }
    let rounds = args[4].parse::<usize>()?;
    if rounds < 5 {
        return Err("at least five rounds required".into());
    }
    let evaluator = old::evaluator_from_path(&args[0])?;
    let threads = args
        .get(5)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(1);
    let mut new_solver = new::Engine::from_file(
        &args[1],
        new::EngineOptions {
            threads,
            tt_mib: 256,
        },
    )?;
    let model = new::EngineModel::from_file(&args[1])?.pattern;
    if level.is_some() {
        let reference = old::Solver::from_file(
            &args[0],
            old::SolverOptions {
                tt_capacity: Some(1),
                ..Default::default()
            },
        )?;
        let actual = new_solver
            .mpc_config()
            .ok_or("missing loaded MPC configuration")?;
        for empties in 0..=60 {
            let expected = reference.mpc_config().final_search.params(empties);
            let loaded = actual.final_search.params(empties);
            if expected.lv != loaded.lv
                || [
                    expected.a.to_bits(),
                    expected.b.to_bits(),
                    expected.e_std.to_bits(),
                ] != [
                    loaded.a.to_bits(),
                    loaded.b.to_bits(),
                    loaded.e_std.to_bits(),
                ]
            {
                return Err("loaded final MPC parameters differ".into());
            }
            for depth in 0..=empties {
                let expected = reference.mpc_config().eval_search.params(depth, empties);
                let loaded = actual.eval_search.params(depth, empties);
                if expected.lv != loaded.lv
                    || [
                        expected.a.to_bits(),
                        expected.b.to_bits(),
                        expected.e_std.to_bits(),
                    ] != [
                        loaded.a.to_bits(),
                        loaded.b.to_bits(),
                        loaded.e_std.to_bits(),
                    ]
                {
                    return Err("loaded midgame MPC parameters differ".into());
                }
            }
        }
    }
    let mut boards = Vec::new();
    for line in BufReader::new(File::open(&args[2])?).lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let cells = parts.next().ok_or("missing cells")?.as_bytes();
        let turn = parts.next().ok_or("missing turn")?.trim_end_matches(';');
        if cells.len() != 64 {
            return Err("position requires 64 cells".into());
        }
        let (mut black, mut white) = (0u64, 0u64);
        for (i, &c) in cells.iter().enumerate() {
            match c {
                b'X' | b'x' | b'*' => black |= 1u64 << i,
                b'O' | b'o' => white |= 1u64 << i,
                b'-' | b'.' => {}
                _ => return Err("invalid cell".into()),
            }
        }
        let (p, o) = match turn {
            "X" | "x" | "*" => (black, white),
            "O" | "o" => (white, black),
            _ => return Err("invalid turn".into()),
        };
        let old_board = old::Board {
            player: p,
            opponent: o,
        };
        let new_board = new::Board::from_bits(p, o).ok_or("invalid board")?;
        if evaluator.evaluate(&old_board) != model.evaluate(&new_board) {
            return Err("converted evaluation differs from reference".into());
        }
        boards.push((old_board, new_board));
    }
    if boards.is_empty() {
        return Err("no positions".into());
    }
    println!("round\tposition\tengine\tseconds\tscore\tnodes");
    let mut times = [Vec::new(), Vec::new()];
    for round in 0..rounds {
        let mut totals = [0f64; 2];
        for (index, (old_board, new_board)) in boards.iter().enumerate() {
            let mut scores = [0; 2];
            // Alternate execution order between rounds; cold tables per position.
            for which in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let (seconds, score, nodes) = if which == 0 {
                    // A fresh thread resets the reference's thread-local endgame TT;
                    // constructing a fresh Solver also clears its global tables.
                    let evaluator = evaluator.clone();
                    let old_model_path = args[0].clone();
                    let board = *old_board;
                    let (seconds, score, nodes, aborted) = std::thread::spawn(move || {
                        let options = old::SolverOptions {
                            tt_capacity: Some(256),
                            ..Default::default()
                        };
                        // Normal API must also load the model-specific MPC settings.
                        let solver = if level.is_some() {
                            old::Solver::from_file(&old_model_path, options)
                                .expect("reference model")
                        } else {
                            old::Solver::with_options(evaluator, options)
                        };
                        let start = Instant::now();
                        let result = if let Some(level) = level {
                            solver.solve(&board, level)
                        } else {
                            match depth {
                                Some(d) => solver.solve_eval(
                                    &board,
                                    i32::from(d),
                                    old::NO_MPC_SELECTIVITY_LV,
                                ),
                                None => solver.solve_final(&board, old::NO_MPC_SELECTIVITY_LV),
                            }
                        };
                        let seconds = start.elapsed().as_secs_f64();
                        if level.is_some()
                            && !matches!(
                                result.solver_type,
                                old::SolverType::Final(old::NO_MPC_SELECTIVITY_LV)
                            )
                        {
                            panic!("reference did not complete a full exact search");
                        }
                        (seconds, result.score, result.nodes, result.aborted)
                    })
                    .join()
                    .map_err(|_| "reference worker panicked")?;
                    if aborted {
                        return Err("reference aborted".into());
                    }
                    (seconds, score, nodes)
                } else {
                    new_solver.clear_tt()?;
                    let start = Instant::now();
                    let result = if let Some(level) = level {
                        new_solver.solve(*new_board, level)?
                    } else {
                        new_solver.search(new::SearchRequest::new(
                            *new_board,
                            depth
                                .map(new::SearchMode::Depth)
                                .unwrap_or(new::SearchMode::Exact),
                        ))?
                    };
                    let seconds = start.elapsed().as_secs_f64();
                    if result.stop_reason != new::StopReason::Completed {
                        return Err("new engine aborted".into());
                    }
                    if level.is_some() {
                        if result.mode != new::SearchMode::Exact || result.selective {
                            return Err("new engine did not complete a full exact search".into());
                        }
                        let mut end = *new_board;
                        let mut sign = 1;
                        for &action in &result.pv {
                            end = end.play(action)?;
                            sign = -sign;
                        }
                        if !end.is_terminal() || Some(sign * end.final_score()) != result.score {
                            return Err(format!(
                                "PV does not prove the score at position {}",
                                index + 1
                            )
                            .into());
                        }
                    }
                    (
                        seconds,
                        result.score.ok_or("missing score")?,
                        result.stats.nodes,
                    )
                };
                scores[which] = score;
                totals[which] += seconds;
                println!(
                    "{}\t{}\t{}\t{seconds:.9}\t{score}\t{nodes}",
                    round + 1,
                    index + 1,
                    if which == 0 { "old" } else { "new" }
                );
            }
            if scores[0] != scores[1] {
                return Err(format!(
                    "search scores differ at position {}: {:?}",
                    index + 1,
                    scores
                )
                .into());
            }
        }
        for i in 0..2 {
            times[i].push(totals[i]);
        }
    }
    for time in &mut times {
        time.sort_by(f64::total_cmp);
    }
    let median = |v: &Vec<f64>| {
        if v.len().is_multiple_of(2) {
            (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0
        } else {
            v[v.len() / 2]
        }
    };
    eprintln!(
        "old median={:.6}s new median={:.6}s ratio={:.3}",
        median(&times[0]),
        median(&times[1]),
        median(&times[0]) / median(&times[1])
    );
    Ok(())
}
