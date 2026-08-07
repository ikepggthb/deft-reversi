//! Egaroucid の book ファイル (`.egbk3` / `.egbk2` / `.egbk`) の読み書き。
//!
//! # `.egbk3` (現行形式)
//!
//! すべて little endian。ヘッダは 14 バイト。
//!
//! | offset | size | 内容 |
//! |--------|------|------|
//! | 0  | 9 | magic `"DICUORAGE"` (= `EGAROUCID` の逆順) |
//! | 9  | 1 | book version (= 3) |
//! | 10 | 4 | `i32 n_boards` |
//!
//! 以降 `n_boards` 個の局面が **25 バイト固定長**で並ぶ。
//!
//! | offset | size | 内容 |
//! |--------|------|------|
//! | 0  | 8 | `u64 player` |
//! | 8  | 8 | `u64 opponent` |
//! | 16 | 1 | `i8 value` |
//! | 17 | 1 | `i8 level` |
//! | 18 | 4 | `u32 n_lines` |
//! | 22 | 1 | `i8 leaf.value` |
//! | 23 | 1 | `i8 leaf.move` |
//! | 24 | 1 | `i8 leaf.level` |
//!
//! Edax と違い **着手リスト (link) を持たない**ため可変長部分が無い。
//! Egaroucid はヘッダの `n_boards` の個数だけ読む (Edax は EOF まで読む)。
//! `value` が石差の範囲外、あるいは `player & opponent != 0` のレコードは
//! 黙って読み飛ばす。同じ盤面が複数あるときは **level が高い方を採る**。
//!
//! # `.egbk2` (旧形式)
//!
//! ヘッダは `.egbk3` と同じ 14 バイト (version = 2)。局面は可変長で
//! `u64 player`, `u64 opponent`, `i8 value`, `i8 level`, `u8 n_moves`,
//! `n_moves` 個の (`i8 value`, `i8 move`)。着手リストは読み飛ばす
//! (Egaroucid 自身も v3 では使わない)。
//!
//! # `.egbk` (最初期形式)
//!
//! マジックが無く `i32 n_boards` から始まる。局面は
//! `u64 player`, `u64 opponent`, `u8 value_raw` の 17 バイト。
//! 評価値は `-((i8)value_raw - 64)` で復元する。

use super::elem::{is_valid_score, BookElem, Leaf};
use super::Book;
use crate::board::board::Board;
use crate::EngineError;

/// `.egbk3` / `.egbk2` の magic。`EGAROUCID` の逆順。
pub const EGBK_MAGIC: &[u8; 9] = b"DICUORAGE";
/// このエンジンが書き出す book version。
pub const EGBK_VERSION: u8 = 3;

/// `.egbk3` のヘッダ長。
pub(super) const EGBK3_HEADER_SIZE: usize = 14;
/// `.egbk3` の 1 局面のバイト数。
pub(super) const EGBK3_RECORD_SIZE: usize = 25;

