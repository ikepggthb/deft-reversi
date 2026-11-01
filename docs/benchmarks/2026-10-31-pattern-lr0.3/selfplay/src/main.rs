//! Self-play between two models: every random opening is played twice with
//! colours swapped. Usage:
//! selfplay MODEL_A MODEL_B LEVEL OPENINGS PLIES THREADS SEED OUT.tsv
use deft_reversi_engine::{
    Action, Color, Engine, EngineOptions, Goal, Model, Position, SearchRequest,
};
use std::{
    collections::HashSet,
    error::Error,
    io::Write,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn openings(count: usize, plies: usize, seed: u64) -> Vec<Position> {
    let mut state = seed | 1;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    while out.len() < count {
        let mut pos = Position::initial();
        for _ in 0..plies {
            if pos.is_terminal() {
                break;
            }
            let action = if pos.must_pass() {
                Action::Pass
            } else {
                let moves: Vec<_> = pos.legal_moves().into_iter().collect();
                Action::Place(moves[(next(&mut state) % moves.len() as u64) as usize])
            };
            pos = pos.play(action).unwrap();
        }
        if pos.is_terminal() || pos.must_pass() {
            continue;
        }
        let board: deft_reversi_engine::Board = pos.into();
        if seen.insert(board.canonical()) {
            out.push(pos);
        }
    }
    out
}

fn engine(path: &str) -> Result<Engine, Box<dyn Error>> {
    let mut e = Engine::new(
        Model::from_file(path)?,
        EngineOptions::default().threads(1).tt_mib(64),
    )?;
    e.set_legacy_main_mpc()?;
    Ok(e)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [a, b, level, count, plies, threads, seed, out] = &args[..] else {
        return Err("usage: selfplay MODEL_A MODEL_B LEVEL OPENINGS PLIES THREADS SEED OUT.tsv".into());
    };
    let level: u8 = level.parse()?;
    let starts = openings(count.parse()?, plies.parse()?, seed.parse()?);
    let jobs = starts.len() * 2;
    let cursor = AtomicUsize::new(0);
    let file = Mutex::new(std::fs::File::create(out)?);
    writeln!(file.lock().unwrap(), "opening\ta_color\ta_score\tstart")?;
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads.parse::<usize>().unwrap() {
            scope.spawn(|| {
                let mut ea = engine(a).unwrap();
                let mut eb = engine(b).unwrap();
                let request = SearchRequest::new(Goal::Level(level));
                loop {
                    let job = cursor.fetch_add(1, Ordering::Relaxed);
                    if job >= jobs {
                        break;
                    }
                    let a_color = if job % 2 == 0 { Color::Black } else { Color::White };
                    let start = starts[job / 2];
                    ea.clear_tt().unwrap();
                    eb.clear_tt().unwrap();
                    let mut pos = start;
                    while !pos.is_terminal() {
                        let action = if pos.must_pass() {
                            Action::Pass
                        } else {
                            let e = if pos.side_to_move() == a_color { &mut ea } else { &mut eb };
                            e.search(pos, &request).unwrap().best_move.unwrap()
                        };
                        pos = pos.play(action).unwrap();
                    }
                    let score = pos.final_score(a_color);
                    writeln!(
                        file.lock().unwrap(),
                        "{}\t{:?}\t{}\t{}",
                        job / 2,
                        a_color,
                        score,
                        start
                    )
                    .unwrap();
                    results.lock().unwrap().push((job / 2, score));
                }
            });
        }
    });
    let games = results.into_inner().unwrap();
    let mut pairs = vec![0i32; starts.len()];
    for &(o, s) in &games {
        pairs[o] += s;
    }
    let r: Vec<i32> = games.iter().map(|&(_, s)| s).collect();
    let (w, d, l) = (
        r.iter().filter(|&&s| s > 0).count(),
        r.iter().filter(|&&s| s == 0).count(),
        r.iter().filter(|&&s| s < 0).count(),
    );
    let n = r.len() as f64;
    let p = (w as f64 + 0.5 * d as f64) / n;
    let mean = r.iter().map(|&s| f64::from(s)).sum::<f64>() / n;
    // Both games of an opening share its bias, so the error is over pairs.
    let m = pairs.len() as f64;
    let pair_mean = pairs.iter().map(|&s| f64::from(s) / 2.0).sum::<f64>() / m;
    let var = pairs.iter().map(|&s| (f64::from(s) / 2.0 - pair_mean).powi(2)).sum::<f64>() / (m - 1.0);
    let elo = |p: f64| -400.0 * (1.0 / p - 1.0).log10();
    let se = (p * (1.0 - p) / n).sqrt();
    println!(
        "games {}  A wins {w} draws {d} losses {l}  score {:.1}%  elo {:+.0} [{:+.0}, {:+.0}]  mean disc diff {:+.2} ± {:.2}",
        r.len(),
        p * 100.0,
        elo(p),
        elo(p - 1.96 * se),
        elo(p + 1.96 * se),
        mean,
        1.96 * (var / m).sqrt()
    );
    Ok(())
}
