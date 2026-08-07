//! Edax の book (`book.dat`) の取り込みと書き出し。
//!
//! Edax 形式は着手リスト (link) を持つ木構造だが、この book は link を持たない
//! ので変換時に組み替えが要る。
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
//! # 上下界
//!
//! Edax は 3 つのエンジンの中で唯一、評価値の上下界をファイルに持っている。
//! この book も上下界を持つので、**そのまま移せる数少ない情報**になっている。
//! ただし Edax の上下界は探索窓をそのまま保存したもので、子から積み上げた
//! ものではない (この book の [`Book::propagate`] は積み上げる)。取り込みでは
//! 探索の深さから見積もった窓と Edax の窓の狭い方を採る。
//!
//! # 読み込み
//!
//! link は捨て、盤面・評価値・上下界・level を取り込む。leaf は「その手を
//! 打った先の子局面」として符号を反転して登録し直す。合法手が無い局面
//! (Edax が持つパス局面) は取り込まない。
//!
//! # 書き出し
//!
//! 子局面を引いて link を合成し、Edax が必要とするパス局面を作って書き足す。
//! 勝敗数は 0 で埋める。

use super::sym::representative_board;
use super::value::{
    clamp_score, is_valid_score, BookValue, ValueFlags, SCORE_MAX, SELECTIVITY_EXACT,
};
use super::{Book, BookMove};
use crate::board::board::Board;
use crate::EngineError;
use std::collections::BTreeSet;
use std::io::Write;

/// Edax book のヘッダ先頭 8 バイト (`EDAX` + `BOOK` の little endian 表現)。
pub const EDAX_BOOK_MAGIC: &[u8; 8] = b"XADEKOOB";
/// Edax が受け付ける book のバージョン。
pub const EDAX_BOOK_VERSION: u8 = 4;
/// 書き出す release 番号。Edax は読み込み時に検査しない。
pub const EDAX_BOOK_RELEASE: u8 = 4;

/// Edax ヘッダのバイト数。
const EDAX_HEADER_SIZE: usize = 42;

/// パスを表す Edax の疑似座標。
const EDAX_MOVE_PASS: u8 = 64;
/// 手が無いことを表す Edax の疑似座標。
const EDAX_MOVE_NOMOVE: u8 = 65;

/// Edax の level を深さに移せないときに使う選択度。
const IMPORTED_SELECTIVITY: u8 = 3;

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

/// Edax の 1 レコードから [`BookValue`] を組み立てる。
///
/// 探索の深さから見積もった窓と、Edax がファイルに持っている窓の
/// **狭い方**を採る。
fn value_from_edax(score: i8, lower: i16, upper: i16, level: i8, n_empties: u8) -> BookValue {
    let level = level.max(0) as u8;
    let mut value = if level >= n_empties {
        BookValue::searched(score, n_empties, n_empties, SELECTIVITY_EXACT)
    } else {
        BookValue::searched(score, n_empties, level, IMPORTED_SELECTIVITY)
    };

    let lower = lower.clamp(-(SCORE_MAX as i16), SCORE_MAX as i16) as i8;
    let upper = upper.clamp(-(SCORE_MAX as i16), SCORE_MAX as i16) as i8;
    if lower <= upper {
        value.lower = value.lower.max(lower).min(score);
        value.upper = value.upper.min(upper).max(score);
    }
    if value.lower == value.upper {
        value.flags = value
            .flags
            .union(ValueFlags::EXACT)
            .union(ValueFlags::ENDGAME);
    }
    value
}

/// [`BookValue`] を Edax の level に落とす。
fn edax_level(value: &BookValue, n_empties: u8) -> u8 {
    if value.is_exact() {
        return n_empties.clamp(1, 60);
    }
    value.depth.clamp(1, 60)
}

impl Book {
    /// Edax 形式の book を読み込む。
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
            r.u32()?; // n_lines (子から数え直せるので持たない)
            let score = r.i16()?;
            let lower = r.i16()?;
            let upper = r.i16()?;
            let n_link = r.u8()?;
            let level = r.i8()?;
            for _ in 0..n_link {
                r.i8()?; // link score (link は保持しない)
                r.u8()?; // link move
            }
            let leaf_value = r.i8()?;
            let leaf_move = r.u8()?;