/// バイト列から固定長を読み進めるカーソル。
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], EngineError> {
        if self.remaining() < n {
            return Err(EngineError::InvalidData(format!(
                "unexpected end of book file at offset {}",
                self.pos
            )));
        }
        let slice = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, EngineError> {
        Ok(self.take(1)?[0])
    }

    fn i8(&mut self) -> Result<i8, EngineError> {
        Ok(self.u8()? as i8)
    }

    fn u32(&mut self) -> Result<u32, EngineError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, EngineError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, EngineError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

/// magic とバージョンを読み、`n_boards` を返す。
fn read_egbk_header(r: &mut Reader<'_>, expected_version: u8) -> Result<i32, EngineError> {
    let magic = r.take(9)?;
    if magic != EGBK_MAGIC {
        return Err(EngineError::InvalidData(
            "not an egaroucid opening book".to_string(),
        ));
    }
    let version = r.u8()?;
    if version != expected_version {
        return Err(EngineError::InvalidData(format!(
            "expected egaroucid book version {expected_version}, found {version}"
        )));
    }
    let n_boards = r.i32()?;
    if n_boards < 0 {
        return Err(EngineError::InvalidData(format!(
            "negative board count: {n_boards}"
        )));
    }
    Ok(n_boards)
}

impl Book {
    /// `.egbk3` (バージョンが違えば `.egbk2`) を読み込む。
    pub fn from_egbk_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        if bytes.len() < 10 {
            return Err(EngineError::InvalidData(
                "book file is too short".to_string(),
            ));
        }
        match bytes[9] {
            3 => Self::from_egbk3_bytes(bytes),
            2 => Self::from_egbk2_bytes(bytes),
            version => Err(EngineError::InvalidData(format!(
                "unsupported egaroucid book version: {version}"
            ))),
        }
    }

    /// `.egbk3` を読み込む。
    pub fn from_egbk3_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        let mut r = Reader::new(bytes);
        let n_boards = read_egbk_header(&mut r, 3)?;

        let mut book = Self::empty();
        for _ in 0..n_boards {
            let player = r.u64()?;
            let opponent = r.u64()?;
            let value = r.i8()?;
            let level = r.i8()?;
            let n_lines = r.u32()?;
            let leaf_value = r.i8()?;
            let leaf_move = r.i8()?;
            let leaf_level = r.i8()?;

            // Egaroucid と同じ検査。壊れたレコードは黙って読み飛ばす。
            if !is_valid_score(value) || player & opponent != 0 {
                continue;
            }
            book.merge_elem(
                &Board { player, opponent },
                BookElem {
                    value,
                    level,
                    leaf: Leaf {
                        value: leaf_value,
                        mv: leaf_move,
                        level: leaf_level,
                    },
                    n_lines,
                },
            );
        }
        Ok(book)
    }

    /// `.egbk2` (旧形式) を読み込む。着手リストは読み飛ばす。
    pub fn from_egbk2_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        let mut r = Reader::new(bytes);
        let n_boards = read_egbk_header(&mut r, 2)?;

        let mut book = Self::empty();
        for _ in 0..n_boards {
            let player = r.u64()?;
            let opponent = r.u64()?;
            let value = r.i8()?;
            let level = r.i8()?;
            let n_moves = r.u8()?;
            for _ in 0..n_moves {
                r.i8()?; // move value
                r.i8()?; // move coordinate
            }
            if !is_valid_score(value) || player & opponent != 0 {
                continue;
            }
            book.merge_elem(
                &Board { player, opponent },
                BookElem {
                    value,
                    level,
                    leaf: Leaf::default(),
                    n_lines: 0,
                },
            );
        }
        Ok(book)
    }

    /// マジックの無い最初期の `.egbk` を読み込む。`level` は補完値。
    pub fn from_egbk1_bytes(bytes: &[u8], level: i8) -> Result<Self, EngineError> {
        let mut r = Reader::new(bytes);
        let n_boards = r.i32()?;
        if n_boards < 0 {
            return Err(EngineError::InvalidData(
                "not a recognizable opening book".to_string(),
            ));
        }
        // 想定サイズと食い違うなら別形式とみなす (誤判定を避ける)。
        let expected = 4 + n_boards as usize * 17;
        if bytes.len() != expected {
            return Err(EngineError::InvalidData(
                "not a recognizable opening book".to_string(),
            ));
        }

        let mut book = Self::empty();
        for _ in 0..n_boards {
            let player = r.u64()?;
            let opponent = r.u64()?;
            let value_raw = r.u8()?;
            // Egaroucid の import_file_egbk と同じ復元式。
            let value = -((value_raw as i8) as i32 - 64);
            if !(-64..=64).contains(&value) || player & opponent != 0 {
                continue;
            }
            book.merge_elem(
                &Board { player, opponent },
                BookElem {
                    value: value as i8,
                    level,
                    leaf: Leaf::default(),
                    n_lines: 0,
                },
            );
        }
        Ok(book)
    }

    /// `.egbk3` 形式のバイト列を作る。
    pub fn to_egbk3_bytes(&self) -> Vec<u8> {
        self.to_egbk3_bytes_with_level(None)
    }

    /// `.egbk3` 形式のバイト列を作る。`level` を指定すると全局面の level と
    /// leaf level をその値で上書きする (Egaroucid の `save_egbk3(file, level)`)。
    pub fn to_egbk3_bytes_with_level(&self, level: Option<i8>) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(EGBK3_HEADER_SIZE + self.positions.len() * EGBK3_RECORD_SIZE);
        out.extend_from_slice(EGBK_MAGIC);
        out.push(EGBK_VERSION);
        out.extend_from_slice(&(self.positions.len() as i32).to_le_bytes());

        for (board, elem) in self.positions.iter() {
            let (elem_level, leaf_level) = match level {
                Some(level) => (level, level),
                None => (elem.level, elem.leaf.level),
            };
            out.extend_from_slice(&board.player.to_le_bytes());
            out.extend_from_slice(&board.opponent.to_le_bytes());
            out.push(elem.value as u8);
            out.push(elem_level as u8);
            out.extend_from_slice(&elem.n_lines.to_le_bytes());
            out.push(elem.leaf.value as u8);
            out.push(elem.leaf.mv as u8);
            out.push(leaf_level as u8);
        }
        out
    }

    /// `.egbk3` 形式でファイルに書き出す。
    pub fn save(&self, path: &str) -> Result<(), EngineError> {
        std::fs::write(path, self.to_egbk3_bytes())?;
        Ok(())
    }

    /// `.egbk3` 形式でファイルに書き出し、level を上書きする。
    pub fn save_with_level(&self, path: &str, level: i8) -> Result<(), EngineError> {
        std::fs::write(path, self.to_egbk3_bytes_with_level(Some(level)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D3: u8 = 19;

    fn sample_book() -> Book {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);

        book.register_representative(
            root.unique_board(),
            BookElem {
                value: 2,
                level: 21,
                leaf: Leaf {
                    value: 1,
                    mv: 26,
                    level: 21,
                },
                n_lines: 7,
            },
        );
        book.register(&child, BookElem::new(-2, 21));
        book
    }

    /// Egaroucid の `save_egbk3` と同じ順序の書き出しを並べた C プログラムが
    /// 実際に出力したバイト列。ヘッダ 14 B + 局面 1 つ (25 B)。
    const EGBK3_GOLDEN: [u8; 39] = [
        0x44, 0x49, 0x43, 0x55, 0x4f, 0x52, 0x41, 0x47, 0x45, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x10, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x10, 0x00, 0x00, 0x00,
        0x02, 0x15, 0x07, 0x00, 0x00, 0x00, 0x01, 0x1a, 0x15,
    ];

    // 上のバイト列は C 版と 1 バイトも違わないこと。
    const _: () = assert!(EGBK3_GOLDEN.len() == EGBK3_HEADER_SIZE + EGBK3_RECORD_SIZE);

    #[test]
    fn header_is_14_bytes_and_starts_with_the_egaroucid_magic() {
        let bytes = Book::empty().to_egbk3_bytes();
        assert_eq!(bytes.len(), EGBK3_HEADER_SIZE);
        assert_eq!(&bytes[0..9], b"DICUORAGE");
        assert_eq!(bytes[9], 3);
        assert_eq!(i32::from_le_bytes(bytes[10..14].try_into().unwrap()), 0);
    }

    #[test]
    fn record_is_25_bytes_with_the_egaroucid_layout() {
        let book = sample_book();
        let bytes = book.to_egbk3_bytes();
        assert_eq!(
            bytes.len(),
            EGBK3_HEADER_SIZE + 2 * EGBK3_RECORD_SIZE,
            "2 positions expected"
        );

        let (board, elem) = book.iter().next().unwrap();
        let record = &bytes[EGBK3_HEADER_SIZE..EGBK3_HEADER_SIZE + EGBK3_RECORD_SIZE];
        assert_eq!(
            u64::from_le_bytes(record[0..8].try_into().unwrap()),
            board.player
        );
        assert_eq!(
            u64::from_le_bytes(record[8..16].try_into().unwrap()),
            board.opponent
        );
        assert_eq!(record[16] as i8, elem.value);
        assert_eq!(record[17] as i8, elem.level);
        assert_eq!(
            u32::from_le_bytes(record[18..22].try_into().unwrap()),
            elem.n_lines
        );
        assert_eq!(record[22] as i8, elem.leaf.value);
        assert_eq!(record[23] as i8, elem.leaf.mv);
        assert_eq!(record[24] as i8, elem.leaf.level);
    }

    #[test]
    fn to_egbk3_bytes_matches_the_bytes_written_by_egaroucid() {
        let mut book = Book::empty();
        book.register_representative(
            Board::new().unique_board(),
            BookElem {
                value: 2,
                level: 21,
                leaf: Leaf {
                    value: 1,
                    mv: 26,
                    level: 21,
                },
                n_lines: 7,
            },
        );
        assert_eq!(book.to_egbk3_bytes(), EGBK3_GOLDEN);
    }

    #[test]
    fn from_egbk3_bytes_reads_a_book_written_by_egaroucid() {
        let book = Book::from_bytes(&EGBK3_GOLDEN).unwrap();
        assert_eq!(book.len(), 1);

        let elem = book.root().unwrap();
        assert_eq!(elem.value, 2);
        assert_eq!(elem.level, 21);
        assert_eq!(elem.n_lines, 7);
        assert_eq!(elem.leaf.value, 1);
        assert_eq!(elem.leaf.level, 21);
        assert!(elem.leaf.is_move());
    }

    #[test]
    fn egbk3_round_trip_preserves_every_field() {
        let book = sample_book();
        let loaded = Book::from_egbk3_bytes(&book.to_egbk3_bytes()).unwrap();

        assert_eq!(loaded.len(), book.len());
        for ((ba, ea), (bb, eb)) in book.iter().zip(loaded.iter()) {
            assert_eq!(ba, bb);
            assert_eq!(ea, eb);
        }
    }

    #[test]
    fn save_with_level_overrides_every_level() {
        let book = sample_book();
        let loaded = Book::from_egbk3_bytes(&book.to_egbk3_bytes_with_level(Some(30))).unwrap();
        for (_, elem) in loaded.iter() {
            assert_eq!(elem.level, 30);
            assert_eq!(elem.leaf.level, 30);
        }
    }

    #[test]
    fn from_bytes_rejects_foreign_and_broken_files() {
        assert!(Book::from_bytes(b"JUNKJUNKJUNK").is_err());

        let mut bytes = EGBK3_GOLDEN.to_vec();
        bytes[9] = 9; // unsupported version
        assert!(Book::from_bytes(&bytes).is_err());

        let mut truncated = EGBK3_GOLDEN.to_vec();
        truncated.truncate(truncated.len() - 3);
        assert!(Book::from_bytes(&truncated).is_err());
    }

    #[test]
    fn from_egbk3_bytes_skips_broken_records() {
        let mut bytes = EGBK3_GOLDEN.to_vec();
        // value を範囲外にすると、その局面は読み飛ばされる。
        bytes[EGBK3_HEADER_SIZE + 16] = 100;
        let book = Book::from_egbk3_bytes(&bytes).unwrap();
        assert_eq!(book.len(), 0);

        // player と opponent が重なっていても読み飛ばす。
        let mut bytes = EGBK3_GOLDEN.to_vec();
        let overlap = 0x0000_0018_1800_0000u64.to_le_bytes();
        bytes[EGBK3_HEADER_SIZE..EGBK3_HEADER_SIZE + 8].copy_from_slice(&overlap);
        bytes[EGBK3_HEADER_SIZE + 8..EGBK3_HEADER_SIZE + 16].copy_from_slice(&overlap);
        assert_eq!(Book::from_egbk3_bytes(&bytes).unwrap().len(), 0);
    }

    #[test]
    fn duplicate_boards_keep_the_higher_level() {
        use super::super::elem::{LEVEL_UNDEFINED, MOVE_UNDEFINED, SCORE_UNDEFINED};
        let mut bytes = Vec::new();
        bytes.extend_from_slice(EGBK_MAGIC);
        bytes.push(3);
        bytes.extend_from_slice(&2i32.to_le_bytes());
        for (value, level) in [(3i8, 5i8), (7i8, 20i8)] {
            let board = Board::new().unique_board();
            bytes.extend_from_slice(&board.player.to_le_bytes());
            bytes.extend_from_slice(&board.opponent.to_le_bytes());
            bytes.push(value as u8);
            bytes.push(level as u8);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.push(SCORE_UNDEFINED as u8);
            bytes.push(MOVE_UNDEFINED as u8);
            bytes.push(LEVEL_UNDEFINED as u8);
        }

        let book = Book::from_egbk3_bytes(&bytes).unwrap();
        assert_eq!(book.len(), 1);
        assert_eq!(book.root().unwrap().value, 7);
        assert_eq!(book.root().unwrap().level, 20);
    }

    #[test]
    fn egbk2_legacy_format_is_readable() {
        let board = Board::new().unique_board();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(EGBK_MAGIC);
        bytes.push(2);
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&board.player.to_le_bytes());
        bytes.extend_from_slice(&board.opponent.to_le_bytes());
        bytes.push(4u8); // value
        bytes.push(12u8); // level
        bytes.push(2u8); // n_moves
        bytes.extend_from_slice(&[4u8, 19u8, 2u8, 26u8]);

        let book = Book::from_bytes(&bytes).unwrap();
        assert_eq!(book.len(), 1);
        assert_eq!(book.root().unwrap().value, 4);
        assert_eq!(book.root().unwrap().level, 12);
        // v2 の着手リストは持ち越さない。
        assert!(!book.root().unwrap().leaf.is_move());
    }

    #[test]
    fn egbk1_legacy_format_is_readable() {
        let board = Board::new().unique_board();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&board.player.to_le_bytes());
        bytes.extend_from_slice(&board.opponent.to_le_bytes());
        // value = -((raw as i8) - 64) なので raw = 66 -> -2
        bytes.push(66u8);

        let book = Book::from_bytes(&bytes).unwrap();
        assert_eq!(book.len(), 1);
        assert_eq!(book.root().unwrap().value, -2);
    }
}
