use crate::bit::*;

pub const A1: u8 = 0;
pub const B1: u8 = 1;
pub const C1: u8 = 2;
pub const D1: u8 = 3;
pub const E1: u8 = 4;
pub const F1: u8 = 5;
pub const G1: u8 = 6;
pub const H1: u8 = 7;
pub const A2: u8 = 8;
pub const B2: u8 = 9;
pub const C2: u8 = 10;
pub const D2: u8 = 11;
pub const E2: u8 = 12;
pub const F2: u8 = 13;
pub const G2: u8 = 14;
pub const H2: u8 = 15;
pub const A3: u8 = 16;
pub const B3: u8 = 17;
pub const C3: u8 = 18;
pub const D3: u8 = 19;
pub const E3: u8 = 20;
pub const F3: u8 = 21;
pub const G3: u8 = 22;
pub const H3: u8 = 23;
pub const A4: u8 = 24;
pub const B4: u8 = 25;
pub const C4: u8 = 26;
pub const D4: u8 = 27;
pub const E4: u8 = 28;
pub const F4: u8 = 29;
pub const G4: u8 = 30;
pub const H4: u8 = 31;
pub const A5: u8 = 32;
pub const B5: u8 = 33;
pub const C5: u8 = 34;
pub const D5: u8 = 35;
pub const E5: u8 = 36;
pub const F5: u8 = 37;
pub const G5: u8 = 38;
pub const H5: u8 = 39;
pub const A6: u8 = 40;
pub const B6: u8 = 41;
pub const C6: u8 = 42;
pub const D6: u8 = 43;
pub const E6: u8 = 44;
pub const F6: u8 = 45;
pub const G6: u8 = 46;
pub const H6: u8 = 47;
pub const A7: u8 = 48;
pub const B7: u8 = 49;
pub const C7: u8 = 50;
pub const D7: u8 = 51;
pub const E7: u8 = 52;
pub const F7: u8 = 53;
pub const G7: u8 = 54;
pub const H7: u8 = 55;
pub const A8: u8 = 56;
pub const B8: u8 = 57;
pub const C8: u8 = 58;
pub const D8: u8 = 59;
pub const E8: u8 = 60;
pub const F8: u8 = 61;
pub const G8: u8 = 62;
pub const H8: u8 = 63;
pub const NO_COORD: u8 = u8::MAX;
pub const TERMINATED: u8 = u8::MAX;
pub const PASS: u8 = 64;



#[derive(Clone,PartialEq,Eq,PartialOrd,Ord)]
pub struct Board {
    pub player: u64,
    pub opponent: u64
}

pub enum PutPieceErr {
    NoValidPlacement,
    Unknown(String)
}

impl Default for Board {
    fn default() -> Self {
        Board {
            player: 0x0000000810000000u64,
            opponent: 0x0000001008000000u64
        }
    }
}

impl Board {

    pub const SIZE: i32 = 8;
    pub const BLACK: usize = 0;
    pub const WHITE: usize = 1;

    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn swap(&mut self) {
        (self.player, self.opponent) = (self.opponent, self.player);
    }

    pub fn clear(&mut self) {
        self.player = 0x0000000810000000u64;
        self.opponent = 0x0000001008000000u64;
    }

    pub fn put_piece_from_coord(&mut self, y: i32, x: i32) -> Result<(), PutPieceErr>
    {
        let mask = 1 << (y * Board::SIZE + x);
        self.put_piece(mask)
    }

    pub fn put_piece(&mut self, put_mask: u64) -> Result<(), PutPieceErr>
    {
        if self.put_able() & put_mask == 0 {
            return Err(PutPieceErr::NoValidPlacement);
        }
        self.put_piece_fast(put_mask);
        Ok(())
    }

