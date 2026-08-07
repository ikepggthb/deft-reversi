//! `book` サブコマンド。定石データベースを作る・育てる・調べる。

use clap::Subcommand;
use deft_reversi_engine::{
    add_line, fix_frontiers, grow, position_num_to_str, position_str_to_num, Board, Book,
    BookValue, GrowthPolicy, NameTable, Solver, SolverOptions, Trust,
};
use std::num::NonZeroUsize;
use std::time::Duration;

/// book の既定のパス。
const DEFAULT_BOOK_PATH: &str = "./book.dbk";

#[derive(Subcommand, Debug)]
pub enum BookCommand {
    /// 初期盤面だけの book を新規作成する
    New {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// 備考 (由来などを残す欄)
        #[arg(long, default_value = "")]
        note: String,
    },
    /// book の概要を表示する
    Info {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// 手数ごとの内訳も表示する
        #[arg(long)]
        by_ply: bool,
    },
    /// 指定局面の book の内容を表示する
    Show {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// 初期盤面からの手順 (例: F5D6C3)
        #[arg(long, default_value = "")]
        record: String,
    },
    /// book を育てる (最良優先)
    Grow {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        #[arg(long = "eval")]
        eval_path: String,
        /// 局面を評価する探索レベル
        #[arg(long, default_value_t = 21)]
        level: i32,
        /// これより深い手数の局面は育てない
        #[arg(long, default_value_t = 30)]
        max_ply: usize,
        /// 自分の手で許す損の累計 (石差)
        #[arg(long, default_value_t = 0)]
        player_error: i32,
        /// 相手の手で許す損の累計 (石差)
        #[arg(long, default_value_t = 4)]
        opponent_error: i32,
        /// 追加する局面数の上限
        #[arg(long, default_value_t = 100)]
        positions: usize,
        /// 打ち切る秒数
        #[arg(long)]
        seconds: Option<u64>,
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
        /// 棋譜ファイル。1 行 1 棋譜 (例: F5D6C3...)
        #[arg(long)]
        games: String,
        /// 各棋譜の何手目まで登録するか
        #[arg(long, default_value_t = 20)]
        max_ply: usize,
    },
    /// 定石名を読み込む / 書き出す
    Names {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// 読み込む定石ファイル (`名前 = 手順` 形式)
        #[arg(long)]
        import: Option<String>,
        /// 書き出し先
        #[arg(long)]
        export: Option<String>,
    },
    /// 値を積み上げ直し、frontier の食い違いを直す
    Fix {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// 初期盤面から辿れない局面を削除する
        #[arg(long)]
        prune_unreachable: bool,
    },
    /// 最善から離れた変化を削る
    Reduce {
        #[arg(long, default_value = DEFAULT_BOOK_PATH)]
        book: String,
        /// 1 手あたりの損の許容量 (石差)
        #[arg(long, default_value_t = 2)]
        max_loss: i32,
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
    /// 形式を変換する (拡張子で判別: .dbk / .egbk3 / .dat)
    Convert {
        /// 入力ファイル (.dbk / .egbk3 / .egbk2 / .egbk / edax の .dat)
        #[arg(long)]
        input: String,
        #[arg(long)]
        out: String,
        /// Edax 形式で書き出すときにヘッダへ書く探索レベル
        #[arg(long)]
        level: Option<i8>,
        /// .dbk で書き出すときに frontier 列を省く (配布用。12% 小さくなる)
        #[arg(long)]
        slim: bool,
    },
}

pub fn run(command: BookCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        BookCommand::New { book, note } => {
            let mut new_book = Book::new();
            new_book.meta_mut().note = note;
            save_any(&new_book, &book, None, false)?;
            println!("created {book}");
            Ok(())
        }
        BookCommand::Info { book, by_ply } => {
            let book = Book::load(&book)?;
            print_info(&book, by_ply);
            Ok(())
        }
        BookCommand::Show { book, record } => {
            let book = Book::load(&book)?;
            let board = board_from_record(&record)?;
            print_position(&book, &board);
            Ok(())
        }
        BookCommand::Grow {
            book,
            eval_path,
            level,
            max_ply,
            player_error,
            opponent_error,
            positions,
            seconds,
            threads,
            hash_mb,
        } => {
            // 探索の中 (YBWC) ではなく、局面ごとに並列化する。1 局面の探索が
            // 浅いほど YBWC は効きにくく、局面をばらまく方がほぼ線形に伸びる。
            let solver = open_solver(&eval_path, NonZeroUsize::MIN, hash_mb)?;
            let mut book_data = Book::load(&book)?;
            let before = book_data.len();

            let policy = GrowthPolicy {
                level,
                max_ply,
                player_error,
                opponent_error,
                max_positions: positions,
                time_limit: seconds.map(Duration::from_secs),
                threads,
                stop: None,
            };
            let report = grow(&mut book_data, &solver, &policy);
            save_any(&book_data, &book, None, false)?;

            println!(
                "grow: +{} positions ({before} -> {}), {} searched in {} rounds, {:.1}s ({:?})",
                report.added,
                book_data.len(),
                report.searched,
                report.rounds,
                report.elapsed.as_secs_f64(),
                report.stopped_by,
            );
            print_root(&book_data);
            Ok(())
        }
        BookCommand::AddGames {
            book,
            games,
            max_ply,
        } => {
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
                match moves_of_record(&record) {
                    Ok(moves) => added += add_line(&mut book_data, &moves, max_ply),
                    Err(e) => {
                        skipped += 1;
                        eprintln!("line {}: {e}", i + 1);
                    }
                }
            }
            book_data.propagate();
            save_any(&book_data, &book, None, false)?;
            println!(
                "add-games: added {added} positions ({skipped} games skipped), {} total",
                book_data.len()
            );
            eprintln!("値はまだ入っていない。`book grow` で埋めること");
            Ok(())
        }
        BookCommand::Names {
            book,
            import,
            export,
        } => {
            let mut book_data = Book::load(&book)?;
            if let Some(path) = import {
                let text = std::fs::read_to_string(&path)?;
                let names = NameTable::from_text(&text)?;
                println!("imported {} openings from {path}", names.len());
                book_data.set_names(names);
                save_any(&book_data, &book, None, false)?;
            }
            if let Some(path) = export {
                std::fs::write(&path, book_data.names().to_text())?;
                println!("exported {} openings to {path}", book_data.names().len());
            }
            if book_data.names().is_empty() {
                println!("this book has no opening names");
            }
            Ok(())
        }
        BookCommand::Fix {
            book,
            prune_unreachable,
        } => {
            let mut book_data = Book::load(&book)?;
            let removed = if prune_unreachable {
                book_data.prune_unreachable()
            } else {
                0
            };
            let fixed = fix_frontiers(&mut book_data);
            let changed = book_data.propagate();
            save_any(&book_data, &book, None, false)?;
            println!(
                "fix: {changed} values updated, {fixed} frontiers repaired, \
                 {removed} unreachable positions removed"
            );
            print_info(&book_data, false);
            Ok(())
        }
        BookCommand::Reduce { book, max_loss } => {
            let mut book_data = Book::load(&book)?;
            let before = book_data.len();
            let removed = book_data.reduce(max_loss);
            book_data.propagate();
            save_any(&book_data, &book, None, false)?;
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
            dest.propagate();
            save_any(&dest, &out, None, false)?;
            println!("merged {added} positions, {} total in {out}", dest.len());
            Ok(())
        }
        BookCommand::Convert {
            input,
            out,
            level,
            slim,
        } => {
            let book = Book::load(&input)?;
            save_any(&book, &out, level, slim)?;
            let lower = out.to_ascii_lowercase();
            // .dbk 以外は「値が無い」を表せないので、その分が落ちる。
            let dropped = if lower.ends_with(".dbk") {
                0
            } else {
                book.n_undefined()
            };
            println!("converted {} positions to {out}", book.len() - dropped);
            if dropped != 0 {
                eprintln!(
                    "{dropped} positions had no value yet and were skipped \
                     (this format cannot represent them; run `book grow` first)"
                );
            }
            Ok(())
        }
    }
}

