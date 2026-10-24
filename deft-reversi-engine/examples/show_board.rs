use deft_reversi_engine::Board;
use std::io::{self, Write};

// cargo run -p deft_reversi_engine --example show_board
// RUSTFLAGS="-Ctarget-cpu=x86-64-v3" cargo run -p deft_reversi_engine --example show_board

fn main() -> io::Result<()> {
    let mut board = Board::new();
    let mut black_to_move = true;

    println!("座標（例: D3）を入力してください。q で終了します。");

    loop {
        print_board(&board, black_to_move);

        let moves = board.get_moves();
        if moves == 0 {
            if board.get_opponent_moves() == 0 {
                print_result(&board, black_to_move);
                break;
            }

            println!("合法手がないためパスします。\n");
            board.try_pass().unwrap();
            black_to_move = !black_to_move;
            continue;
        }

        let side = if black_to_move { "黒(X)" } else { "白(O)" };
        print!("{side} の手 > ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }

        let input = input.trim();
        if input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
            println!("終了します。");
            break;
        }

        let Some(move_mask) = parse_move(input) else {
            println!("A1 から H8 までの座標を入力してください。\n");
            continue;
        };

        if moves & move_mask == 0 {
            println!("そこには打てません。* のマスを選んでください。\n");
            continue;
        }

        board.try_play(move_mask).unwrap();
        black_to_move = !black_to_move;
        println!();
    }

    Ok(())
}

fn parse_move(input: &str) -> Option<u64> {
    let bytes = input.as_bytes();
    if bytes.len() != 2 {
        return None;
    }

    let file = bytes[0].to_ascii_uppercase();
    let rank = bytes[1];
    if !(b'A'..=b'H').contains(&file) || !(b'1'..=b'8').contains(&rank) {
        return None;
    }

    let index = (rank - b'1') * 8 + (file - b'A');
    Some(1_u64 << index)
}

fn print_board(board: &Board, black_to_move: bool) {
    let moves = board.get_moves();
    let (black, white) = if black_to_move {
        (board.player_bits(), board.opponent_bits())
    } else {
        (board.opponent_bits(), board.player_bits())
    };

    println!("  A B C D E F G H");
    for rank in 0..8 {
        print!("{} ", rank + 1);

        for file in 0..8 {
            let square = 1_u64 << (rank * 8 + file);
            let symbol = if black & square != 0 {
                'X'
            } else if white & square != 0 {
                'O'
            } else if moves & square != 0 {
                '*'
            } else {
                '.'
            };

            print!("{symbol} ");
        }

        println!();
    }

    println!();
    println!("X: 黒, O: 白, *: 合法手");
}

fn print_result(board: &Board, black_to_move: bool) {
    let (black, white) = if black_to_move {
        (board.player_bits(), board.opponent_bits())
    } else {
        (board.opponent_bits(), board.player_bits())
    };
    let black_count = black.count_ones();
    let white_count = white.count_ones();

    println!("対局終了: 黒 {black_count} - 白 {white_count}");
    match black_count.cmp(&white_count) {
        std::cmp::Ordering::Greater => println!("黒の勝ちです。"),
        std::cmp::Ordering::Less => println!("白の勝ちです。"),
        std::cmp::Ordering::Equal => println!("引き分けです。"),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_move;

    #[test]
    fn parses_board_coordinates() {
        assert_eq!(parse_move("A1"), Some(1));
        assert_eq!(parse_move("d3"), Some(1_u64 << 19));
        assert_eq!(parse_move("H8"), Some(1_u64 << 63));
    }

    #[test]
    fn rejects_invalid_coordinates() {
        assert_eq!(parse_move(""), None);
        assert_eq!(parse_move("A0"), None);
        assert_eq!(parse_move("I1"), None);
        assert_eq!(parse_move("D3x"), None);
    }
}
