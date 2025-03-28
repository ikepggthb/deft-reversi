use crate::{bit::*, flip, get_moves::get_moves};

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
pub const PASS: u8 = 64;
pub const NO_COORD: u8 = 65;


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

    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn swap(&mut self) {
        (self.player, self.opponent) = (self.opponent, self.player);
    }

    
    #[inline(always)]
    pub fn swapped_board(&self) -> Board {
        let mut b = self.clone();
        b.swap();
        b
    }

    pub fn clear(&mut self) {
        self.player = 0x0000000810000000u64;
        self.opponent = 0x0000001008000000u64;
    }

    pub fn put(&mut self, put_mask: u64) -> Result<(), PutPieceErr>
    {
        if self.moves() & put_mask == 0 {
            return Err(PutPieceErr::NoValidPlacement);
        }
        self.put_piece_fast(put_mask);
        Ok(())
    }

    #[inline(always)]
    pub fn flip_bit(&self, x: u64) -> u64 {
        let p: u64 = self.player;
        let o: u64 = self.opponent;
        
        #[cfg(target_feature = "avx2")]
        unsafe {
            flip::flip_avx2(x.trailing_zeros() as usize, p, o)
        }
        
        #[cfg(not(target_feature = "avx2"))]
        flip::flip_std(x, p, o)
    }

    #[inline(always)]
    pub fn put_piece_fast(&mut self, put_mask: u64)
    {
        let flip_bit = self.flip_bit(put_mask);
        
        self.player ^= flip_bit | put_mask;
        self.opponent ^= flip_bit; 
        self.swap();
    }

    #[inline(always)]
    pub fn put_piece_fast_from_flip_bit(&mut self, put_mask: u64, flip_bit: u64)
    {
        self.player ^= flip_bit | put_mask;
        self.opponent ^= flip_bit;

        self.swap();
    }

    #[inline(always)]
    pub fn opponent_moves(&self) -> u64 {
        let p = self.player;
        let o = self.opponent;
        get_moves(o, p)
    }

    #[inline(always)]
    pub fn moves(&self) -> u64 {
        let p = self.player;
        let o = self.opponent;
        get_moves(p, o)
    }

    pub fn all_symmetries(&self) -> Vec<Board>
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
    pub fn all_rotations(&self) -> Vec<Board>
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

    /// Returns the current move count (number of moves played so far).
    /// The count starts at 0 and ends at 60 (excluding passes).
    #[inline(always)]
    pub fn move_count(&self) -> i32
    {
        (self.player | self.opponent).count_ones() as i32 - 4
    }

    pub fn get_unique_board(&self) -> Board{
        self.all_symmetries()
                    .into_iter()
                    .min()
                    .unwrap()
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