    #[inline(always)]
    pub fn flip_bit(&self, x: u64) -> u64 {
        let p: u64 = self.player;
        let o: u64 = self.opponent;
        let mut flip = 0u64;

        // 左方向 (x << 1)
        {
            let m_o = o & 0x7e7e7e7e7e7e7e7e;
            let mut f = (x << 1) & m_o;
            f |= (f << 1) & m_o;
            let pre = m_o & (m_o << 1);
            f |= (f << 2) & pre;
            f |= (f << 2) & pre;
            let outflank = p & (f << 1);
            f &= -((outflank != 0) as i64) as u64;
            flip |= f;
        }

        // 右方向 (x >> 1)
        {
            let m_o = o & 0x7e7e7e7e7e7e7e7e;
            let mut f = (x >> 1) & m_o;
            f |= (f >> 1) & m_o;
            let pre = m_o & (m_o >> 1);
            f |= (f >> 2) & pre;
            f |= (f >> 2) & pre;
            let outflank = p & (f >> 1);
            f &= -((outflank != 0) as i64) as u64;
            flip |= f;
        }

        // 上方向 (x << 8)
        {
            let m_o = o & 0xffffffffffffff00;
            let mut f = (x << 8) & m_o;
            f |= (f << 8) & m_o;
            let pre = m_o & (m_o << 8);
            f |= (f << 16) & pre;
            f |= (f << 16) & pre;
            let outflank = p & (f << 8);
            f &= -((outflank != 0) as i64) as u64;
            flip |= f;
        }

        // 下方向 (x >> 8)
        {
            let m_o = o & 0xffffffffffffff00;
            let mut f = (x >> 8) & m_o;
            f |= (f >> 8) & m_o;
            let pre = m_o & (m_o >> 8);
            f |= (f >> 16) & pre;
            f |= (f >> 16) & pre;
            let outflank = p & (f >> 8);
            f &= -((outflank != 0) as i64) as u64;
            flip |= f;
        }

        // 斜め左上・右下方向 (x << 7, x >> 7)
        {
            let m_o = o & 0x007e7e7e7e7e7e00;

            // 左上方向 (x << 7)
            {
                let mut f = (x << 7) & m_o;
                f |= (f << 7) & m_o;
                let pre = m_o & (m_o << 7);
                f |= (f << 14) & pre;
                f |= (f << 14) & pre;
                let outflank = p & (f << 7);
                f &= -((outflank != 0) as i64) as u64;
                flip |= f;
            }

            // 右下方向 (x >> 7)
            {
                let mut f = (x >> 7) & m_o;
                f |= (f >> 7) & m_o;
                let pre = m_o & (m_o >> 7);
                f |= (f >> 14) & pre;
                f |= (f >> 14) & pre;
                let outflank = p & (f >> 7);
                f &= -((outflank != 0) as i64) as u64;
                flip |= f;
            }
        }

        // 斜め左下・右上方向 (x << 9, x >> 9)
        {
            let m_o = o & 0x007e7e7e7e7e7e00;

            // 左下方向 (x << 9)
            {
                let mut f = (x << 9) & m_o;
                f |= (f << 9) & m_o;
                let pre = m_o & (m_o << 9);
                f |= (f << 18) & pre;
                f |= (f << 18) & pre;
                let outflank = p & (f << 9);
                f &= -((outflank != 0) as i64) as u64;
                flip |= f;
            }

            // 右上方向 (x >> 9)
            {
                let mut f = (x >> 9) & m_o;
                f |= (f >> 9) & m_o;
                let pre = m_o & (m_o >> 9);
                f |= (f >> 18) & pre;
                f |= (f >> 18) & pre;
                let outflank = p & (f >> 9);
                f &= -((outflank != 0) as i64) as u64;
                flip |= f;
            }
        }

        flip
    }


    #[inline(always)]
    pub fn put_piece_fast(&mut self, put_mask: u64)
    {
        let flip_bit = self.flip_bit(put_mask);
        
        self.player ^= (flip_bit | put_mask); // BLACK
        self.opponent ^= flip_bit; // WHITE

        self.swap();
    }

    #[inline(always)]
    pub fn opponent_put_able(&self) -> u64 {
        unsafe {
            let pb = self as *const Board as *mut Board;

            // (*pb).swap();だとうまく動作しません(原因不明)
            std::ptr::swap(&mut (*pb).player, &mut (*pb).opponent);
            let legal_moves = (*pb).put_able();
            std::ptr::swap(&mut (*pb).player, &mut (*pb).opponent);

            legal_moves
        }
    }


