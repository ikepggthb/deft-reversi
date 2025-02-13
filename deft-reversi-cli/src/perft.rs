use deft_reversi_engine::*;
use std::time;

pub struct Perft {
    is_count_pass : bool,
    n_leaf_node   : u64,
    n_passed      : u64,
    n_end         : u64, // leaf node を含まない
}

impl Perft {        
    fn new(count_pass: bool) -> Self{
        Self {
            is_count_pass : count_pass,
            n_leaf_node   : 0,
            n_passed      : 0,
            n_end         : 0
        }
    }

    fn clear(&mut self) {
        self.n_end = 0;
        self.n_leaf_node = 0;
        self.n_passed = 0;
    }

    fn search(&mut self, board: &Board, depth: u64) {
        let legal_moves = board.moves();
        if depth == 1 {
            if legal_moves == 0 {
                let passed_legal_moves = board.opponent_moves();
                if passed_legal_moves == 0 {
                    // end
                    self.n_leaf_node += 1;
                    self.n_end += 1;
                } else {
                    // pass
                    self.n_passed += 1;
                    if self.is_count_pass {
                        self.n_leaf_node += 1;
                    } else {
                        self.n_leaf_node += passed_legal_moves.count_ones() as u64;
                    }
                }
            } else {
                self.n_leaf_node += legal_moves.count_ones() as u64;
            }
            return;
        }

        if legal_moves == 0 {
            if board.opponent_moves() == 0 {
                // end
                self.n_leaf_node += 1;
                self.n_end += 1;
            } else {
                // pass
                self.n_passed += 1;
                let passed_board = {
                    let mut b = board.clone(); b.swap(); b
                };
                self.search(&passed_board,  depth - if self.is_count_pass {1} else {0});
            }
            return;
        }
            
        let move_iterator = MoveIterator::new(legal_moves);
        for legal_move in move_iterator {
            let mut put_board = board.clone();
            put_board.put_piece_fast(legal_move);
            self.search(&put_board, depth - 1);
        }


        // let mut legal_moves = legal_moves;
        // while legal_moves != 0 {
        //     let put_place = (!legal_moves + 1) & legal_moves;
        //     legal_moves &= legal_moves - 1;
        //     let mut put_board = board.clone();
        //     put_board.put_piece_fast(put_place);
        //     self.search(&put_board, depth + 1);
        // }


        // let (move_list, n_moves) = get_put_boards_fast(board, legal_moves);
        // for move_cand in move_list.iter().take(n_moves) {
        //     let mut put_board = board.clone();
        //     put_board.player |= move_cand.legal_move;
        //     put_board.player ^= move_cand.flip;
        //     put_board.opponent ^= move_cand.flip;
        //     put_board.swap();
        //     self.search(&put_board, depth + 1);
        // }

        // let move_list = get_put_boards_fast2(board, legal_moves);
        // for move_cand in move_list.iter() {
        //     let mut put_board = board.clone();
        //     put_board.player |= move_cand.legal_move;
        //     put_board.player ^= move_cand.flip;
        //     put_board.opponent ^= move_cand.flip;
        //     put_board.swap();
        //     self.search(&put_board, depth + 1);
        // }
    }

    // board をコピーしない実装
    // fn search2(&mut self, board: &mut Board, depth: u64) {
    //     self.n_node += 1;

    //     let legal_moves = board.put_able();
    //     if legal_moves == 0 {
    //         if board.opponent_put_able() == 0 {
    //             // End
    //             self.n_leaf_node += 1;
    //             self.n_end += 1;
    //             return;
    //         }

    //         if depth >= self.max_depth {
    //             self.n_leaf_node += 1;
    //             return;
    //         }

    //         board.swap();

    //         let depth=  depth + if self.is_count_pass {1} else {0};
    //         self.n_passed += 1;
    //         return self.search2(board, depth);
    //     }

    //     if depth >= self.max_depth {
    //         self.n_leaf_node += 1;
    //         return;
    //     }

    //     let move_iterator = MoveIterator::new(legal_moves);
    //     for position in move_iterator {
    //         let flip_bit = board.flip_bit(position);
    //         let tmp = board.player ^ (flip_bit | position);
    //         board.player = board.opponent ^ flip_bit;
    //         board.opponent = tmp;
    //         self.search2(board, depth + 1);
    //         let tmp = board.opponent ^ (flip_bit | position);
    //         board.opponent = board.player ^ flip_bit;
    //         board.player = tmp;
    //     }
    // }
    
    fn run(&mut self, depth: u64) {
        let board = Board::new();
        self.search(&board, depth);
    }

}

fn format_duration(duration: time::Duration) -> String {
    let millis = duration.as_millis() % 1000; // ミリ秒
    let seconds = duration.as_secs() % 60; // 秒
    let minutes = duration.as_secs() / 60 % 60; // 分
    let hours = duration.as_secs() / 3600; // 時

    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}
pub fn run_perft(depth: u64, count_pass: bool) {
    let mut perft = Perft::new(count_pass);

    let now = time::Instant::now();
    println!(" depth | leaf                 | end        | passed     | time       ");
    for i in 1..=depth {
        perft.clear();
        perft.run(i);
        println!(" {: >5} | {: >20} | {: >10} | {: >10} | {:>14}",
                    i, perft.n_leaf_node, perft.n_end, perft.n_passed, format_duration(now.elapsed()));
    }
}