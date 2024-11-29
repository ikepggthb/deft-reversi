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



#[derive(Clone)]
pub struct Board {
    pub bit_board: [u64; 2],
    pub next_turn: usize
}

pub enum PutPieceErr {
    NoValidPlacement,
    Unknown(String)
}

impl Default for Board {
    fn default() -> Self {
        Board {
            bit_board: [0x0000000810000000u64,0x0000001008000000u64],
            next_turn: Board::BLACK
        }
    }
}

impl Board {

    pub const BOARD_SIZE: i32 = 8;
    pub const BLACK: usize = 0;
    pub const WHITE: usize = 1;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.bit_board = [0x0000000810000000u64,0x0000001008000000u64];
        self.next_turn = Board::BLACK;
    }

    pub fn put_piece_from_coord(&mut self, y: i32, x: i32) -> Result<(), PutPieceErr>
    {
        let mask = 1 << (y * Board::BOARD_SIZE + x);
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
    pub fn flip_bit(&self, x: u64) -> u64
    {
        let p: u64 = self.bit_board[self.next_turn];
        let o: u64 = self.bit_board[self.next_turn ^ 1];
        let mut flip = 0u64;

        let maskd = o & 0x7e7e7e7e7e7e7e7e;
        let mut flip1 =  (x << 1) & maskd;
        flip1 |=  (flip1 << 1) & maskd;
        flip1 |=  (flip1 << 1) & maskd;
        flip1 |=  (flip1 << 1) & maskd;
        flip1 |=  (flip1 << 1) & maskd;
        flip1 |=  (flip1 << 1) & maskd;
        flip1 |=  (flip1 << 1) & maskd;
        let outflank = p & (flip1 << 1);
        if outflank == 0 {flip1 = 0};
        flip |= flip1;
        
        // 逆方向
        let mut flip2 =  (x >> 1) & maskd;
        flip2 |=  (flip2 >> 1) & maskd;
        flip2 |=  (flip2 >> 1) & maskd;
        flip2 |=  (flip2 >> 1) & maskd;
        flip2 |=  (flip2 >> 1) & maskd;
        flip2 |=  (flip2 >> 1) & maskd;
        flip2 |=  (flip2 >> 1) & maskd;
        let outflank = p & (flip2 >> 1);
        if outflank == 0 {flip2 = 0};
        flip |= flip2;

        // 上下
        let maskd = o & 0xffffffffffffff00;
        let mut flip1 =  (x << 8) & maskd;
        flip1 |=  (flip1 << 8) & maskd;
        flip1 |=  (flip1 << 8) & maskd;
        flip1 |=  (flip1 << 8) & maskd;
        flip1 |=  (flip1 << 8) & maskd;
        flip1 |=  (flip1 << 8) & maskd;
        flip1 |=  (flip1 << 8) & maskd;
        let outflank = p & (flip1 << 8);
        if outflank == 0 {flip1 = 0};
        flip |= flip1;
        
        // 逆方向
        let mut flip2 =  (x >> 8) & maskd;
        flip2 |=  (flip2 >> 8) & maskd;
        flip2 |=  (flip2 >> 8) & maskd;
        flip2 |=  (flip2 >> 8) & maskd;
        flip2 |=  (flip2 >> 8) & maskd;
        flip2 |=  (flip2 >> 8) & maskd;
        flip2 |=  (flip2 >> 8) & maskd;
        let outflank = p & (flip2 >> 8);
        if outflank == 0 {flip2 = 0};
        flip |= flip2;

        // 斜め
        let maskd = o & 0x007e7e7e7e7e7e00;
        let mut flip1 =  (x << 7) & maskd;
        flip1 |=  (flip1 << 7) & maskd;
        flip1 |=  (flip1 << 7) & maskd;
        flip1 |=  (flip1 << 7) & maskd;
        flip1 |=  (flip1 << 7) & maskd;
        flip1 |=  (flip1 << 7) & maskd;
        flip1 |=  (flip1 << 7) & maskd;
        let outflank = p & (flip1 << 7);
        if outflank == 0 {flip1 = 0};
        flip |= flip1;
        
        // 逆方向
        let mut flip2 =  (x >> 7) & maskd;
        flip2 |=  (flip2 >> 7) & maskd;
        flip2 |=  (flip2 >> 7) & maskd;
        flip2 |=  (flip2 >> 7) & maskd;
        flip2 |=  (flip2 >> 7) & maskd;
        flip2 |=  (flip2 >> 7) & maskd;
        flip2 |=  (flip2 >> 7) & maskd;
        let outflank = p & (flip2 >> 7);
        if outflank == 0 {flip2 = 0};
        flip |= flip2;

        // 斜め 2
        let mut flip1 =  (x << 9) & maskd;
        flip1 |=  (flip1 << 9) & maskd;
        flip1 |=  (flip1 << 9) & maskd;
        flip1 |=  (flip1 << 9) & maskd;
        flip1 |=  (flip1 << 9) & maskd;
        flip1 |=  (flip1 << 9) & maskd;
        flip1 |=  (flip1 << 9) & maskd;
        let outflank = p & (flip1 << 9);
        if outflank == 0 {flip1 = 0};
        flip |= flip1;
        
        // 逆方向
        let mut flip2 =  (x >> 9) & maskd;
        flip2 |=  (flip2 >> 9) & maskd;
        flip2 |=  (flip2 >> 9) & maskd;
        flip2 |=  (flip2 >> 9) & maskd;
        flip2 |=  (flip2 >> 9) & maskd;
        flip2 |=  (flip2 >> 9) & maskd;
        flip2 |=  (flip2 >> 9) & maskd;
        let outflank = p & (flip2 >> 9);
        if outflank == 0 {flip2 = 0};
        flip |= flip2;

        flip
    }

    #[inline(always)]
    pub fn put_piece_fast(&mut self, put_mask: u64)
    {
        // ひっくり返す箇所を計算
        let reverse_bit = self.flip_bit(put_mask);
        
        // 石を置く
        self.bit_board[self.next_turn] |= put_mask;

        // ひっくり返す
        self.bit_board[0] ^= reverse_bit; // BLACK
        self.bit_board[1] ^= reverse_bit; // WHITE

        // 次のターンにする
        self.next_turn ^= 1;
    }

    #[inline(always)]
    pub fn opponent_put_able(&self) -> u64 {
        let blank = !(self.bit_board[Board::BLACK] | self.bit_board[Board::WHITE]);

        let p: u64 = self.bit_board[self.next_turn ^ 1];
        let o: u64 = self.bit_board[self.next_turn];

        let mut legal_moves = 0u64;

        // 左右
        let maskd = 0x7e7e7e7e7e7e7e7e & o;
        let mut flip =  (p << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        legal_moves |=  (flip << 1) & blank;
        
        // 逆方向
        let mut flip =  (p >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        legal_moves |=  (flip >> 1) & blank;


        // 上下
        let maskd = 0xffffffffffffff00 & o;
        let mut flip =  (p << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        legal_moves |=  (flip << 8) & blank;
        
        // 逆方向
        let mut flip =  (p >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        legal_moves |=  (flip >> 8) & blank;


        // 斜め
        let maskd = 0x007e7e7e7e7e7e00 & o;
        let mut flip =  (p << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        legal_moves |=  (flip << 7) & blank;
        
        // 逆方向
        let mut flip =  (p >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        legal_moves |=  (flip >> 7) & blank;


        // 斜め 2
        let mut flip =  (p << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        legal_moves |=  (flip << 9) & blank;
        
        // 逆方向
        let mut flip =  (p >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        legal_moves |=  (flip >> 9) & blank;

        legal_moves

    }

    #[inline(always)]
    pub fn put_able(&self) -> u64
    {
        let blank = !(self.bit_board[Board::BLACK] | self.bit_board[Board::WHITE]);

        let p: u64 = self.bit_board[self.next_turn];
        let o: u64 = self.bit_board[self.next_turn ^ 1];

        let mut legal_moves = 0u64;

        // 左右
        let maskd = 0x7e7e7e7e7e7e7e7e & o;
        let mut flip =  (p << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        flip |=  (flip << 1) & maskd;
        legal_moves |=  (flip << 1) & blank;
        
        // 逆方向
        let mut flip =  (p >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        flip |=  (flip >> 1) & maskd;
        legal_moves |=  (flip >> 1) & blank;


        // 上下
        let maskd = 0xffffffffffffff00 & o;
        let mut flip =  (p << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        flip |=  (flip << 8) & maskd;
        legal_moves |=  (flip << 8) & blank;
        
        // 逆方向
        let mut flip =  (p >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        flip |=  (flip >> 8) & maskd;
        legal_moves |=  (flip >> 8) & blank;


        // 斜め
        let maskd = 0x007e7e7e7e7e7e00 & o;
        let mut flip =  (p << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        flip |=  (flip << 7) & maskd;
        legal_moves |=  (flip << 7) & blank;
        
        // 逆方向
        let mut flip =  (p >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        flip |=  (flip >> 7) & maskd;
        legal_moves |=  (flip >> 7) & blank;


        // 斜め 2
        let mut flip =  (p << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        flip |=  (flip << 9) & maskd;
        legal_moves |=  (flip << 9) & blank;
        
        // 逆方向
        let mut flip =  (p >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        flip |=  (flip >> 9) & maskd;
        legal_moves |=  (flip >> 9) & blank;

        legal_moves

    }


    pub fn get_all_symmetries(&self) -> Vec<Board>
    {
        let mut symmetries = Vec::new();

        for i in 0b0000..0b1000 { // 2^3 = 8 different combinations
            let mut sym_board = self.clone();
            if (i & 0b0001) != 0 {
                sym_board.bit_board[Board::BLACK] = horizontal_mirror(sym_board.bit_board[Board::BLACK]);
                sym_board.bit_board[Board::WHITE] = horizontal_mirror(sym_board.bit_board[Board::WHITE]);
            }
            if (i & 0b0010) != 0 {
                sym_board.bit_board[Board::BLACK] = vertical_mirror(sym_board.bit_board[Board::BLACK]);
                sym_board.bit_board[Board::WHITE] = vertical_mirror(sym_board.bit_board[Board::WHITE]);
            }
            if (i & 0b0100) != 0 {
                sym_board.bit_board[Board::BLACK] = transpose(sym_board.bit_board[Board::BLACK]);
                sym_board.bit_board[Board::WHITE] = transpose(sym_board.bit_board[Board::WHITE]);
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
        rotate_90_degrees.bit_board[Board::BLACK] = vertical_mirror(rotate_90_degrees.bit_board[Board::BLACK]);
        rotate_90_degrees.bit_board[Board::WHITE] = vertical_mirror(rotate_90_degrees.bit_board[Board::WHITE]);
        rotate_90_degrees.bit_board[Board::BLACK] = transpose(rotate_90_degrees.bit_board[Board::BLACK]);
        rotate_90_degrees.bit_board[Board::WHITE] = transpose(rotate_90_degrees.bit_board[Board::WHITE]);
        rotations.push(rotate_90_degrees);

        let mut rotate_180_degrees = self.clone();
        rotate_180_degrees.bit_board[Board::BLACK] = vertical_mirror(rotate_180_degrees.bit_board[Board::BLACK]);
        rotate_180_degrees.bit_board[Board::WHITE] = vertical_mirror(rotate_180_degrees.bit_board[Board::WHITE]);
        rotate_180_degrees.bit_board[Board::BLACK] = horizontal_mirror(rotate_180_degrees.bit_board[Board::BLACK]);
        rotate_180_degrees.bit_board[Board::WHITE] = horizontal_mirror(rotate_180_degrees.bit_board[Board::WHITE]);
        rotations.push(rotate_180_degrees);

        let mut rotate_270_degrees = self.clone();
        rotate_270_degrees.bit_board[Board::BLACK] = horizontal_mirror(rotate_270_degrees.bit_board[Board::BLACK]);
        rotate_270_degrees.bit_board[Board::WHITE] = horizontal_mirror(rotate_270_degrees.bit_board[Board::WHITE]);
        rotate_270_degrees.bit_board[Board::BLACK] = transpose(rotate_270_degrees.bit_board[Board::BLACK]);
        rotate_270_degrees.bit_board[Board::WHITE] = transpose(rotate_270_degrees.bit_board[Board::WHITE]);
        rotations.push(rotate_270_degrees);

        rotations
    }

    #[inline(always)]
    pub fn move_count(&self) -> i32
    { // 現在何手目まで打たれたか(0~60)
        (self.bit_board[Board::BLACK] | self.bit_board[Board::WHITE]).count_ones() as i32 - 4
    }

    pub fn print_board(&self) {
        for y in 0..8 {
            for x in 0..8 {
                let mask = 1u64 << (y * 8 + x);
                if self.bit_board[Board::BLACK] & mask != 0 {
                    print!("X");
                } else if self.bit_board[Board::WHITE] & mask != 0 {
                    print!("O");
                } else {
                    print!(".");
                }
            }
            println!();
        }
    }

    pub fn print_board_string(&self)  -> String {
        let mut s = String::new();
         s += "next turn: ";
        if self.next_turn == Board::BLACK {
            s += "X";
        } else {
            s += "O";
        }
        s += "\n";
        for y in 0..8 {
            for x in 0..8 {
                let mask = 1u64 << (y * 8 + x);
                if self.bit_board[Board::BLACK] & mask != 0 {
                    s += "X";
                } else if self.bit_board[Board::WHITE] & mask != 0 {
                    s += "O";
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
        (self.bit_board[0] | self.bit_board[1]).count_ones() as i32
    }

    #[inline(always)]
    pub fn empties_count(&self) -> i32
    {
        (self.bit_board[0] | self.bit_board[1]).count_zeros() as i32
    }

}

pub fn position_bit_to_num(bit: u64) -> Result<u8, &'static str> {
    for i in 0..64 {
        let mask = 1u64 << i;
        if bit == mask {
            return Ok(i);
        }
    }

    Err("Invalid position bit")
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