    #[inline(always)]
    pub fn put_able(&self) -> u64 {
        let P = self.player;
        let O = self.opponent;

        let mut moves: u64;
        let mut mO: u64;
        let mut flip1: u64;
        let mut flip7: u64;
        let mut flip9: u64;
        let mut flip8: u64;
        let mut pre1: u64;
        let mut pre7: u64;
        let mut pre9: u64;
        let mut pre8: u64;

        // 水平方向マスク処理用(7,9,1方向)のo
        mO = O & 0x7e7e7e7e7e7e7e7e_u64;
        
        // 正方向（左上7、左下9、下8、右1）
        flip7  = mO & (P << 7);
        flip9  = mO & (P << 9);
        flip8  = O & (P << 8);
        flip1  = mO & (P << 1);

        flip7 |= mO & (flip7 << 7);
        flip9 |= mO & (flip9 << 9);
        flip8 |= O  & (flip8 << 8);
        moves  = mO + flip1; 

        pre7 = mO & (mO << 7);
        pre9 = mO & (mO << 9);
        pre8 = O & (O << 8);

        flip7 |= pre7 & (flip7 << 14);
        flip9 |= pre9 & (flip9 << 18);
        flip8 |= pre8 & (flip8 << 16);

        flip7 |= pre7 & (flip7 << 14);
        flip9 |= pre9 & (flip9 << 18);
        flip8 |= pre8 & (flip8 << 16);

        moves |= flip7 << 7;
        moves |= flip9 << 9;
        moves |= flip8 << 8;

        // 逆方向（右下7、右上9、上8、左1）
        flip7 = mO & (P >> 7);
        flip9 = mO & (P >> 9);
        flip8 = O & (P >> 8);
        flip1 = mO & (P >> 1);

        flip7 |= mO & (flip7 >> 7);
        flip9 |= mO & (flip9 >> 9);
        flip8 |= O  & (flip8 >> 8);
        flip1 |= mO & (flip1 >> 1);

        pre7 >>= 7;
        pre9 >>= 9;
        pre8 >>= 8;
        pre1 = mO & (mO >> 1);

        flip7 |= pre7 & (flip7 >> 14);
        flip9 |= pre9 & (flip9 >> 18);
        flip8 |= pre8 & (flip8 >> 16);
        flip1 |= pre1 & (flip1 >> 2);

        flip7 |= pre7 & (flip7 >> 14);
        flip9 |= pre9 & (flip9 >> 18);
        flip8 |= pre8 & (flip8 >> 16);
        flip1 |= pre1 & (flip1 >> 2);

        moves |= flip7 >> 7;
        moves |= flip9 >> 9;
        moves |= flip8 >> 8;
        moves |= flip1 >> 1;

        // 空きマスでマスク
        moves & !(P | O)
    }


    pub fn get_all_symmetries(&self) -> Vec<Board>
    {
        let mut symmetries = Vec::new();

        for i in 0b0000..0b1000 { // 2^3 = 8 different combinations
            let mut sym_board = self.clone();
            if (i & 0b0001) != 0 {
                sym_board.player = horizontal_mirror(sym_board.player);
                sym_board.opponent = horizontal_mirror(sym_board.opponent);
            }
            if (i & 0b0010) != 0 {
                sym_board.player = vertical_mirror(sym_board.player);
                sym_board.opponent = vertical_mirror(sym_board.opponent);
            }
            if (i & 0b0100) != 0 {
                sym_board.player = transpose(sym_board.player);
                sym_board.opponent = transpose(sym_board.opponent);
            }
            symmetries.push(sym_board);
        }
        symmetries
    }
    pub fn get_all_rotations(&self) -> Vec<Board>
    {
        let mut rotations = Vec::new();

        let no_rotation = self.clone();
        rotations.push(no_rotation);

        let mut rotate_90_degrees = self.clone();
        rotate_90_degrees.player = vertical_mirror(rotate_90_degrees.player);
        rotate_90_degrees.opponent = vertical_mirror(rotate_90_degrees.opponent);
        rotate_90_degrees.player = transpose(rotate_90_degrees.player);
        rotate_90_degrees.opponent = transpose(rotate_90_degrees.opponent);
        rotations.push(rotate_90_degrees);

        let mut rotate_180_degrees = self.clone();
        rotate_180_degrees.player = vertical_mirror(rotate_180_degrees.player);
        rotate_180_degrees.opponent = vertical_mirror(rotate_180_degrees.opponent);
        rotate_180_degrees.player = horizontal_mirror(rotate_180_degrees.player);
        rotate_180_degrees.opponent = horizontal_mirror(rotate_180_degrees.opponent);
        rotations.push(rotate_180_degrees);

        let mut rotate_270_degrees = self.clone();
        rotate_270_degrees.player = horizontal_mirror(rotate_270_degrees.player);
        rotate_270_degrees.opponent = horizontal_mirror(rotate_270_degrees.opponent);
        rotate_270_degrees.player = transpose(rotate_270_degrees.player);
        rotate_270_degrees.opponent = transpose(rotate_270_degrees.opponent);
        rotations.push(rotate_270_degrees);

        rotations
    }

