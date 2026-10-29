use deft_reversi_engine::{Action, Color, Position, Square};
use std::io::{self, Write};

// cargo run -p deft_reversi_engine --example show_board
// RUSTFLAGS="-Ctarget-cpu=x86-64-v3" cargo run -p deft_reversi_engine --example show_board

fn main() -> io::Result<()> {
    let mut position = Position::initial();

    println!("座標（例: D3）を入力してください。q で終了します。");

    loop {
        print_board(&position);

        if position.is_terminal() {
            print_result(&position);
            break;
        }
        if position.must_pass() {
            println!("合法手がないためパスします。\n");
            position = position.play(Action::Pass).unwrap();
            continue;
        }

        let side = match position.side_to_move() {
            Color::Black => "黒(X)",
            Color::White => "白(O)",
        };
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

        let Ok(square) = input.parse::<Square>() else {
            println!("A1 から H8 までの座標を入力してください。\n");
            continue;
        };

        match position.play(square) {
            Ok(next) => position = next,
            Err(_) => {
                println!("そこには打てません。* のマスを選んでください。\n");
                continue;
            }
        }
        println!();
    }

    Ok(())
}

fn print_board(position: &Position) {
    let moves = position.legal_moves();

    println!("  A B C D E F G H");
    for y in 0..8 {
        print!("{} ", y + 1);
        for x in 0..8 {
            let square = Square::from_xy(x, y).unwrap();
            let symbol = if position.black() & square.bit() != 0 {
                'X'
            } else if position.white() & square.bit() != 0 {
                'O'
            } else if moves.contains(square) {
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

fn print_result(position: &Position) {
    let black = position.disc_count(Color::Black);
    let white = position.disc_count(Color::White);

    println!("対局終了: 黒 {black} - 白 {white}");
    match black.cmp(&white) {
        std::cmp::Ordering::Greater => println!("黒の勝ちです。"),
        std::cmp::Ordering::Less => println!("白の勝ちです。"),
        std::cmp::Ordering::Equal => println!("引き分けです。"),
    }
}
