use deft_reversi_engine::*;
use std::time::Duration;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let games = std::fs::read_to_string(&a[1]).unwrap();
    let limit = Duration::from_secs_f64(a[2].parse().unwrap());
    let empties: Vec<u8> = a[3].split(',').map(|s| s.parse().unwrap()).collect();
    let ngames: usize = a[4].parse().unwrap();
    let threads: usize = a[5].parse().unwrap();
    let skip: usize = a[6].parse().unwrap();
    let model = Model::from_file(std::env::var("DEFT_MODEL").unwrap()).unwrap();
    let mut engine = Engine::new(model, EngineOptions::default().threads(threads).tt_mib(64)).unwrap();
    for (g, line) in games.lines().enumerate().skip(skip).take(ngames) {
        let mut pos = Position::initial();
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut boards = vec![];
        while i + 1 < bytes.len() {
            let sq: Square = line[i..i + 2].parse().unwrap();
            if pos.must_pass() { pos = pos.play(Action::Pass).unwrap(); }
            pos = pos.play(Action::Place(sq)).unwrap();
            boards.push(pos);
            i += 2;
        }
        for &e in &empties {
            let Some(p) = boards.iter().find(|p| p.empty_count() == e && !p.must_pass()) else { continue };
            engine.clear_tt().unwrap();
            let req = SearchRequest::timed_levels(60, limit).skip_pv_recovery();
            let mut prev = Duration::ZERO;
            let mut row = String::new();
            let r = engine.search_with(*p, &req, |pr| {
                let tag = match pr.kind { ResultKind::Depth{depth, ..} => format!("d{depth}"), ResultKind::Endgame{selectivity} => format!("e{}", selectivity.percent()) };
                row.push_str(&format!(" {tag}:{:.4}:{}:{:?}", pr.elapsed.as_secs_f64(), pr.score, pr.best_move.map(|m| m.to_string())));
                prev = pr.elapsed;
            }).unwrap();
            println!("g{g} e{e} stop={:?} total={:.3}{row}", r.stop_reason, r.elapsed.as_secs_f64());
        }
    }
}