    #[inline(always)]
    pub fn move_count(&self) -> i32
    { // 現在何手目まで打たれたか(0~60)
        (self.player | self.opponent).count_ones() as i32 - 4
    }

    pub fn print_board(&self) {
        for y in 0..8 {
            for x in 0..8 {
                let mask = 1u64 << (y * 8 + x);
                if self.player & mask != 0 {
                    print!("X");
                } else if self.opponent & mask != 0 {
                    print!("O");
                } else {
                    print!(".");
                }
            }
            println!();
        }
    }

    pub fn get_unique_board(&self) -> Board{
        self.get_all_symmetries()
                    .into_iter()
                    .min()
                    .unwrap()
    }

    pub fn print_board_string(&self, player: usize)  -> String {
        let mut s = String::new();
         s += "next turn: ";
        
        if player == Board::BLACK {
            s += "X";
        } else {
            s += "O";
        }
        s += "\n";
        for y in 0..8 {
            for x in 0..8 {
                let mask = 1u64 << (y * 8 + x);
                if self.player & mask != 0 {
                    if player == Board::BLACK {
                        s += "X";
                    } else {
                        s += "O";
                    }
                } else if self.opponent & mask != 0 {
                    if player == Board::BLACK {
                        s += "O";
                    } else {
                        s += "X";
                    }
                } else {
                    s += ".";
                }
            }
            s += "\n";
        }

        s
    }

    #[inline(always)]
    pub fn piece_count(&self) -> i32
    {
        (self.player | self.opponent).count_ones() as i32
    }

    #[inline(always)]
    pub fn empties_count(&self) -> i32
    {
        (self.player | self.opponent).count_zeros() as i32
    }

}


#[inline(always)]
pub fn pos_b_2_n_fast(bit: u64) -> i32 {
    bit.trailing_zeros() as i32
}

pub fn position_bit_to_num(bit: u64) -> Result<u8, &'static str> {
    if bit.count_ones() != 1 {
        return Err("Invalid position bit")
    }

    Ok(bit.trailing_zeros() as u8)
}

#[inline(always)]
pub fn position_num_to_bit(num: i32) -> Result<u64, &'static str> {
    if !(0..64).contains(&num) {
        return Err("Invalid position string");
    }

    Ok(1u64 << num)
}

pub fn position_str_to_bit(s: &str) -> Result<u64, &'static str> {
    if s.len() != 2 {
        return Err("Invalid position string");
    }

    let mut chars = s.chars();

    let col = match chars.next() {
        Some(c) => {
            match c.to_ascii_uppercase() {
                'A' => 0,
                'B' => 1,
                'C' => 2,
                'D' => 3,
                'E' => 4,
                'F' => 5,
                'G' => 6,
                'H' => 7,
                _ => return Err("Invalid column letter")
            }
        }
        _ => return Err("Invalid column letter"),
    };

    let row = match chars.next() {
        Some('1') => 0,
        Some('2') => 1,
        Some('3') => 2,
        Some('4') => 3,
        Some('5') => 4,
        Some('6') => 5,
        Some('7') => 6,
        Some('8') => 7,
        _ => return Err("Invalid row number"),
    };

    let position = row * 8 + col;
    Ok(1u64 << position)
}

pub fn position_bit_to_str(bit: u64) -> Result<String, &'static str> {
    if bit.count_ones() != 1 {
        return Err("Invalid bit position");
    }

    let pos = position_bit_to_num(bit)?;
    
    let col = (pos % 8) as u8;
    let row = (pos / 8) as u8;

    let col_char = match col {
        0 => 'A',
        1 => 'B',
        2 => 'C',
        3 => 'D',
        4 => 'E',
        5 => 'F',
        6 => 'G',
        7 => 'H',
        _ => return Err("Invalid column"),
    };

    let row_char = match row {
        0 => '1',
        1 => '2',
        2 => '3',
        3 => '4',
        4 => '5',
        5 => '6',
        6 => '7',
        7 => '8',
        _ => return Err("Invalid row"),
    };

    Ok(format!("{}{}", col_char, row_char))
}


