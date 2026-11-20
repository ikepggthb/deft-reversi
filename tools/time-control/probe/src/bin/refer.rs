//! Reference scores of the moves a probe run chose, for `losses.py`.
//! usage: refer GAMES.txt PROBE_OUT DEPTH THREADS  (DEFT_MODEL)
//! For each position in PROBE_OUT it searches the root without restriction
//! (best move and score), then each other move some iteration chose, alone.
//! Positions with at most 28 empties are solved exactly; others get a
//! DEPTH-ply search at 85% MPC (the timed midgame iterations use 83%), a reference rather than the truth, so only
//! iterations clearly shallower than DEPTH have meaningful losses.
//! Prints `gG eE SQ:SCORE ...` like `truth`, listing only those moves.
use deft_reversi_engine::*;
use std::collections::BTreeSet;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let games: Vec<String> = std::fs::read_to_string(&a[1]).unwrap().lines().map(String::from).collect();
    let probe = std::fs::read_to_string(&a[2]).unwrap();
    let depth: u8 = a[3].parse().unwrap();
    let threads: usize = a[4].parse().unwrap();
    let model = Model::from_file(std::env::var("DEFT_MODEL").unwrap()).unwrap();
    let mut engine =
        Engine::new(model, EngineOptions::default().threads(threads).tt_mib(256)).unwrap();
    for line in probe.lines().filter(|l| l.starts_with('g') && l.contains("stop=")) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let g: usize = w[0][1..].parse().unwrap();
        let e: u8 = w[1][1..].parse().unwrap();
        let chosen: BTreeSet<String> = w[5..]
            .iter()
            .map(|s| s.splitn(4, ':').nth(3).unwrap().trim_matches(|c| "Some()\"".contains(c)).to_string())
            .collect();
        let record = &games[g];
        let mut pos = Position::initial();
        let mut found = None;
        for i in (0..record.len().saturating_sub(1)).step_by(2) {
            let sq: Square = record[i..i + 2].parse().unwrap();
            if pos.must_pass() {
                pos = pos.play(Action::Pass).unwrap();
            }
            pos = pos.play(Action::Place(sq)).unwrap();
            if pos.empty_count() == e && !pos.must_pass() {
                found = Some(pos);
                break;
            }
        }
        let p = found.unwrap();
        let request = || {
            if e <= 28 {
                SearchRequest::new(Goal::Endgame)
            } else {
                SearchRequest::new(Goal::Depth(depth)).selectivity(Selectivity::Percent85)
            }
            .skip_pv_recovery()
        };
        let r = engine.search(p, &request()).unwrap();
        let best = r.best_move.map(|m| m.to_string()).unwrap_or_default();
        let mut row = vec![format!("{best}:{}", r.score.unwrap())];
        for m in chosen.iter().filter(|m| **m != best) {
            let sq: Square = m.parse().unwrap();
            let s = engine.search(p, &request().root_moves(1u64 << sq.index())).unwrap();
            row.push(format!("{m}:{}", s.score.unwrap()));
        }
        println!("g{g} e{e} {}", row.join(" "));
    }
}
