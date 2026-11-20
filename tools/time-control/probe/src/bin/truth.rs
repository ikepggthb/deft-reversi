//! Exact scores of every legal move, for the positions the probe searches.
//! usage: truth GAMES.txt EMPTIES,.. NGAMES THREADS SKIP  (DEFT_MODEL)
//! Prints `gG eE SQ:SCORE ...` (scores for the side to move).
use deft_reversi_engine::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let games = std::fs::read_to_string(&a[1]).unwrap();
    let empties: Vec<u8> = a[2].split(',').map(|s| s.parse().unwrap()).collect();
    let ngames: usize = a[3].parse().unwrap();
    let threads: usize = a[4].parse().unwrap();
    let skip: usize = a[5].parse().unwrap();
    let model = Model::from_file(std::env::var("DEFT_MODEL").unwrap()).unwrap();
    let mut engine =
        Engine::new(model, EngineOptions::default().threads(threads).tt_mib(256)).unwrap();
    for (g, line) in games.lines().enumerate().skip(skip).take(ngames) {
        let mut pos = Position::initial();
        let mut boards = vec![];
        for i in (0..line.len().saturating_sub(1)).step_by(2) {
            let sq: Square = line[i..i + 2].parse().unwrap();
            if pos.must_pass() {
                pos = pos.play(Action::Pass).unwrap();
            }
            pos = pos.play(Action::Place(sq)).unwrap();
            boards.push(pos);
        }
        for &e in &empties {
            // The same choice as the probe: the first position with e empties
            // whose side to move has a move.
            let Some(p) = boards.iter().find(|p| p.empty_count() == e && !p.must_pass()) else {
                continue;
            };
            let r = engine
                .search(*p, &SearchRequest::new(Goal::Endgame).all_moves().skip_pv_recovery())
                .unwrap();
            let row: Vec<String> =
                r.moves.iter().map(|m| format!("{}:{}", m.action, m.score)).collect();
            println!("g{g} e{e} {}", row.join(" "));
        }
    }
}
