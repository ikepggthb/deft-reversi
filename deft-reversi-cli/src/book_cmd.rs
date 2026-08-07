//! `book` サブコマンド。Egaroucid 互換の定石データベースを作る・調べる。

use clap::Subcommand;
use deft_reversi_engine::{
    position_num_to_str, position_str_to_num, Board, Book, Evaluator, Solver, SolverOptions,
};
use std::num::NonZeroUsize;
use std::sync::Arc;

/// book の既定のパス。
const DEFAULT_BOOK_PATH: &str = "./book.egbk3";

#[derive(Subcommand, Debug)]
pub enum BookCommand {
    /// 初期盤面だけの book を新規作成する
    New {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
    },
    /// book の概要を表示する
    Info {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
    },
    /// 指定局面の book の手を表示する
    Show {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// 初期盤面からの手順 (例: F5D6C3)
        #[arg(long, default_value = "")]
        record: String,
    },
    /// 局面を探索して book に登録する
    Add {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        #[arg(long = "eval")]
        eval_path: String,
        /// 初期盤面からの手順 (例: F5D6C3)
        #[arg(long, default_value = "")]
        record: String,
        #[arg(long, default_value_t = 21)]
        book_level: i32,
        #[arg(long, default_value_t = NonZeroUsize::MIN)]
        threads: NonZeroUsize,
        #[arg(long)]
        hash_mb: Option<usize>,
    },
    /// leaf を探索して book を広げる
    Expand {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        #[arg(long = "eval")]
        eval_path: String,
        /// 局面を評価する探索レベル
        #[arg(long, default_value_t = 21)]
        book_level: i32,
        /// その局面の評価値と leaf の値の差の許容量
        #[arg(long, default_value_t = 2)]
        max_error: i32,
        /// 繰り返し回数の上限 (0 で変化が無くなるまで)
        #[arg(long, default_value_t = 1)]
        rounds: usize,
        /// 同時に探索する局面数。book の育成では探索の中を並列化するより
        /// 局面をばらまく方が効率が良い
        #[arg(long, default_value_t = NonZeroUsize::MIN)]
        threads: NonZeroUsize,
        #[arg(long)]
        hash_mb: Option<usize>,
    },
    /// 棋譜ファイル (1 行 1 棋譜) の局面を book に追加する
    AddGames {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        #[arg(long = "eval")]
        eval_path: String,
        /// 棋譜ファイル。1 行 1 棋譜 (例: F5D6C3...)
        #[arg(long)]
        games: String,
        #[arg(long, default_value_t = 21)]
        book_level: i32,
        /// 各棋譜の何手目まで登録するか
        #[arg(long, default_value_t = 20)]
        max_moves: usize,
        #[arg(long, default_value_t = NonZeroUsize::MIN)]
        threads: NonZeroUsize,
        #[arg(long)]
        hash_mb: Option<usize>,
    },
    /// 評価値を伝播させて leaf の整合を取る (egaroucid の book fix)
    Fix {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// leaf の値も評価値の候補にする (Edax と同じ扱い)
        #[arg(long)]
        edax_compliant: bool,
        /// 初期盤面から辿れない局面を削除する
        #[arg(long)]
        prune_unreachable: bool,
    },
    /// 最善手から離れた変化を削る (egaroucid の book reduce)
    Reduce {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        #[arg(long, default_value_t = 30)]
        max_depth: usize,
        /// 1 手あたりの誤差の許容量
        #[arg(long, default_value_t = 2)]
        max_error_per_move: i32,
        /// 1 本の変化を通しての誤差合計の許容量
        #[arg(long, default_value_t = 4)]
        max_line_error: i32,
    },
    /// 2 つの book を統合する
    Merge {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// 取り込む側の book
        #[arg(long)]
        with: String,
        #[arg(long)]
        out: String,
    },
    /// 別形式の book を .egbk3 に変換する (.egbk2 / .egbk / edax の .dat)
    Convert {
        /// 入力ファイル
        #[arg(long)]
        input: String,
        #[arg(long)]
        out: String,
    },
    /// Edax 形式 (.dat) で書き出す
    ExportEdax {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        #[arg(long)]
        out: String,
        /// ヘッダに書く探索レベル。省略すると book 内の最大値
        #[arg(long)]
        level: Option<i8>,
    },
}

