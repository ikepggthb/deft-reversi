use deft_reversi_engine::{Board, perft};
use std::time::Instant;

fn main() {
    let depth = std::env::args()
        .nth(1)
        .map(|s| s.parse::<u8>().expect("depth must be 0..=255"))
        .unwrap_or(9);
    let start = Instant::now();
    let nodes = perft(std::hint::black_box(Board::new()), depth);
    let elapsed = start.elapsed();
    println!(
        "depth={depth} leaves={nodes} elapsed={elapsed:?} leaves/s={:.0}",
        nodes as f64 / elapsed.as_secs_f64()
    );
}
