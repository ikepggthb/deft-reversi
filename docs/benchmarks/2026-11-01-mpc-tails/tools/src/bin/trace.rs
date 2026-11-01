//! Prints every iteration of one search: kind, score, best move, nodes and
//! seconds spent in it. Usage: trace MODEL[@RULES] POSITION [exact|DEPTH]
use deft_reversi_engine::{EngineOptions, Goal, Position, Progress, SearchRequest};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut engine = selfplay::engine(
        &args[0],
        EngineOptions::default().threads(1).tt_mib(
            std::env::var("TT_MIB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(256),
        ),
    )?;
    let position: Position = args[1].parse()?;
    let goal = match args.get(2).map(String::as_str) {
        None | Some("exact") => Goal::Endgame,
        Some(d) => Goal::Depth(d.parse()?),
    };
    let (mut nodes, mut seconds) = (0u64, 0f64);
    let result = engine.search_with(position, &SearchRequest::new(goal), |p: &Progress| {
        let s = p.elapsed.as_secs_f64();
        println!(
            "{:<45} {:+3} {:<5} {:>12} {:>8.3}s  pv {}",
            format!("{:?}", p.kind),
            p.score,
            p.best_move.map_or("-".into(), |m| format!("{m:?}")),
            p.nodes - nodes,
            s - seconds,
            p.pv.iter()
                .take(8)
                .map(|a| format!("{a:?}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        nodes = p.nodes;
        seconds = s;
    })?;
    println!(
        "total {} nodes {:.3}s score {:?} stats {:?}",
        result.stats.nodes,
        result.elapsed.as_secs_f64(),
        result.score,
        result.stats
    );
    Ok(())
}