/// 拡張子から形式を選んで保存する。
fn save_any(
    book: &Book,
    path: &str,
    edax_level: Option<i8>,
    slim: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".dat") {
        book.save_edax(path, edax_level)?;
    } else if lower.ends_with(".egbk3") || lower.ends_with(".egbk2") || lower.ends_with(".egbk") {
        book.save_egbk3(path)?;
    } else {
        book.save_dbk_with(path, !slim)?;
    }
    Ok(())
}

fn open_solver(
    eval_path: &str,
    search_threads: NonZeroUsize,
    hash_mb: Option<usize>,
) -> Result<Solver, Box<dyn std::error::Error>> {
    let opts = SolverOptions {
        search_threads,
        tt_capacity: hash_mb,
        ..SolverOptions::default()
    };
    // book の育成は評価器が違えば結果がまるごと変わる。既定の評価器に黙って
    // 落ちると使い物にならない book ができるので、ここは失敗させる。
    Solver::from_file(eval_path, opts)
        .map_err(|e| format!("failed to load the evaluator from {eval_path}: {e}").into())
}

/// 手順を座標の並びに直す。パスは自動で処理する。
fn moves_of_record(record: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let record = record.trim();
    if record.len() % 2 != 0 {
        return Err("record length must be even".into());
    }
    let mut board = Board::new();
    let mut moves = Vec::with_capacity(record.len() / 2);
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
        moves.push(mv);
        board = board.make_move(1u64 << mv);
    }
    Ok(moves)
}

