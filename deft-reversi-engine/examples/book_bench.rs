//! book の読み込み・全体走査・育成のコストを測るベンチ。
//!
//! - `book_bench enumerate <depth>` — 初期盤面から depth 手までの全局面を
//!   book に入れ、伝播・保存・参照を計測する
//! - `book_bench grow <positions> <level> <threads> [out.dbk]` — 育成を計測する。
//!   出力先を渡すと育てた book を保存する
use deft_reversi_engine::{
    grow, Board, Book, BookValue, Evaluator, GrowthPolicy, Solver, SolverOptions,
};
use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::time::Instant;

fn main() {
    let arg = |i: usize| std::env::args().nth(i);
    if arg(1).as_deref() == Some("grow") {
        let positions = arg(2).and_then(|s| s.parse().ok()).unwrap_or(200);
        let level = arg(3).and_then(|s| s.parse().ok()).unwrap_or(8);
        let threads = arg(4).and_then(|s| s.parse().ok()).unwrap_or(1);
        bench_grow(positions, level, threads, arg(5));
        return;
    }
    let depth: usize = arg(2)
        .or_else(|| arg(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(9);
    bench_enumerate(depth);
}

fn bench_enumerate(depth: usize) {
    // 初期盤面から depth 手までの全局面を列挙して book に入れる。
    let t = Instant::now();
    let mut book = Book::empty();
    let mut frontier: Vec<Board> = vec![Board::new()];
    let mut seen: BTreeSet<(u64, u64)> = BTreeSet::new();
    let key = |b: &Board| {
        let u = b.unique_board();
        (u.player, u.opponent)
    };
    seen.insert(key(&Board::new()));
    book.insert(&Board::new(), BookValue::exact(0));

    for ply in 0..depth {
        let mut next = Vec::new();
        for board in &frontier {
            let mut legal = board.moves();
            if legal == 0 {
                if board.opponent_moves() == 0 {
                    continue;
                }
                let passed = board.passed();
                if seen.insert(key(&passed)) {
                    book.insert(&passed, BookValue::exact(0));
                    next.push(passed);
                }
                continue;
            }
            while legal != 0 {
                let mv = legal.trailing_zeros();
                legal &= legal - 1;
                let child = board.make_move(1u64 << mv);
                if seen.insert(key(&child)) {
                    // 値は深さで散らしておく (伝播が実際に動くように)。
                    let value = ((child.player.count_ones() as i32) % 9 - 4) as i8;
                    book.insert(&child, BookValue::exact(value));
                    next.push(child);
                }
            }
        }
        frontier = next;
        println!("  ply {:>2}: {:>10} positions", ply + 1, book.len());
    }
    drop(seen);
    println!(
        "build (depth {depth})      : {:>6.2} s, {} positions",
        t.elapsed().as_secs_f64(),
        book.len()
    );
    println!("  table RSS            : {} MiB", peak_rss_mib());

    let t = Instant::now();
    let bytes = book.to_dbk_bytes();
    println!(
        "to_dbk_bytes           : {:>6.2} s ({} MB)",
        t.elapsed().as_secs_f64(),
        bytes.len() / 1_000_000
    );

    let t = Instant::now();
    let reloaded = Book::from_dbk_bytes(&bytes).unwrap();
    println!(
        "from_dbk_bytes         : {:>6.2} s ({} positions)",
        t.elapsed().as_secs_f64(),
        reloaded.len()
    );

    let t = Instant::now();
    let bytes = book.to_egbk3_bytes();
    println!(
        "to_egbk3_bytes         : {:>6.2} s ({} MB)",
        t.elapsed().as_secs_f64(),
        bytes.len() / 1_000_000
    );

    let t = Instant::now();
    let changed = book.propagate();
    println!(
        "propagate              : {:>6.2} s ({changed} values updated)",
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    let removed = book.prune_unreachable();
    println!(
        "prune_unreachable      : {:>6.2} s ({removed} removed)",
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    let removed = book.reduce(2);
    println!(
        "reduce(2)              : {:>6.2} s ({removed} removed, {} left)",
        t.elapsed().as_secs_f64(),
        book.len()
    );

    // 対局中の参照コスト。
    let t = Instant::now();
    let mut n = 0;
    for _ in 0..100_000 {
        n += book.moves(&Board::new()).len();
    }
    println!(
        "moves x100k            : {:>6.2} s ({n})",
        t.elapsed().as_secs_f64()
    );
    println!("peak RSS               : {} MiB", peak_rss_mib());
}

/// ここまでのピーク RSS (MiB)。取れない環境では 0。
fn peak_rss_mib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))?
                .split_whitespace()
                .nth(1)?
                .parse::<u64>()
                .ok()
        })
        .map(|kib| kib / 1024)
        .unwrap_or(0)
}

/// book の育成を計測する。
fn bench_grow(positions: usize, level: i32, threads: usize, out: Option<String>) {
    let threads = NonZeroUsize::new(threads).unwrap();
    let solver = Solver::with_options(
        std::sync::Arc::new(Evaluator::default()),
        SolverOptions::default(),
    );

    let mut book = Book::new();
    let policy = GrowthPolicy {
        level,
        max_ply: 30,
        player_error: 0,
        opponent_error: 4,
        max_positions: positions,
        time_limit: None,
        threads,
        stop: None,
    };

    let t = Instant::now();
    let report = grow(&mut book, &solver, &policy);
    println!(
        "grow positions={positions} level={level} threads={threads}: \
         {:>6.2} s ({} added, {} searched, {} rounds, {} total, {:?})",
        t.elapsed().as_secs_f64(),
        report.added,
        report.searched,
        report.rounds,
        book.len(),
        report.stopped_by,
    );
    if let Some(path) = out {
        book.save(&path).unwrap();
        println!("saved to {path}");
    }
}
