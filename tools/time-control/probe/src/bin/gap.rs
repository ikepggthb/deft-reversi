//! Root by-products and the true gap to the second move, per iteration
//! (stage 4-2).
//!
//! usage: gap POSITIONS.tsv OUT.tsv LIMIT GAP_LIMIT SHARD/SHARDS  (DEFT_MODEL)
//!
//! POSITIONS has `key black white side empties` per line (`#` lines are
//! skipped). Takes every SHARDS-th position starting at SHARD (0-based) and,
//! from an empty table on one thread, runs the games' iteration plan
//! (`timed_levels(60, LIMIT seconds)`), recording each finished iteration:
//! its move and score, nodes, and the root counters of `Progress::root`.
//! Then, for every iteration but the last, it searches the same kind again
//! with every root move scored on a full window (`all_moves`, at most
//! GAP_LIMIT seconds) for the true gap between the best and the second
//! move at that depth. Searching for the gap is not something a game would
//! do (it costs several iterations); it is measured here only to compare
//! with the free counters.
//!
//! Appends one line per iteration:
//!   key empties k kind best score nodes elapsed first_nodes window_nodes
//!   fail_highs aspiration_fails gap second moves
//! `nodes` is the iteration's own (the difference of the cumulative count),
//! `gap` the all-moves best score minus the second (`-` when the search ran
//! out of time or there is one move), `moves` all scored moves `sq:score`.
//! A position whose key is in OUT.tsv is skipped, so a stopped run resumes;
//! a file named STOP next to OUT.tsv ends the run after the current
//! position.
use deft_reversi_engine::*;
use std::collections::HashSet;
use std::io::Write;
use std::time::Duration;

fn tag(kind: &ResultKind) -> String {
    match kind {
        ResultKind::Depth { depth, .. } => format!("d{depth}"),
        ResultKind::Endgame { selectivity } => format!("e{}", selectivity.percent()),
    }
}

fn square(action: Option<Action>) -> String {
    match action {
        Some(Action::Place(sq)) => sq.to_string(),
        Some(Action::Pass) => "pass".into(),
        None => "-".into(),
    }
}

struct Row {
    kind: ResultKind,
    best: Option<Action>,
    score: i32,
    nodes: u64,
    elapsed: Duration,
    root: RootStats,
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let input = std::fs::read_to_string(&a[1]).unwrap();
    let out_path = &a[2];
    let limit = Duration::from_secs_f64(a[3].parse().unwrap());
    let gap_limit = Duration::from_secs_f64(a[4].parse().unwrap());
    let (shard, shards) = a[5].split_once('/').unwrap();
    let (shard, shards): (usize, usize) = (shard.parse().unwrap(), shards.parse().unwrap());
    let done: HashSet<String> = std::fs::read_to_string(out_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split('\t').next().map(String::from))
        .collect();
    let model = Model::from_file(std::env::var("DEFT_MODEL").unwrap()).unwrap();
    let mut engine = Engine::new(model, EngineOptions::default().threads(1).tt_mib(256)).unwrap();
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out_path)
        .unwrap();
    let stop = std::path::Path::new(out_path)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("STOP");
    let lines = input.lines().filter(|l| !l.starts_with('#'));
    for line in lines.skip(shard).step_by(shards) {
        if stop.exists() {
            break;
        }
        let w: Vec<&str> = line.split('\t').collect();
        if done.contains(w[0]) {
            continue;
        }
        let black = u64::from_str_radix(w[1], 16).unwrap();
        let white = u64::from_str_radix(w[2], 16).unwrap();
        let side = if w[3] == "b" { Color::Black } else { Color::White };
        let empties: u8 = w[4].parse().unwrap();
        let pos = Position::from_colors(black, white, side).unwrap();
        assert_eq!(pos.empty_count(), empties, "{line}");
        engine.clear_tt().unwrap();
        let mut rows: Vec<Row> = Vec::new();
        let mut last_nodes = 0;
        let request = SearchRequest::timed_levels(60, limit).skip_pv_recovery();
        engine
            .search_with(pos, &request, |p| {
                rows.push(Row {
                    kind: p.kind,
                    best: p.best_move,
                    score: p.score,
                    nodes: p.nodes - last_nodes,
                    elapsed: p.elapsed,
                    root: p.root,
                });
                last_nodes = p.nodes;
            })
            .unwrap();
        let legal = pos.legal_moves().bits().count_ones() as usize;
        let mut text = String::new();
        for (k, r) in rows.iter().enumerate() {
            let (mut gap, mut second, mut moves) = ("-".to_string(), "-".to_string(), "-".to_string());
            if k + 1 < rows.len() && legal >= 2 {
                let goal = match r.kind {
                    ResultKind::Depth { depth, .. } => Goal::Depth(depth),
                    ResultKind::Endgame { .. } => Goal::Endgame,
                };
                let req = SearchRequest::new(goal)
                    .selectivity(r.kind.selectivity())
                    .all_moves()
                    .skip_pv_recovery()
                    .time_limit(gap_limit);
                if let Ok(res) = engine.search(pos, &req)
                    && res.moves.len() == legal
                {
                    gap = (res.moves[0].score - res.moves[1].score).to_string();
                    second = square(Some(res.moves[1].action));
                    moves = res
                        .moves
                        .iter()
                        .map(|m| format!("{}:{}", square(Some(m.action)), m.score))
                        .collect::<Vec<_>>()
                        .join(",");
                }
            }
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.5}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                w[0],
                empties,
                k,
                tag(&r.kind),
                square(r.best),
                r.score,
                r.nodes,
                r.elapsed.as_secs_f64(),
                r.root.first_nodes,
                r.root.window_nodes,
                r.root.fail_highs,
                r.root.aspiration_fails,
                gap,
                second,
                moves
            ));
        }
        if rows.is_empty() {
            text.push_str(&format!("{}\t{}\t-1\n", w[0], empties));
        }
        out.write_all(text.as_bytes()).unwrap();
        out.flush().unwrap();
    }
}
