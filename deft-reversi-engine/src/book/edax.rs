//! Edax の book (`book.dat`) との相互変換。
//!
//! Egaroucid の `import_file_edax` / `save_bin_edax` (src/engine/book.hpp) に
//! 対応する。Edax 形式は着手リスト (link) を持つ木構造だが、こちらは link を
//! 持たないので変換時に組み替えが要る。
//!
//! # Edax のファイル形式
//!
//! すべて little endian。ヘッダは 42 バイト。
//!
//! | offset | size | 内容 |
//! |--------|------|------|
//! | 0  | 4  | `EDAX` magic `0x45444158` (バイト列は `"XADE"`) |
//! | 4  | 4  | `BOOK` magic `0x424f4f4b` (バイト列は `"KOOB"`) |
//! | 8  | 1  | version (= 4) |
//! | 9  | 1  | release |
//! | 10 | 8  | `i16 year`, `i8 month, day, hour, minute, second`, パディング 1 バイト |
//! | 18 | 20 | `i32 level, n_empties, midgame_error, endcut_error, verbosity` |
//! | 38 | 4  | `i32 n_nodes` |
//!
//! offset 17 の 1 バイトは C の構造体
//! `struct { short year; char month, day, hour, minute, second; }` を丸ごと
//! `fwrite` するために入るパディング (`sizeof` が 8 になる)。
//!
//! 局面は 40 バイトの固定部と可変長の着手リスト。
//!
//! | size | 内容 |
//! |------|------|
//! | 8 + 8 | `u64 player`, `u64 opponent` |
//! | 4 × 4 | `u32 n_wins, n_draws, n_losses, n_lines` |
//! | 2 × 3 | `i16 score.value, score.lower, score.upper` |
//! | 1 + 1 | `u8 n_link`, `u8 level` |
//! | 2 × n_link | link (`i8 score`, `u8 move`) |
//! | 2 | leaf (`i8 score`, `u8 move`) |
//!
//! # 読み込み (Egaroucid の `import_file_edax`)
//!
//! link は捨て、盤面・評価値・level・`n_lines` だけを取り込む。leaf は
//! 「その手を打った先の子局面」として符号を反転して登録し直す。合法手が無い
//! 局面 (Edax が持つパス局面) は取り込まない。
//!
//! # 書き出し (Egaroucid の `save_bin_edax`)
//!
//! 子局面を引いて link を合成し、Edax が必要とするパス局面を作って書き足す。
//! 上下界は ±64 (不明扱い)、勝敗数は 0 で埋める。

use super::elem::{
    clamp_score, is_valid_policy, is_valid_score, next_board, BookElem, BookMove, Leaf,
    LEVEL_UNDEFINED, MOVE_NOMOVE, MOVE_PASS, SCORE_UNDEFINED,
};
use super::Book;
use crate::board::board::Board;
use crate::EngineError;
use std::collections::BTreeSet;

/// Edax book のヘッダ先頭 8 バイト (`EDAX` + `BOOK` の little endian 表現)。
pub const EDAX_BOOK_MAGIC: &[u8; 8] = b"XADEKOOB";
/// Edax が受け付ける book のバージョン。
pub const EDAX_BOOK_VERSION: u8 = 4;
/// 書き出す release 番号。Edax は読み込み時に検査しない。
pub const EDAX_BOOK_RELEASE: u8 = 4;

