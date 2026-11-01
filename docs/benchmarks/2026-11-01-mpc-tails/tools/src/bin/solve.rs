//! The engine's `solve` example, with the model given as `MODEL[@RULES]`.
use deft_reversi_engine::{
    Action, EngineOptions, Goal, Position, ResultKind, SearchRequest, Selectivity,
};
use std::{
    error::Error,
    fs::File,
    io::{BufRead, BufReader},
    time::Duration,
};
fn main() -> Result<(), Box<dyn Error>> {
    // The model argument chooses the ProbCut rules (see the library), so
    // the benchmark scripts' flag is ignored.
    let mut args = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--legacy-main-mpc");
    let model = args.next().ok_or(
        "usage: solve MODEL POSITIONS [exact|endNN|DEPTH] [THREADS] [TT_MiB] [MILLISECONDS]",
    )?;
    let positions = args.next().ok_or("positions file required")?;
    // `endNN` is a selective endgame search at NN percent.
    let mut selectivity = None;
    let goal = match args.next().as_deref() {
        None | Some("exact") => Goal::Endgame,
        Some(s) if s.starts_with("end") => {
            selectivity = Some(match &s[3..] {
                "68" => Selectivity::Percent68,
                "77" => Selectivity::Percent77,
                "85" => Selectivity::Percent85,
                "95" => Selectivity::Percent95,
                "98" => Selectivity::Percent98,
                "99" => Selectivity::Percent99,
                _ => return Err(format!("unknown selectivity `{s}`").into()),
            });
            Goal::Endgame
        }
        Some(depth) => Goal::Depth(depth.parse()?),
    };
    let options = EngineOptions::default()
        .threads(args.next().map(|s| s.parse()).transpose()?.unwrap_or(1))
        .tt_mib(args.next().map(|s| s.parse()).transpose()?.unwrap_or(256));
    let time_limit = args
        .next()
        .map(|s| s.parse::<u64>())
        .transpose()?
        .map(Duration::from_millis);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let mut engine = selfplay::engine(&model, options)?;
    println!(
        "position\tscore\taction\tmode\tdepth\tnodes\tleaves\tsplits\tjobs\thandoffs\tjoin_seconds\tseconds\tstop"
    );
    for (index, line) in BufReader::new(File::open(positions)?).lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let position: Position = line
            .parse()
            .map_err(|error| format!("line {}: {error}", index + 1))?;
        let mut request = SearchRequest::new(goal);
        if let Some(selectivity) = selectivity {
            request = request.selectivity(selectivity);
        }
        if let Some(limit) = time_limit {
            request = request.time_limit(limit);
        }
        let result = engine.search(position, &request)?;
        let action = match result.best_move {
            Some(Action::Place(square)) => square.to_string(),
            Some(Action::Pass) => "pass".into(),
            None => "terminal".into(),
        };
        // The benchmark scripts read these mode names.
        let (mode, depth) = match result.kind {
            None => ("Depth(0)".into(), 0),
            Some(ResultKind::Depth { depth, .. }) => (format!("Depth({depth})"), depth),
            Some(ResultKind::Endgame { selectivity }) => (
                if selectivity == Selectivity::Full {
                    "Exact"
                } else {
                    "SelectiveEndgame"
                }
                .into(),
                position.empty_count(),
            ),
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.9}\t{:.9}\t{:?}",
            index + 1,
            result
                .score
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into()),
            action,
            mode,
            depth,
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
