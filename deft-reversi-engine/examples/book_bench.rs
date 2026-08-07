//! book の読み込みと全体走査のコストを測る一時的なベンチ。
//!
//! 使い方: `book_bench enumerate <depth>` — 初期盤面から depth 手までの
//! 全局面を book に入れ、negamax などの全体走査を計測する。
use deft_reversi_engine::{Board, Book, BookElem};
use std::collections::BTreeSet;
use std::time::Instant;

fn main() {
    let depth: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(9);

    // 初期盤面から depth 手までの全局面を列挙して book に入れる。
    let t = Instant::now();
    let mut book = Book::empty();
    let mut frontier: Vec<Board> = vec![Board::new()];
    let mut seen: BTreeSet<Board> = BTreeSet::new();
    seen.insert(Board::new().unique_board());
    book.register(&Board::new(), BookElem::new(0, 10));

    for ply in 0..depth {
        let mut next = Vec::new();
        for board in &frontier {
            let mut legal = board.moves();
            if legal == 0 {
                if board.opponent_moves() == 0 {
                    continue;
                }
                let passed = board.passed();
                if seen.insert(passed.unique_board()) {
                    book.register(&passed, BookElem::new(0, 10));
                    next.push(passed);
                }
                continue;
            }
            while legal != 0 {
                let mv = legal.trailing_zeros();
                legal &= legal - 1;
                let child = board.make_move(1u64 << mv);
                if seen.insert(child.unique_board()) {
                    // 値は深さで散らしておく (negamax が実際に伝播するように)。
                    let value = ((child.player.count_ones() as i32) % 9 - 4) as i8;
                    book.register(&child, BookElem::new(value, 10));
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

    let t = Instant::now();
    let bytes = book.to_egbk3_bytes();
    println!(
        "to_egbk3_bytes         : {:>6.2} s ({} MB)",
        t.elapsed().as_secs_f64(),
        bytes.len() / 1_000_000
    );

    let t = Instant::now();
    let n_fix = book.negamax(false);
    println!(
        "negamax                : {:>6.2} s ({n_fix} values updated)",
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    book.recalculate_n_lines();
    println!(
        "recalculate_n_lines    : {:>6.2} s (root n_lines {})",
        t.elapsed().as_secs_f64(),
        book.root().unwrap().n_lines
    );

    let t = Instant::now();
    let removed = book.remove_unreachable();
    println!(
        "remove_unreachable     : {:>6.2} s ({removed} removed)",
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    let removed = book.reduce(60, 1, 2);
    println!(
        "reduce(60, 1, 2)       : {:>6.2} s ({removed} removed, {} left)",
        t.elapsed().as_secs_f64(),
        book.len()
    );

    // 対局中の参照コスト。
    let t = Instant::now();
    let mut n = 0;
    for _ in 0..100_000 {
        n += book.moves_with_value(&Board::new()).len();
    }
    println!(
        "moves_with_value x100k : {:>6.2} s ({n})",
        t.elapsed().as_secs_f64()
    );
}