/// 初期盤面から手順を進めた盤面を返す。
fn board_from_record(record: &str) -> Result<Board, Box<dyn std::error::Error>> {
    let mut board = Board::new();
    for mv in moves_of_record(record)? {
        if board.moves() == 0 {
            board = board.passed();
        }
        board = board.make_move(1u64 << mv);
    }
    Ok(board)
}

fn print_info(book: &Book, by_ply: bool) {
    let meta = book.meta();
    println!("positions : {}", book.len());
    println!("max ply   : {}", book.max_ply().unwrap_or(0));
    println!(
        "grown at  : level {}, up to ply {}, error {}/{} (self/opponent)",
        meta.level, meta.max_ply, meta.player_error, meta.opponent_error
    );
    if !meta.note.is_empty() {
        println!("note      : {}", meta.note);
    }
    println!("openings  : {}", book.names().len());

    // 信用度の内訳。どれだけ「詰まっている」book なのかが一目で分かる。
    let mut trust = [0usize; 4];
    for (_, _, value) in book.iter() {
        trust[trust_index(Trust::of(value))] += 1;
    }
    println!(
        "trust     : exact {}, high {}, low {}, none {}",
        trust[3], trust[2], trust[1], trust[0]
    );

    if by_ply {
        println!("by ply    :");
        for ply in 0..=book.max_ply().unwrap_or(0) {
            let n = book.table().layer(ply).len();
            if n != 0 {
                println!("  {ply:>2} : {n}");
            }
        }
    }
    print_root(book);
}

fn trust_index(trust: Trust) -> usize {
    match trust {
        Trust::None => 0,
        Trust::Low => 1,
        Trust::High => 2,
        Trust::Exact => 3,
    }
}

fn print_root(book: &Book) {
    match book.value_of(&Board::new()) {
        Some(value) => println!(
            "root      : {} (trust {})",
            format_value(&value),
            Trust::of(&value)
        ),
        None => println!("root      : not registered"),
    }
}