//cargo test --lib --release -- board::perft --nocapture
#[cfg(test)]
mod perft {
    use crate::search::{get_put_boards, get_put_boards_fast, get_put_boards_fast2};
    use super::*;
    use std::time;

    struct Perft {
        max_depth     : u64,
        is_count_pass : bool,
        n_node        : u64,
        n_leaf_node   : u64,
        n_passed      : u64,
        n_end         : u64, // leaf node を含む
    }

    impl Perft {        
        fn new(depth: u64) -> Self{
            Self {
                max_depth     : depth,
                is_count_pass : false,
                n_node        : 0,
                n_leaf_node   : 0,
                n_passed      : 0,
                n_end         : 0
            }
        }

        fn clear(&mut self) {
            *self = Self::new(self.max_depth);
        }


        
        fn search(&mut self, board: &Board, depth: u64) {
            self.n_node += 1;

            let legal_moves = board.put_able();
            if legal_moves == 0 {
                if board.opponent_put_able() == 0 {
                    // End
                    self.n_leaf_node += 1;
                    self.n_end += 1;
                    return;
                }

                if depth >= self.max_depth {
                    self.n_leaf_node += 1;
                    return;
                }

                let passed_board = {
                    let mut b = board.clone();
                    b.swap();
                    b
                };

                let depth=  depth + if self.is_count_pass {1} else {0};
                self.n_passed += 1;
                return self.search(&passed_board, depth);
            }

            if depth >= self.max_depth {
                self.n_leaf_node += 1;
                return;
            }

            let mut legal_moves = legal_moves;
            while legal_moves != 0 {
                let put_place = (!legal_moves + 1) & legal_moves;
                legal_moves &= legal_moves - 1;
                let mut put_board = board.clone();
                put_board.put_piece_fast(put_place);
                self.search(&put_board, depth + 1);
            }

            // let boards = get_put_boards(board, legal_moves);
            // for b in boards.iter() {
            //     self.search(&b.board, depth + 1);
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
        fn search2(&mut self, board: &mut Board, depth: u64) {
            self.n_node += 1;

            let legal_moves = board.put_able();
            if legal_moves == 0 {
                if board.opponent_put_able() == 0 {
                    // End
                    self.n_leaf_node += 1;
                    self.n_end += 1;
                    return;
                }

                if depth >= self.max_depth {
                    self.n_leaf_node += 1;
                    return;
                }

                let passed_board = {
                    let mut b = board.clone();
                    b.swap();
                    b
                };

                let depth=  depth + if self.is_count_pass {1} else {0};
                self.n_passed += 1;
                return self.search(&passed_board, depth);
            }

            if depth >= self.max_depth {
                self.n_leaf_node += 1;
                return;
            }

            let mut legal_moves = legal_moves;
            while legal_moves != 0 {
                let position = (!legal_moves + 1) & legal_moves;
                legal_moves &= legal_moves - 1;
                let flip_bit = board.flip_bit(position);
                let tmp = board.player ^ (flip_bit | position);
                board.player = board.opponent ^ flip_bit;
                board.opponent = tmp;
                self.search(board, depth + 1);
                let tmp = board.opponent ^ (flip_bit | position);
                board.opponent = board.player ^ flip_bit;
                board.player = tmp;
            }

        }
        
        fn run(&mut self) {
            let mut board = Board::new();
            self.search(&board, 0);
            
            // self.search2(&mut board, 0);
        }

    }
    
    #[test]
    fn run_perft() {
        let mut perft = Perft::new(1);
        perft.is_count_pass = false;

        let now = time::Instant::now();
        println!(" depth | node       | leaf       | end        | passed     | time / s   ");
        for i in 1..=15 {
            perft.clear();
            perft.max_depth = i;
            perft.run();
            println!(" {: >5} | {: >10} | {: >10} | {: >10} | {: >10} | {}",
                        i, perft.n_node, perft.n_leaf_node, perft.n_end, perft.n_passed, now.elapsed().as_secs_f64());
        }
    }
}