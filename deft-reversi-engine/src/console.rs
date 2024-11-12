use crate::{board::*, eval::*, game::*};


fn print_board(game_state: &Game) {
    let b = game_state.get_board();
    println!(" ABCDEFGH");
    for y in 0..8 {
        print!("{}", y+1);
        for x in 0..8 {
            let mask = 1u64 << (y * 8 + x);
            if b.bit_board[Board::BLACK] & mask != 0 {
                print!("X");
            } else if b.bit_board[Board::WHITE] & mask != 0 {
                print!("O");
            } else {
                print!(".");
            }
        }
        println!();
    }
}

fn print_record (game_state: &Game) {
    let record = game_state.record();
    println!("record: {}", record);
}

fn print_move (game_state: &Game) {
    print!("move: ");
    for i in 0..64{
        let mask = 1u64 << i;
        let bit_p = mask & game_state.get_board().put_able();
        if bit_p != 0 {
            print!("{}, ", position_bit_to_str(bit_p).unwrap());
        }
    }
    println!();
}

fn input() -> String{
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).unwrap();
    return buf.trim().parse().unwrap();
}

pub fn console_game() {

    let evaluator: Evaluator = Evaluator::read_file("res/eval.json").unwrap();
    let mut game_state = Game::new(evaluator);
    game_state.set_ai_level(2);

    loop {
        if game_state.get_board().next_turn == Board::BLACK {
            print_board(&game_state);
            print_record(&game_state);
            print_move(&game_state);

            println!("Your turn (enter position like 'E3'):");
            let stdin = input();
            if let Err(err) = game_state.put(&stdin) {
                println!("{}", err)
            }
            continue;
        } else {
            game_state.ai_put();
        }
    }

}