fn format_value(value: &BookValue) -> String {
    if !value.is_defined() {
        return "?".to_string();
    }
    if value.lower == value.upper {
        return format!("{:+}", value.score);
    }
    format!("{:+} [{:+}, {:+}]", value.score, value.lower, value.upper)
}

fn print_position(book: &Book, board: &Board) {
    print_board(board);

    let Some(probe) = book.probe(board) else {
        println!("this position is not in the book");
        return;
    };

    println!("value : {}", format_value(&probe.value));
    println!("trust : {}", probe.trust());
    if probe.value.is_defined() {
        println!(
            "search: depth {}, selectivity {}",
            probe.value.depth, probe.value.selectivity
        );
    }
    let registered = book.registered_moves(board).count_ones();
    println!(
        "moves : {registered} / {} legal registered, {} with a value{}",
        board.moves().count_ones(),
        probe.moves.len(),
        if probe.complete { " (complete)" } else { "" }
    );
    if !probe.is_consistent() {
        println!("warn  : this position disagrees with its children");
    }
    if !probe.names.is_empty() {
        println!("name  : {}", probe.names.join(", "));
    }
    if !probe.upcoming.is_empty() {
        // 序盤ほど通る定石が多いので、全部並べると読めなくなる。
        println!("line  : {}", summarize(&probe.upcoming, 8));
    }

    for child in book.children(board) {
        println!(
            "  {} {:>12}  {}",
            move_to_string(child.mv),
            format_value(&child.value),
            Trust::of(&child.value)
        );
    }
    if registered == 0 {
        println!("  (no child position is registered)");
    }

    if let Some(frontier) = book.frontier_of(board) {
        if frontier.has_move() {
            println!(
                "next  : {} ({:+}, depth {}) — まだ book に無い手の中の最善",
                move_to_string(frontier.mv as u8),
                frontier.score,
                frontier.depth
            );
        } else if frontier.is_exhausted() {
            println!("next  : (all legal moves are in the book)");
        }
    }
}

/// 名前が多すぎるときは頭から `limit` 件だけ出す。
fn summarize(names: &[&str], limit: usize) -> String {
    if names.len() <= limit {
        return names.join(", ");
    }
    format!(
        "{}, ... (+{})",
        names[..limit].join(", "),
        names.len() - limit
    )
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
    fn moves_of_record_returns_the_coordinates() {
        assert_eq!(moves_of_record("F5D6").unwrap(), vec![37, 43]);
    }

    #[test]
    fn move_to_string_formats_coordinates() {
        assert_eq!(move_to_string(0), "A1");
        assert_eq!(move_to_string(37), "F5");
        assert_eq!(move_to_string(63), "H8");
    }

    #[test]
    fn format_value_shows_the_window_only_when_it_is_open() {
        assert_eq!(format_value(&BookValue::exact(4)), "+4");
        assert_eq!(format_value(&BookValue::undefined()), "?");
        let open = BookValue::from_search(4, 40, 12, 3, 3);
        assert_eq!(format_value(&open), "+4 [+1, +7]");
    }

    #[test]
    fn summarize_caps_long_lists() {
        assert_eq!(summarize(&["a", "b"], 8), "a, b");
        let many: Vec<&str> = vec!["x"; 12];
        assert_eq!(summarize(&many, 3), "x, x, x, ... (+9)");
    }

    #[test]
    fn save_any_picks_the_format_from_the_extension() {
        let dir = std::env::temp_dir().join(format!("book-cmd-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let book = Book::new();

        for (name, magic) in [
            ("a.dbk", &b"DEFTBOOK"[..]),
            ("a.egbk3", &b"DICUORAGE"[..]),
            ("a.dat", &b"XADEKOOB"[..]),
        ] {
            let path = dir.join(name);
            let path = path.to_str().unwrap();
            save_any(&book, path, None, false).unwrap();
            let bytes = std::fs::read(path).unwrap();
            assert!(bytes.starts_with(magic), "{name}");
            // 書いたものをそのまま読み戻せること。
            assert!(Book::load(path).is_ok(), "{name}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