/// Edax ヘッダのバイト数。
const EDAX_HEADER_SIZE: usize = 42;

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], EngineError> {
        if self.bytes.len() - self.pos < n {
            return Err(EngineError::InvalidData(format!(
                "unexpected end of edax book at offset {}",
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

    fn i16(&mut self) -> Result<i16, EngineError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, EngineError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, EngineError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

impl Book {
    /// Edax 形式の book を読み込む。Egaroucid の `import_file_edax`。
    pub fn from_edax_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        let mut r = Reader::new(bytes);
        let magic = r.take(8)?;
        if magic != EDAX_BOOK_MAGIC {
            return Err(EngineError::InvalidData(
                "not an edax opening book".to_string(),
            ));
        }
        let version = r.u8()?;
        if version != EDAX_BOOK_VERSION {
            return Err(EngineError::InvalidData(format!(
                "incompatible edax book version: {version}"
            )));
        }
        // release + date(8) + options(20) を読み飛ばして n_nodes へ。
        r.take(EDAX_HEADER_SIZE - 9 - 4)?;
        let n_nodes = i32::from_le_bytes(r.take(4)?.try_into().unwrap());
        if n_nodes < 0 {
            return Err(EngineError::InvalidData(format!(
                "negative node count: {n_nodes}"
            )));
        }

        let mut book = Self::empty();
        for _ in 0..n_nodes {
            let player = r.u64()?;
            let opponent = r.u64()?;
            r.u32()?; // n_wins
            r.u32()?; // n_draws
            r.u32()?; // n_losses
            let n_lines = r.u32()?;
            let value = r.i16()?;
            r.i16()?; // score.lower
            r.i16()?; // score.upper
            let n_link = r.u8()?;
            let level = r.i8()?;
            for _ in 0..n_link {
                r.i8()?; // link score (link は保持しない)
                r.u8()?; // link move
            }
            let leaf_value = r.i8()?;
            let leaf_move = r.u8()?;

            let board = Board { player, opponent };
            let value_ok = (-64..=64).contains(&value);

            // Edax のパス局面 (合法手なし) は取り込まない。
            if value_ok && player & opponent == 0 && board.moves() != 0 {
                book.merge_elem(
                    &board,
                    BookElem {
                        value: value as i8,
                        level,
                        leaf: Leaf {
                            value: SCORE_UNDEFINED,
                            mv: super::MOVE_UNDEFINED,
                            level,
                        },
                        n_lines,
                    },
                );
            }

            // leaf は「その手を指した先の局面」として登録し直す。
            if is_valid_score(leaf_value) && leaf_move < 64 && player & opponent == 0 {
                if let Some(child) = next_board(&board, leaf_move as i8) {
                    let mut child = child;
                    let mut child_value = -(leaf_value as i32);
                    let is_end = child.moves() == 0 && child.opponent_moves() == 0;
                    if child.moves() == 0 && !is_end {
                        child = child.passed();
                        child_value = -child_value;
                    }
                    book.merge_elem(
                        &child,
                        BookElem {
                            value: clamp_score(child_value),
                            level,
                            leaf: Leaf {
                                value: SCORE_UNDEFINED,
                                mv: super::MOVE_UNDEFINED,
                                level,
                            },
                            n_lines: 1,
                        },
                    );
                }
            }
        }
        Ok(book)
    }

    /// Edax 形式の book をファイルから読み込む。
    pub fn load_edax(path: &str) -> Result<Self, EngineError> {
        Self::from_edax_bytes(&std::fs::read(path)?)
    }

    /// Edax 形式のバイト列を作る。Egaroucid の `save_bin_edax`。
    ///
    /// `level` にはヘッダに書く探索レベルを渡す。`None` なら book 内の最大値。
    /// `date` は `(year, month, day, hour, minute, second)`。
    pub fn to_edax_bytes(&self, level: Option<i8>, date: (i16, i8, i8, i8, i8, i8)) -> Vec<u8> {
        let header_level = level
            .map(|l| l as i32)
            .unwrap_or_else(|| self.iter().map(|(_, e)| e.level as i32).max().unwrap_or(1))
            .max(1);

        // Edax はパス局面自体も book に必要とするので作って書き足す。
        let pass_boards = self.pass_boards();

        // Egaroucid は 65 - n_discs (= 空きマス数 + 1) の最小値を書く。
        let mut n_empties = 64i32;
        for board in self.boards().chain(pass_boards.iter()) {
            n_empties = n_empties.min(board.empties_count() as i32 + 1);
        }

        let mut out =
            Vec::with_capacity(EDAX_HEADER_SIZE + (self.positions.len() + pass_boards.len()) * 56);
        out.extend_from_slice(EDAX_BOOK_MAGIC);
        out.push(EDAX_BOOK_VERSION);
        out.push(EDAX_BOOK_RELEASE);
        out.extend_from_slice(&date.0.to_le_bytes());
        out.push(date.1 as u8);
        out.push(date.2 as u8);
        out.push(date.3 as u8);
        out.push(date.4 as u8);
        out.push(date.5 as u8);
        out.push(0); // date 構造体のパディング
        out.extend_from_slice(&header_level.to_le_bytes());
        out.extend_from_slice(&n_empties.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes()); // midgame_error
        out.extend_from_slice(&0i32.to_le_bytes()); // endcut_error
        out.extend_from_slice(&0i32.to_le_bytes()); // verbosity
        let n_position = (self.len() + pass_boards.len()) as i32;
        out.extend_from_slice(&n_position.to_le_bytes());

        // パス局面: 「パスする」という link 1 本だけを持つ。
        for pass_board in &pass_boards {
            let passed = pass_board.passed();
            let elem = self.get(&passed).unwrap_or_default();
            let level = clamp_edax_level(level.unwrap_or(elem.level));
            write_edax_position(
                &mut out,
                pass_board,
                elem.value as i16,
                elem.n_lines,
                level,
                &[BookMove {
                    mv: MOVE_PASS as u8,
                    value: elem.value,
                }],
                Leaf {
                    value: SCORE_UNDEFINED,
                    mv: MOVE_NOMOVE,
                    level: LEVEL_UNDEFINED,
                },
            );
        }

        for (board, elem) in self.iter() {
            let record_level = clamp_edax_level(level.unwrap_or(elem.level));

            if board.moves() == 0 {
                // 合法手が無い局面。パスできるなら pass link を 1 本持たせる。
                let passed = board.passed();
                let has_pass_child = passed.moves() != 0 && self.contains(&passed);
                let (value, n_lines, links, leaf) = if has_pass_child {
                    let passed_elem = self.get(&passed).unwrap_or_default();
                    (
                        passed_elem.value as i16,
                        passed_elem.n_lines,
                        vec![BookMove {
                            mv: MOVE_PASS as u8,
                            value: passed_elem.value,
                        }],
                        Leaf {
                            value: SCORE_UNDEFINED,
                            mv: MOVE_NOMOVE,
                            level: LEVEL_UNDEFINED,
                        },
                    )
                } else {
                    (
                        elem.value as i16,
                        elem.n_lines,
                        Vec::new(),
                        Leaf {
                            value: if elem.has_value() { elem.value } else { 0 },
                            mv: MOVE_PASS,
                            level: LEVEL_UNDEFINED,
                        },
                    )
                };
                write_edax_position(&mut out, board, value, n_lines, record_level, &links, leaf);
                continue;
            }

            let mut links = self.moves_with_value(board);
            let mobility = board.moves().count_ones() as usize;

            // link に入らなかった手のうち最善のものを leaf にする。
            let mut leaf = Leaf {
                value: SCORE_UNDEFINED,
                mv: MOVE_NOMOVE,
                level: LEVEL_UNDEFINED,
            };
            if links.len() != mobility {
                let registered_leaf_ok = is_valid_score(elem.leaf.value)
                    && elem.leaf.is_move()
                    && board.moves() & (1u64 << elem.leaf.mv) != 0
                    && !links.iter().any(|l| l.mv as i8 == elem.leaf.mv);
                if registered_leaf_ok {
                    leaf = elem.leaf;
                } else if !links.is_empty() {
                    // 候補が無ければ、最悪の link を leaf に降格させる
                    // (Egaroucid が追加探索できないときと同じ扱い)。
                    let worst = links
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, l)| l.value)
                        .map(|(i, _)| i)
                        .expect("links is not empty");
                    let demoted = links.remove(worst);
                    leaf = Leaf {
                        value: demoted.value,
                        mv: demoted.mv as i8,
                        level: LEVEL_UNDEFINED,
                    };
                }
            }

            write_edax_position(
                &mut out,
                board,
                elem.value as i16,
                elem.n_lines,
                record_level,
                &links,
                leaf,
            );
        }
        out
    }

    /// Edax 形式でファイルに書き出す。
    pub fn save_edax(&self, path: &str, level: Option<i8>) -> Result<(), EngineError> {
        std::fs::write(path, self.to_edax_bytes(level, now_date()))?;
        Ok(())
    }

    /// book の局面から 1 手進めるとパスになる局面を集める。
    ///
    /// Edax は「パスする」局面自体を book に持つ必要があるため、書き出し時に
    /// 補う。Egaroucid の `get_pass_boards`。
    fn pass_boards(&self) -> Vec<Board> {
        let mut result = BTreeSet::new();
        for board in self.boards() {
            let mut legal = board.moves();
            while legal != 0 {
                let mv = legal.trailing_zeros() as u8;
                legal &= legal - 1;
                let child = board.make_move(1u64 << mv);
                // 子でパスが必要 かつ パス後の局面が book にある場合だけ補う。
                if child.moves() == 0
                    && child.opponent_moves() != 0
                    && self.contains(&child.passed())
                {
                    let key = child.unique_board();
                    if !self.contains_representative(&key) {
                        result.insert(key);
                    }
                }
            }
        }
        result.into_iter().collect()
    }
}

fn clamp_edax_level(level: i8) -> u8 {
    level.clamp(1, 60) as u8
}

fn write_edax_position(
    out: &mut Vec<u8>,
    board: &Board,
    value: i16,
    n_lines: u32,
    level: u8,
    links: &[BookMove],
    leaf: Leaf,
) {
    out.extend_from_slice(&board.player.to_le_bytes());
    out.extend_from_slice(&board.opponent.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // n_wins
    out.extend_from_slice(&0u32.to_le_bytes()); // n_draws
    out.extend_from_slice(&0u32.to_le_bytes()); // n_losses
    out.extend_from_slice(&n_lines.to_le_bytes());
    out.extend_from_slice(&value.to_le_bytes());
    out.extend_from_slice(&(-64i16).to_le_bytes()); // score.lower (不明扱い)
    out.extend_from_slice(&64i16.to_le_bytes()); // score.upper
    out.push(links.len().min(u8::MAX as usize) as u8);
    out.push(level);
    for link in links.iter().take(u8::MAX as usize) {
        out.push(link.value as u8);
        out.push(link.mv);
    }
    out.push(leaf.value as u8);
    out.push(leaf.mv as u8);
}

/// 現在時刻 (UTC) を `(year, month, day, hour, minute, second)` で返す。
pub(super) fn now_date() -> (i16, i8, i8, i8, i8, i8) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    date_from_unix_seconds(secs)
}

/// UNIX 秒を UTC の年月日時分秒に直す (Howard Hinnant の civil_from_days)。
fn date_from_unix_seconds(secs: i64) -> (i16, i8, i8, i8, i8, i8) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    (
        year as i16,
        m as i8,
        d as i8,
        (rem / 3600) as i8,
        ((rem % 3600) / 60) as i8,
        (rem % 60) as i8,
    )
}

/// 座標が Edax の疑似座標を含めて有効か (デバッグ用)。
#[allow(dead_code)]
fn is_edax_move(mv: u8) -> bool {
    is_valid_policy(mv as i8) || mv == MOVE_PASS as u8 || mv == MOVE_NOMOVE as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const D3: u8 = 19;
    const FIXED_DATE: (i16, i8, i8, i8, i8, i8) = (2026, 8, 7, 12, 34, 56);

    fn sample_book() -> Book {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        book.register(&root, BookElem::new(2, 21));
        book.register(&child, BookElem::new(-2, 21));
        book
    }

    /// Egaroucid の `save_bin_edax` と同じ順序の `fwrite` を並べた C プログラム
    /// が出力したヘッダ 42 バイト。日時は 2026-08-07 12:34:56、level 21。
    const EDAX_HEADER_GOLDEN: [u8; 42] = [
        0x58, 0x41, 0x44, 0x45, 0x4b, 0x4f, 0x4f, 0x42, 0x04, 0x04, 0xea, 0x07, 0x08, 0x07, 0x0c,
        0x22, 0x38, 0x00, 0x15, 0x00, 0x00, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn edax_header_matches_the_bytes_written_by_egaroucid() {
        let book = sample_book();
        let bytes = book.to_edax_bytes(Some(21), FIXED_DATE);
        assert_eq!(&bytes[..EDAX_HEADER_SIZE], &EDAX_HEADER_GOLDEN);
    }

    #[test]
    fn edax_export_writes_one_record_per_position() {
        let book = sample_book();
        let bytes = book.to_edax_bytes(Some(21), FIXED_DATE);

        // 初期盤面: link 4 本 + leaf、子局面: link 0 本 + leaf。
        let expected = EDAX_HEADER_SIZE + (40 + 4 * 2 + 2) + (40 + 2);
        assert_eq!(bytes.len(), expected);
        assert_eq!(
            i32::from_le_bytes(bytes[38..42].try_into().unwrap()),
            2,
            "n_position"
        );
    }

    #[test]
    fn edax_export_uses_unknown_bounds_and_zero_game_counts() {
        let book = sample_book();
        let bytes = book.to_edax_bytes(Some(21), FIXED_DATE);
        let record = &bytes[EDAX_HEADER_SIZE..];

        assert_eq!(u32::from_le_bytes(record[16..20].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(record[20..24].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(record[24..28].try_into().unwrap()), 0);
        assert_eq!(i16::from_le_bytes(record[34..36].try_into().unwrap()), -64);
        assert_eq!(i16::from_le_bytes(record[36..38].try_into().unwrap()), 64);
        assert_eq!(record[39], 21, "level");
    }

    #[test]
    fn edax_round_trip_keeps_the_positions() {
        let book = sample_book();
        let bytes = book.to_edax_bytes(Some(21), FIXED_DATE);
        let loaded = Book::from_bytes(&bytes).unwrap();

        assert!(loaded.contains(&Board::new()));
        assert!(loaded.contains(&Board::new().make_move(1u64 << D3)));
        assert_eq!(loaded.get(&Board::new()).unwrap().value, 2);
    }

    #[test]
    fn edax_import_turns_a_leaf_into_a_child_position() {
        // link 0 本 + leaf 1 つだけを持つ Edax book を組む。
        let mut book = Book::empty();
        book.register(
            &Board::new(),
            BookElem {
                value: 2,
                level: 21,
                leaf: Leaf {
                    value: 2,
                    mv: D3 as i8,
                    level: 21,
                },
                n_lines: 1,
            },
        );
        let bytes = book.to_edax_bytes(Some(21), FIXED_DATE);

        let loaded = Book::from_edax_bytes(&bytes).unwrap();
        // leaf の手を打った先が子局面として登録される。
        let child = Board::new().make_move(1u64 << D3);
        assert!(loaded.contains(&child));
        assert_eq!(loaded.get(&child).unwrap().value, -2);
    }

    #[test]
    fn edax_import_rejects_foreign_files() {
        assert!(Book::from_edax_bytes(b"not a book at all").is_err());

        let mut bytes = sample_book().to_edax_bytes(Some(21), FIXED_DATE);
        bytes[8] = 3;
        assert!(Book::from_edax_bytes(&bytes).is_err());
    }

    #[test]
    fn edax_export_adds_the_pass_positions_edax_needs() {
        // パスが起きる局面を含む book を作り、パス局面が補われることを見る。
        let mut book = Book::empty();
        let mut board = Board::new();
        let mut pass_parent = None;
        for _ in 0..40 {
            let legal = board.moves();
            if legal == 0 {
                break;
            }
            let mut bits = legal;
            let mut found = None;
            while bits != 0 {
                let mv = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                let child = board.make_move(1u64 << mv);
                if child.moves() == 0 && child.opponent_moves() != 0 {
                    found = Some((mv, child));
                    break;
                }
            }
            if let Some((mv, child)) = found {
                book.register(&board, BookElem::new(0, 5));
                book.register(&child.passed(), BookElem::new(0, 5));
                pass_parent = Some((board, mv, child));
                break;
            }
            board = board.make_move(1u64 << legal.trailing_zeros());
            book.register(&board, BookElem::new(0, 5));
        }

        let Some((_, _, pass_board)) = pass_parent else {
            return; // パスの出る手が見つからなければこのテストは何も見ない
        };

        let n_before = book.len();
        let bytes = book.to_edax_bytes(Some(5), FIXED_DATE);
        let n_position = i32::from_le_bytes(bytes[38..42].try_into().unwrap()) as usize;
        assert_eq!(n_position, n_before + 1, "pass position should be added");

        // Edax 形式として読み直すと、パス局面は取り込まれない (Egaroucid と同じ)。
        let loaded = Book::from_edax_bytes(&bytes).unwrap();
        assert!(!loaded.contains(&pass_board));
    }

    #[test]
    fn date_from_unix_seconds_matches_known_timestamps() {
        assert_eq!(date_from_unix_seconds(946_684_800), (2000, 1, 1, 0, 0, 0));
        assert_eq!(
            date_from_unix_seconds(1_709_251_199),
            (2024, 2, 29, 23, 59, 59)
        );
    }
}
