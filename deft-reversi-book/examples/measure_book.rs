//! Measure load, save, allocation estimates and allocation-free candidate lookup.
//! Arguments: PLAY WORK COPY_OF_PLAY. The input files are read only.
use deft_reversi_book::*;
use deft_reversi_engine::Board;
use std::{hint::black_box, time::Instant};
fn main() {
    let a: Vec<_> = std::env::args().collect();
    let limit = ReadLimits {
        max_bytes: 2 * 1024 * 1024 * 1024,
        ..Default::default()
    };
    let now = Instant::now();
    let book = Book::load(&a[1], limit).unwrap();
    let load = now.elapsed().as_secs_f64();
    let now = Instant::now();
    book.save(&a[3]).unwrap();
    let save = now.elapsed().as_secs_f64();
    let stats = book.stats();
    let board = Board::new();
    assert!(book.probe(board).is_some());
    let now = Instant::now();
    let mut sum = 0i64;
    for _ in 0..1_000_000 {
        let entry = book.probe(black_box(board)).unwrap();
        sum += entry
            .moves()
            .filter_map(|c| c.value)
            .map(|v| v.score() as i64)
            .sum::<i64>();
    }
    black_box(sum);
    let ns = now.elapsed().as_nanos() as f64 / 1_000_000.0;
    let now = Instant::now();
    let editor = BookEditor::load(&a[2], limit).unwrap();
    let work_load = now.elapsed().as_secs_f64();
    println!(
        "{{\"load_s\":{load},\"save_s\":{save},\"probe_with_candidates_ns\":{ns},\"play_resident\":{},\"work_load_s\":{work_load},\"work_managed\":{},\"positions\":{},\"candidates\":{}}}",
        stats.resident_bytes,
        editor.estimated_bytes(),
        stats.positions,
        stats.candidates
    );
}