pub fn run(command: BookCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        BookCommand::New { book } => {
            let new_book = Book::new();
            new_book.save(&book)?;
            println!("created {book}");
            Ok(())
        }
        BookCommand::Info { book } => {
            let book = Book::load(&book)?;
            print_info(&book);
            Ok(())
        }
        BookCommand::Show { book, record } => {
            let book = Book::load(&book)?;
            let board = board_from_record(&record)?;
            print_position(&book, &board);
            Ok(())
        }
        BookCommand::Add {
            book,
            eval_path,
            record,
            book_level,
            threads,
            hash_mb,
        } => {
            let solver = open_solver(&eval_path, threads, hash_mb)?;
            let mut book_data = Book::load(&book)?;
            let board = board_from_record(&record)?;
            let added = book_data.add_board(&board, book_level, &solver);
            book_data.negamax(false);
            book_data.save(&book)?;
            println!(
                "add: {}, {} positions",
                if added { "registered" } else { "updated" },
                book_data.len()
            );
            Ok(())
        }
        BookCommand::Expand {
            book,
            eval_path,
            book_level,
            max_error,
            rounds,
            threads,
            hash_mb,
        } => {
            // 探索の中 (YBWC) ではなく、局面ごとに並列化する。1 局面の探索が
            // 浅いほど YBWC は効きにくく、局面をばらまく方がほぼ線形に伸びる。
            let solver = open_solver(&eval_path, NonZeroUsize::MIN, hash_mb)?;
            let mut book_data = Book::load(&book)?;
            let added =
                book_data.expand_with_threads(rounds, max_error, book_level, &solver, threads);
            book_data.save(&book)?;
            println!("expand: added {added} positions, {} total", book_data.len());
            Ok(())
        }
        BookCommand::AddGames {
            book,
            eval_path,
            games,
            book_level,
            max_moves,
            threads,
            hash_mb,
        } => {
            let solver = open_solver(&eval_path, threads, hash_mb)?;
            let mut book_data = Book::load(&book)?;
            let text = std::fs::read_to_string(&games)?;

            let mut added = 0;
            let mut skipped = 0;
            for (i, line) in text.lines().enumerate() {
                let record: String = line
                    .trim()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if record.is_empty() {
                    continue;
                }
                match book_data.add_record(&record, max_moves, book_level, &solver) {
                    Ok(n) => added += n,
                    Err(e) => {
                        skipped += 1;
                        eprintln!("line {}: {e}", i + 1);
                    }
                }
            }
            book_data.negamax(false);
            book_data.save(&book)?;
            println!(
                "add-games: added {added} positions ({skipped} games skipped), {} total",
                book_data.len()
            );
            Ok(())
        }
        BookCommand::Fix {
            book,
            edax_compliant,
            prune_unreachable,
        } => {
            let mut book_data = Book::load(&book)?;
            let removed = if prune_unreachable {
                book_data.remove_unreachable()
            } else {
                0
            };
            let n_fix = book_data.fix(edax_compliant);
            book_data.save(&book)?;
            println!("fix: {n_fix} values updated, {removed} unreachable positions removed");
            print_info(&book_data);
            Ok(())
        }
        BookCommand::Reduce {
            book,
            max_depth,
            max_error_per_move,
            max_line_error,
        } => {
            let mut book_data = Book::load(&book)?;
            let before = book_data.len();
            let removed = book_data.reduce(max_depth, max_error_per_move, max_line_error);
            book_data.negamax(false);
            book_data.save(&book)?;
            println!(
                "reduce: removed {removed} positions ({before} -> {})",
                book_data.len()
            );
            Ok(())
        }
        BookCommand::Merge { book, with, out } => {
            let mut dest = Book::load(&book)?;
            let source = Book::load(&with)?;
            let added = dest.merge(&source);
            dest.negamax(false);
            dest.save(&out)?;
            println!("merged {added} positions, {} total in {out}", dest.len());
            Ok(())
        }
        BookCommand::Convert { input, out } => {
            let book = Book::load(&input)?;
            book.save(&out)?;
            println!("converted {} positions to {out}", book.len());
            Ok(())
        }
        BookCommand::ExportEdax { book, out, level } => {
            let book = Book::load(&book)?;
            book.save_edax(&out, level)?;
            println!("exported {} positions to {out} (edax format)", book.len());
            Ok(())
        }
    }
}

