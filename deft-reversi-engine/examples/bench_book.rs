//! Read-only probe benchmark. Never generates a book or starts engine searches.
use deft_reversi_engine::{
    Board,
    book::{Book, ReadLimits},
};
use std::{error::Error, hint::black_box, time::Instant};
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(2..=4).contains(&args.len()) {
        return Err("usage: bench_book BOOK POSITIONS.obf [QUERIES=10000] [READ_MiB=512]".into());
    }
    let count = args
        .get(2)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(10000);
    if !(1..=1_000_000).contains(&count) {
        return Err("queries must be 1..=1000000".into());
    }
    let max_bytes = args
        .get(3)
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(512)
        .checked_mul(1024 * 1024)
        .ok_or("read limit overflow")?;
    let mut positions = Vec::new();
    for line in std::fs::read_to_string(&args[1])?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let cells = parts.next().ok_or("board required")?;
        let turn = parts.next().ok_or("turn required")?.trim_end_matches(';');
        if cells.len() != 64 {
            return Err("board must contain 64 cells".into());
        }
        let (mut p, mut o) = (0, 0);
        for (i, c) in cells.bytes().enumerate() {
            match c {
                b'X' | b'x' | b'*' => p |= 1 << i,
                b'O' | b'o' => o |= 1 << i,
                b'-' | b'.' => {}
                _ => return Err("invalid cell".into()),
            }
        }
        let (p, o) = match turn {
            "X" | "x" | "*" => (p, o),
            "O" | "o" => (o, p),
            _ => return Err("invalid side".into()),
        };
        positions.push(Board::from_bits(p, o).ok_or("overlapping stones")?);
    }
    if positions.is_empty() {
        return Err("no positions".into());
    }
    let load = Instant::now();
    let book = Book::load(
        &args[0],
        ReadLimits {
            max_bytes,
            max_positions: max_bytes / 16,
            max_candidates: max_bytes / 2,
        },
    )?;
    let load_seconds = load.elapsed().as_secs_f64();
    let stats = book.stats();
    let mut samples = Vec::with_capacity(count);
    let mut hits = 0;
    // Warm the supplied working set once. Results are warm-query measurements.
    for &board in &positions {
        black_box(book.probe(black_box(board)));
    }
    for i in 0..count {
        let start = Instant::now();
        let entry = book.probe(black_box(positions[i % positions.len()]));
        if let Some(entry) = entry {
            let checksum = entry.candidates().fold(0u64, |v, c| {
                v.wrapping_add(match c.action {
                    deft_reversi_engine::Action::Pass => 64,
                    deft_reversi_engine::Action::Place(s) => u64::from(s.index()),
                })
                .wrapping_add(c.value.map_or(0, |v| i64::from(v.score) as u64))
            });
            black_box(checksum);
            hits += 1;
        }
        samples.push(start.elapsed().as_nanos());
    }
    samples.sort_unstable();
    let percentile = |p: usize| samples[(count * p).div_ceil(100).saturating_sub(1)];
    println!(
        "{}",
        serde_json::json!({"positions":stats.positions,"candidates":stats.candidates,"resident_bytes":stats.resident_bytes,"load_seconds":load_seconds,"queries":count,"hits":hits,"p50_ns":percentile(50),"p95_ns":percentile(95),"p99_ns":percentile(99),"includes":"normalization, lookup, candidate coordinate restoration, timer overhead; warm supplied working set"})
    );
    Ok(())
}