            if player & opponent != 0 {
                continue;
            }
            let board = Board { player, opponent };
            let n_empties = board.empties_count() as u8;

            // Edax のパス局面 (合法手なし) は取り込まない。
            if (-(SCORE_MAX as i16)..=SCORE_MAX as i16).contains(&score) && board.moves() != 0 {
                book.update(
                    &board,
                    value_from_edax(score as i8, lower, upper, level, n_empties),
                );
            }

            // leaf は「その手を指した先の局面」として登録し直す。
            if is_valid_score(leaf_value)
                && leaf_move < 64
                && board.moves() & (1u64 << leaf_move) != 0
            {
                let child = board.make_move(1u64 << leaf_move);
                let (child_board, sign) = if child.moves() == 0 && child.opponent_moves() != 0 {
                    (child.passed(), 1)
                } else {
                    (child, -1)
                };
                let child_score = clamp_score(sign * leaf_value as i32);
                let child_empties = child_board.empties_count() as u8;
                book.update(
                    &child_board,
                    value_from_edax(
                        child_score,
                        -(SCORE_MAX as i16),
                        SCORE_MAX as i16,
                        level,
                        child_empties,
                    ),
                );
            }
        }
        Ok(book)
    }

    /// Edax 形式の book をファイルから読み込む。
    pub fn load_edax(path: &str) -> Result<Self, EngineError> {
        Self::from_edax_bytes(&std::fs::read(path)?)
    }

    /// Edax 形式のバイト列を作る。
    ///
    /// `level` にはヘッダに書く探索レベルを渡す。`None` なら book 内の最大値。
    /// `date` は `(year, month, day, hour, minute, second)`。
    ///
    /// **値がまだ入っていない局面は書き出さない** ([`Book::n_undefined`])。
    pub fn to_edax_bytes(&self, level: Option<i8>, date: (i16, i8, i8, i8, i8, i8)) -> Vec<u8> {
        let header_level = level
            .map(|l| l as i32)
            .unwrap_or_else(|| {
                self.iter()
                    .map(|(_, board, value)| edax_level(value, board.empties_count() as u8) as i32)
                    .max()
                    .unwrap_or(1)
            })
            .max(1);

        // Edax はパス局面自体も book に必要とするので作って書き足す。
        let pass_boards = self.pass_boards();

        // Edax は 65 - n_discs (= 空きマス数 + 1) の最小値を書く。
        let mut n_empties = 64i32;
        for board in self
            .iter()
            .map(|(_, board, _)| board)
            .chain(pass_boards.iter())
        {
            n_empties = n_empties.min(board.empties_count() as i32 + 1);
        }

        let mut out = Vec::with_capacity(EDAX_HEADER_SIZE + (self.len() + pass_boards.len()) * 56);
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
        let n_position = (self.len() - self.n_undefined() + pass_boards.len()) as i32;
        out.extend_from_slice(&n_position.to_le_bytes());

        // パス局面: 「パスする」という link 1 本だけを持つ。
        for pass_board in &pass_boards {
            let passed = pass_board.passed();
            let value = self.value_of(&passed).unwrap_or_else(BookValue::undefined);
            let record_level = level
                .map(|l| l.clamp(1, 60) as u8)
                .unwrap_or_else(|| edax_level(&value, pass_board.empties_count() as u8));
            write_edax_position(
                &mut out,
                pass_board,
                &value,
                1,
                record_level,
                &[BookMove {
                    mv: EDAX_MOVE_PASS,
                    value,
                }],
                None,
            );
        }

        let n_lines = self.count_lines();
        for (id, board, value) in self.iter() {
            // Edax も「値が無い」を表せないので、まだ値の無い局面は書かない。
            if !value.is_defined() {
                continue;
            }
            let n_empties_here = board.empties_count() as u8;
            let record_level = level
                .map(|l| l.clamp(1, 60) as u8)
                .unwrap_or_else(|| edax_level(value, n_empties_here));
            let lines = n_lines[id.ply as usize][id.slot as usize].max(1);

            if board.moves() == 0 {
                // 合法手が無い局面。パスできるなら pass link を 1 本持たせる。
                let passed = board.passed();
                let passed_value = if passed.moves() != 0 {
                    self.value_of(&passed)
                } else {
                    None
                };
                match passed_value {
                    Some(passed_value) => write_edax_position(
                        &mut out,
                        board,
                        &passed_value,
                        lines,
                        record_level,
                        &[BookMove {
                            mv: EDAX_MOVE_PASS,
                            value: passed_value,
                        }],
                        None,
                    ),
                    None => write_edax_position(
                        &mut out,
                        board,
                        value,
                        lines,
                        record_level,
                        &[],
                        Some((
                            if value.is_defined() { value.score } else { 0 },
                            EDAX_MOVE_PASS,
                        )),
                    ),
                }
                continue;
            }

            let mut links = self.moves(board);
            let mobility = board.moves().count_ones() as usize;

            // link に入らなかった手のうち最善のものを leaf にする。
            let mut leaf = None;
            if links.len() != mobility {
                let frontier = self.table().frontier(id);
                let usable = frontier.has_move()
                    && board.moves() & (1u64 << frontier.mv) != 0
                    && !links.iter().any(|l| l.mv as i8 == frontier.mv);
                if usable {
                    leaf = Some((frontier.score, frontier.mv as u8));
                } else if !links.is_empty() {
                    // 候補が無ければ、最悪の link を leaf に降格させる
                    // (Edax は leaf が無いと未展開の手を持てない)。
                    let worst = links
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, l)| l.value.score)
                        .map(|(i, _)| i)
                        .expect("links is not empty");
                    let demoted = links.remove(worst);
                    leaf = Some((demoted.value.score, demoted.mv));
                }
            }

            write_edax_position(&mut out, board, value, lines, record_level, &links, leaf);
        }
        out
    }

    /// Edax 形式でファイルに書き出す。
    ///
    /// 一時ファイルに書いてから置き換える。
    pub fn save_edax(&self, path: &str, level: Option<i8>) -> Result<(), EngineError> {
        let tmp = format!("{path}.tmp");
        {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&self.to_edax_bytes(level, now_date()))?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// book の局面から 1 手進めるとパスになる局面を集める。
    ///
    /// Edax は「パスする」局面自体を book に持つ必要があるため、書き出し時に
    /// 補う。
    fn pass_boards(&self) -> Vec<Board> {
        let mut result = BTreeSet::new();
        for (_, board, value) in self.iter() {
            if !value.is_defined() {
                continue;
            }
            let mut legal = board.moves();
            while legal != 0 {
                let mv = legal.trailing_zeros() as u8;
                legal &= legal - 1;
                let child = board.make_move(1u64 << mv);
                // 子でパスが必要 かつ パス後の局面が book にある場合だけ補う。
                if child.moves() == 0
                    && child.opponent_moves() != 0
                    && self
                        .value_of(&child.passed())
                        .is_some_and(|v| v.is_defined())
                {
                    let (key, _) = representative_board(&child);
                    if !self.contains(&key) {
                        result.insert((key.player, key.opponent));
                    }
                }
            }
        }
        result
            .into_iter()
            .map(|(player, opponent)| Board { player, opponent })
            .collect()
    }
}

