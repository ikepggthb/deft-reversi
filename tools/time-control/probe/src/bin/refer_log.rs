//! Reference scores for the positions of `refer_input.py` (stage 3-2).
//!
//! usage: refer_log POSITIONS.tsv OUT.tsv DEPTH SHARD/SHARDS  (DEFT_MODEL)
//!
//! Takes every SHARDS-th position starting at SHARD (0-based) and, as `refer`
//! does, searches it without restriction (best move and score), then each
//! other move an iteration chose, alone. Positions with at most 28 empties
//! are solved exactly; others get a DEPTH-ply search at 85% MPC, a
//! reference rather than the truth.
//!
//! Appends `key empties best:score move:score ... seconds` per position and
//! skips keys already in OUT.tsv, so a stopped run resumes where it ended.
//! A file named STOP next to OUT.tsv ends the run after the current
//! position. One search thread; run one process per core.
use deft_reversi_engine::*;
use std::collections::HashSet;
use std::io::Write;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let input = std::fs::read_to_string(&a[1]).unwrap();
    let out_path = &a[2];
    let depth: u8 = a[3].parse().unwrap();
    let (shard, shards) = a[4].split_once('/').unwrap();
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
        let request = || {
            if empties <= 28 {
                SearchRequest::new(Goal::Endgame)
            } else {
                SearchRequest::new(Goal::Depth(depth)).selectivity(Selectivity::Percent85)
            }
            .skip_pv_recovery()
        };
        let start = Instant::now();
        engine.clear_tt().unwrap();
        let r = engine.search(pos, &request()).unwrap();
        let best = r.best_move.map(|m| m.to_string().to_lowercase()).unwrap();
        let mut row = vec![format!("{best}:{}", r.score.unwrap())];
        for m in w[5].split(',').filter(|m| *m != best) {
            let sq: Square = m.parse().unwrap();
            let s = engine.search(pos, &request().root_moves(1u64 << sq.index())).unwrap();
            row.push(format!("{m}:{}", s.score.unwrap()));
        }
        writeln!(
            out,
            "{}\t{empties}\t{}\t{:.2}",
            w[0],
            row.join(" "),
            start.elapsed().as_secs_f64()
        )
        .unwrap();
    }
}
