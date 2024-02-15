use crate::board::*;
use rand::Rng;

pub fn simplest_eval (board: &Board) -> i32 
{
    const SCORES: [i32; 64] = [
        120, -40,  1,  0,  0,  1, -40, 120,
        -40, -60, -5, -4, -4, -5, -60, -40,
         1,  -5,  -1, -2, -2, -1,  -5,   1,
         0,  -4,  -2, -1, -1, -2,  -4,   0,
         0,  -4,  -2, -1, -1, -2,  -4,   0,
         1,  -5,  -1, -2, -2, -1,  -5,   1,
        -40, -60, -5, -4, -4, -5, -60, -40,
        120, -40,  1,  0,  0,  1, -40, 120,
    ];

    let m1 = [0x7E00000000000000u64, 0x1010101010100, 0x80808080808000, 0x7e];
    let m2 = [0x8100000000000000u64, 0x100000000000001, 0x8000000000000080, 0x81];


    let mut place_score = 0;

    let player_board = board.bit_board[board.next_turn];
    let opponent_board = board.bit_board[board.next_turn ^1];

    for i in 0..4 {
        if ((player_board & m1[i]) | (opponent_board & m2[i])) == m1[i] {
            place_score += 120;
        }
        if ((opponent_board & m1[i]) | (player_board & m2[i])) == m1[i] {
            place_score -= 120;
        }
        let side = m1[i] | m2[i];
        if side & (player_board | opponent_board) == side {
            place_score += ((player_board & side).count_ones() as i32 - (opponent_board & side).count_ones() as i32)  * 20;
        }

    }
    
    let mut player_board_bit = player_board;
    let mut opponent_board_bit = opponent_board;
    while  player_board_bit != 0 {
        let bit_index = player_board_bit.trailing_zeros() as usize;
        player_board_bit &= player_board_bit - 1; // 1番小さい桁の1を0にする。
        place_score += SCORES[bit_index];
    }
    while  opponent_board_bit != 0 {
        let bit_index = opponent_board_bit.trailing_zeros() as usize;
        opponent_board_bit &= opponent_board_bit - 1; // 1番小さい桁の1を0にする。
        place_score -= SCORES[bit_index];
    }

    let player_piece_count = player_board.count_ones() as i32;
    let opponent_piece_count = opponent_board.count_ones() as i32;
    let piece_count_score = 
        if player_piece_count + opponent_piece_count < 40 {
            opponent_piece_count - player_piece_count
        } else {0};
    
    let player_mobility = board.put_able().count_ones() as i32;
    let mut board = board.clone();
    board.next_turn ^= 1;
    let opponent_mobility = board.put_able().count_ones() as i32;

    let mobility_score = player_mobility - opponent_mobility;

    if player_mobility == 0 && opponent_mobility == 0 {
        if player_piece_count > opponent_piece_count  {
            1000
        } else {
            -1000
        }
    } else {
         (place_score * 10 + mobility_score * 85 + piece_count_score * 40) / 40
    }
}


#[allow(dead_code)]
pub fn put_random_piece(board: &mut Board) -> Result<(), PutPieceErr> {
    let legal_moves = board.put_able();
    if legal_moves == 0 {
        return Err(PutPieceErr::NoValidPlacement);
    }

    let mut bit_indices = [0; 64];
    let mut count = 0;
    let mut moves = legal_moves;
    while moves != 0 {
        let bit_index = moves.trailing_zeros();
        bit_indices[count as usize] = bit_index;
        count += 1;
        moves &= moves - 1; 
    }

    let mut rng = rand::thread_rng();
    let random_index = rng.gen_range(0..count);
    let selected_bit_index = bit_indices[random_index as usize];

    board.put_piece(1 << selected_bit_index)
}

pub fn put_eval_one_simple (board: &mut Board) -> Result<(), PutPieceErr> {
    let legal_moves = board.put_able();
    if legal_moves == 0 {
        return Err(PutPieceErr::NoValidPlacement);
    }

    let mut max_score = i32::MIN;
    let mut max_score_put_place = 0;
    let mut moves = legal_moves;
    while  moves != 0 {
        let mut virt_board = board.clone();
        let put_place = (!moves + 1) & moves; //最も小さい位のbitをマスクする
        moves &= moves - 1; // 最も小さい位のbitを消す
        virt_board.put_piece(put_place)?;   
        let current_score: i32 = -simplest_eval(&virt_board);
        if current_score > max_score {
            max_score = current_score;
            max_score_put_place = put_place;
        }
    }

    board.put_piece(max_score_put_place)
}