fn write_edax_position(
    out: &mut Vec<u8>,
    board: &Board,
    value: &BookValue,
    n_lines: u32,
    level: u8,
    links: &[BookMove],
    leaf: Option<(i8, u8)>,
) {
    let score = if value.is_defined() { value.score } else { 0 };
    out.extend_from_slice(&board.player.to_le_bytes());
    out.extend_from_slice(&board.opponent.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // n_wins
    out.extend_from_slice(&0u32.to_le_bytes()); // n_draws
    out.extend_from_slice(&0u32.to_le_bytes()); // n_losses
    out.extend_from_slice(&n_lines.to_le_bytes());
    out.extend_from_slice(&(score as i16).to_le_bytes());
    // Edax の唯一の「そのまま移せる」欄。積み上げた上下界を書く。
    out.extend_from_slice(&(value.lower as i16).to_le_bytes());
    out.extend_from_slice(&(value.upper as i16).to_le_bytes());
    out.push(links.len().min(u8::MAX as usize) as u8);
    out.push(level);
    for link in links.iter().take(u8::MAX as usize) {
        out.push(link.value.score as u8);
        out.push(link.mv);
    }
    match leaf {
        Some((score, mv)) => {
            out.push(score as u8);
            out.push(mv);
        }
        None => {
            out.push(0);
            out.push(EDAX_MOVE_NOMOVE);
        }
    }
}

/// 現在時刻を Edax のヘッダ用に取り出す。取れなければ 0 で埋める。
fn now_date() -> (i16, i8, i8, i8, i8, i8) {
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return (1970, 1, 1, 0, 0, 0);
    };
    let secs = now.as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);

    // 1970-01-01 からの日数を暦に直す (Howard Hinnant の civil_from_days)。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (
        y as i16,
        m as i8,
        d as i8,
        hour as i8,
        minute as i8,
        second as i8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::value::Frontier;

    const D3: u8 = 19;

    /// gcc でコンパイルした Edax の `book_save` 相当のプログラムが実際に
    /// 書き出したヘッダ 42 バイト (level 21, n_empties 61, n_nodes 1)。
    const EDAX_HEADER_GOLDEN: [u8; EDAX_HEADER_SIZE] = [
        0x58, 0x41, 0x44, 0x45, // "XADE"
        0x4b, 0x4f, 0x4f, 0x42, // "KOOB"
        4, 4, // version, release
        0xe8, 0x07, // year = 2024
        1, 2, 3, 4, 5, 0, // month, day, hour, minute, second, padding
        21, 0, 0, 0, // level
        61, 0, 0, 0, // n_empties
        0, 0, 0, 0, // midgame_error
        0, 0, 0, 0, // endcut_error
        0, 0, 0, 0, // verbosity
        1, 0, 0, 0, // n_nodes
    ];

    fn sample_book() -> Book {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        book.insert(&root, BookValue::searched(2, 60, 21, 3));
        book.insert(&child, BookValue::searched(-2, 59, 21, 3));
        book
    }

    #[test]
    fn the_header_matches_edax_byte_for_byte() {
        let mut book = Book::empty();
        book.insert(&Board::new(), BookValue::searched(0, 60, 21, 3));
        let bytes = book.to_edax_bytes(Some(21), (2024, 1, 2, 3, 4, 5));
        assert_eq!(&bytes[..EDAX_HEADER_SIZE], &EDAX_HEADER_GOLDEN);
    }

    #[test]
    fn round_trip_keeps_the_positions_and_scores() {
        let book = sample_book();
        let bytes = book.to_edax_bytes(Some(21), (2024, 1, 1, 0, 0, 0));
        let loaded = Book::from_edax_bytes(&bytes).unwrap();

        assert_eq!(loaded.value_of(&Board::new()).unwrap().score, 2);
        let child = Board::new().make_move(1u64 << D3);
        assert_eq!(loaded.value_of(&child).unwrap().score, -2);
    }

    #[test]
    fn the_bounds_survive_the_round_trip() {
        let mut book = Book::empty();
        let value = BookValue::from_search(4, 40, 12, 3, 3);
        book.insert(&Board::new(), value);
        let bytes = book.to_edax_bytes(Some(12), (2024, 1, 1, 0, 0, 0));

        // ファイルにそのまま書かれていること。
        let at = EDAX_HEADER_SIZE + 8 + 8 + 16 + 2;
        assert_eq!(i16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()), 1);
        assert_eq!(
            i16::from_le_bytes(bytes[at + 2..at + 4].try_into().unwrap()),
            7
        );

        let loaded = Book::from_edax_bytes(&bytes).unwrap();
        let got = loaded.value_of(&Board::new()).unwrap();
        assert_eq!((got.lower, got.upper), (1, 7));
    }

    #[test]
    fn links_point_at_the_registered_children() {
        let book = sample_book();
        let bytes = book.to_edax_bytes(Some(21), (2024, 1, 1, 0, 0, 0));
        let loaded_back = Book::from_edax_bytes(&bytes).unwrap();
        // 往復して局面が失われていないこと。
        assert!(loaded_back.contains(&Board::new()));
        assert!(loaded_back.contains(&Board::new().make_move(1u64 << D3)));

        // 初期盤面のレコードには 4 本の link が入る (4 手とも同じ子に落ちる)。
        let at = EDAX_HEADER_SIZE + 8 + 8 + 16 + 6;
        let n_link = bytes[at];
        assert_eq!(n_link, 4);
    }

    #[test]
    fn the_frontier_becomes_the_edax_leaf() {
        let mut book = Book::empty();
        let board = Board::new().make_move(1u64 << D3);
        let mv = board.moves().trailing_zeros() as u8;
        book.insert(&board, BookValue::searched(1, 59, 21, 3));
        book.set_frontier(
            &board,
            Frontier {
                mv: mv as i8,
                score: -3,
                depth: 21,
            },
        );

        let bytes = book.to_edax_bytes(Some(21), (2024, 1, 1, 0, 0, 0));
        // 1 局面だけなので link は 0 本、leaf が最後の 2 バイト。
        let leaf_score = bytes[bytes.len() - 2] as i8;
        let leaf_move = bytes[bytes.len() - 1];
        assert_eq!(leaf_score, -3);
        // 保存されるのは正規形なので、座標もその向きになる。
        let id = book.table().locate(&board).unwrap();
        let stored = *book.table().board(id);
        assert_ne!(stored.moves() & (1u64 << leaf_move), 0);

        // 往復すると leaf の手の先が子局面として入る。
        let loaded = Book::from_edax_bytes(&bytes).unwrap();
        assert_eq!(
            loaded.value_of(&board.make_move(1u64 << mv)).unwrap().score,
            3
        );
    }

    #[test]
    fn an_edax_leaf_becomes_a_child_position() {
        let mut book = Book::empty();
        let board = Board::new();
        book.insert(&board, BookValue::searched(0, 60, 21, 3));
        book.set_frontier(
            &board,
            Frontier {
                mv: D3 as i8,
                score: 4,
                depth: 21,
            },
        );

        let bytes = book.to_edax_bytes(Some(21), (2024, 1, 1, 0, 0, 0));
        let loaded = Book::from_edax_bytes(&bytes).unwrap();
        // leaf の手を打った先が、符号を反転した値で登録される。
        let child = board.make_move(1u64 << D3);
        assert_eq!(loaded.value_of(&child).unwrap().score, -4);
    }

    #[test]
    fn positions_without_a_value_are_not_written() {
        let mut book = Book::empty();
        book.insert(&Board::new(), BookValue::undefined());
        book.insert(
            &Board::new().make_move(1u64 << D3),
            BookValue::searched(1, 59, 21, 3),
        );
        assert_eq!(book.n_undefined(), 1);

        let bytes = book.to_edax_bytes(Some(21), (2024, 1, 1, 0, 0, 0));
        // ヘッダの局面数が実際に書いたレコード数と一致すること。
        assert_eq!(
            i32::from_le_bytes(bytes[38..42].try_into().unwrap()),
            1,
            "値のある 1 局面だけが書かれる"
        );
        let loaded = Book::from_edax_bytes(&bytes).unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn a_bad_magic_is_rejected() {
        let mut bytes = sample_book().to_edax_bytes(None, (2024, 1, 1, 0, 0, 0));
        bytes[0] = b'Z';
        assert!(Book::from_edax_bytes(&bytes).is_err());
    }

    #[test]
    fn a_wrong_version_is_rejected() {
        let mut bytes = sample_book().to_edax_bytes(None, (2024, 1, 1, 0, 0, 0));
        bytes[8] = 3;
        let err = Book::from_edax_bytes(&bytes).unwrap_err();
        assert!(format!("{err}").contains("version"), "{err}");
    }

    #[test]
    fn a_truncated_file_is_rejected() {
        let bytes = sample_book().to_edax_bytes(None, (2024, 1, 1, 0, 0, 0));
        assert!(Book::from_edax_bytes(&bytes[..EDAX_HEADER_SIZE + 10]).is_err());
    }

    #[test]
    fn now_date_produces_a_plausible_calendar_date() {
        let (year, month, day, hour, minute, second) = now_date();
        assert!((2020..2200).contains(&year), "year = {year}");
        assert!((1..=12).contains(&month));
        assert!((1..=31).contains(&day));
        assert!((0..24).contains(&hour));
        assert!((0..60).contains(&minute));
        assert!((0..60).contains(&second));
    }
}