fn open_solver(
    eval_path: &str,
    search_threads: NonZeroUsize,
    hash_mb: Option<usize>,
) -> Result<Solver, Box<dyn std::error::Error>> {
    let opts = || SolverOptions {
        search_threads,
        tt_capacity: hash_mb,
        ..SolverOptions::default()
    };
    match Solver::from_file(eval_path, opts()) {
        Ok(solver) => Ok(solver),
        Err(e) => {
            eprintln!("Evaluator: {e} (falling back to the default evaluator)");
            Ok(Solver::with_options(Arc::new(Evaluator::default()), opts()))
        }
    }
}

/// 初期盤面から手順を進めた盤面を返す。パスは自動で処理する。
fn board_from_record(record: &str) -> Result<Board, Box<dyn std::error::Error>> {
    let record = record.trim();
    if record.len() % 2 != 0 {
        return Err("record length must be even".into());
    }
    let mut board = Board::new();
    for chunk in record.as_bytes().chunks(2) {
        let move_str = std::str::from_utf8(chunk)?;
        if board.moves() == 0 {
            if board.opponent_moves() == 0 {
                return Err(format!("game is already over before {move_str}").into());
            }
            board = board.passed();
        }
        let mv = position_str_to_num(move_str)?;
        if board.moves() & (1u64 << mv) == 0 {
            return Err(format!("illegal move: {move_str}").into());
        }
        board = board.make_move(1u64 << mv);
    }
    Ok(board)
}

fn print_info(book: &Book) {
    println!("positions : {}", book.len());

    let mut level_min = i8::MAX;
    let mut level_max = i8::MIN;
    let mut n_leaves = 0;
    for (_, elem) in book.iter() {
        level_min = level_min.min(elem.level);
        level_max = level_max.max(elem.level);
        if elem.leaf.is_move() {
            n_leaves += 1;
        }
    }
    if !book.is_empty() {
        println!("level     : {level_min} .. {level_max}");
        println!("leaves    : {n_leaves}");
    }

    match book.root() {
        Some(root) => {
            println!("root value: {:+}", root.value);
            println!("root lines: {}", root.n_lines);
        }
        None => println!("root      : not registered"),
    }
}

fn print_position(book: &Book, board: &Board) {
    print_board(board);

    let Some(elem) = book.get(board) else {
        println!("this position is not in the book");
        return;
    };
    println!("value : {:+}", elem.value);
    println!("level : {}", elem.level);
    println!("lines : {}", elem.n_lines);
    if elem.leaf.is_move() {
        println!(
            "leaf  : {} ({:+}, level {})",
            move_to_string(elem.leaf.mv as u8),
            elem.leaf.value,
            elem.leaf.level
        );
    }

    let mut moves = book.moves_with_value(board);
    moves.sort_by(|a, b| b.value.cmp(&a.value));
    print!("moves :");
    if moves.is_empty() {
        print!(" (no child position is registered)");
    }
    for m in moves {
        print!(" {}:{:+}", move_to_string(m.mv), m.value);
    }
    println!();
}

fn print_board(board: &Board) {
    let (player, opponent) = (board.player, board.opponent);
    println!("  A B C D E F G H");
    for row in 0..8 {
        print!("{} ", row + 1);
        for col in 0..8 {
            let bit = 1u64 << (row * 8 + col);
            let c = if player & bit != 0 {
                'X'
            } else if opponent & bit != 0 {
                'O'
            } else {
                '-'
            };
            print!("{c} ");
        }
        println!();
    }
}

fn move_to_string(mv: u8) -> String {
    position_num_to_str(mv).unwrap_or_else(|_| "??".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_from_record_replays_moves() {
        let board = board_from_record("F5").unwrap();
        assert_eq!(board, Board::new().make_move(1u64 << 37));
        assert_eq!(board_from_record("").unwrap(), Board::new());
    }

    #[test]
    fn board_from_record_rejects_bad_input() {
        assert!(board_from_record("F").is_err());
        assert!(board_from_record("A1").is_err());
    }

    #[test]
    fn move_to_string_formats_coordinates() {
        assert_eq!(move_to_string(0), "A1");
        assert_eq!(move_to_string(37), "F5");
        assert_eq!(move_to_string(63), "H8");
    }
}
