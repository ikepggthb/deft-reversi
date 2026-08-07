//! Egaroucid の book ファイル (`.egbk3` / `.egbk2` / `.egbk`) の取り込みと書き出し。
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
//! Edax と違い着手リスト (link) を持たない。`value` が石差の範囲外、あるいは
//! `player & opponent != 0` のレコードは黙って読み飛ばす。
//!
//! # 値の対応
//!
//! Egaroucid の `level` は Egaroucid の探索表に紐づく数字なので、
//! [`BookValue`] の深さ・選択度にそのままは移せない。ここでは
//!
//! - `level >= 空きマス数` なら終局まで読んでいるとみなす
//! - そうでなければ `level` を探索深さとみなし、選択度は既定値
//!
//! と近似する。近似なので上下界は広めに付く。取り込んだ book をこのエンジンで
//! 育て直すと、探索し直した局面から順に本来の幅に締まっていく。
//!
//! Egaroucid の `leaf` (book に無い手の中の最善) はこの book の
//! [`Frontier`] と同じ役割なのでそのまま移す。`n_lines` は保持しない
//! (子から数え直せるので、書き出すときに計算する)。
//!
//! # `.egbk2` (旧形式)
//!
//! ヘッダは `.egbk3` と同じ 14 バイト (version = 2)。局面は可変長で
//! `u64 player`, `u64 opponent`, `i8 value`, `i8 level`, `u8 n_moves`,
//! `n_moves` 個の (`i8 value`, `i8 move`)。着手リストは読み飛ばす。
//!
//! # `.egbk` (最初期形式)
//!
//! マジックが無く `i32 n_boards` から始まる。局面は
//! `u64 player`, `u64 opponent`, `u8 value_raw` の 17 バイト。
//! 評価値は `-((i8)value_raw - 64)` で復元する。

use super::layer::{ply_of, PlyLayer, N_LAYERS};
use super::sym::{
    convert_coord_from_representative, convert_coord_to_representative, representative_board,
};
use super::value::{is_valid_score, BookValue, Frontier, SELECTIVITY_EXACT};
use super::Book;
use crate::board::board::Board;
use crate::EngineError;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::num::NonZeroUsize;

/// `.egbk3` / `.egbk2` の magic。`EGAROUCID` の逆順。
pub const EGBK_MAGIC: &[u8; 9] = b"DICUORAGE";
/// このエンジンが書き出す book version。
pub const EGBK_VERSION: u8 = 3;

/// `.egbk3` のヘッダ長。
const EGBK3_HEADER_SIZE: usize = 14;
/// `.egbk3` の 1 局面のバイト数。
const EGBK3_RECORD_SIZE: usize = 25;

/// Egaroucid の level を深さに移せないときに使う選択度。
///
/// Egaroucid の既定の中盤探索はそれなりに枝刈りするので、完全 (6) より
/// 落として広めの窓を付ける。
const IMPORTED_SELECTIVITY: u8 = 3;

/// 取り込んだ 1 局面。
type Imported = (Board, BookValue, Frontier);

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

    fn i32(&mut self) -> Result<i32, EngineError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, EngineError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

/// 読み込みに使うスレッド数。
fn default_load_threads() -> NonZeroUsize {
    std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}

/// Egaroucid の `level` から [`BookValue`] を組み立てる。
fn value_from_level(score: i8, n_empties: u8, level: i8) -> BookValue {
    let level = level.max(0) as u8;
    if level >= n_empties {
        BookValue::searched(score, n_empties, n_empties, SELECTIVITY_EXACT)
    } else {
        BookValue::searched(score, n_empties, level, IMPORTED_SELECTIVITY)
    }
}

/// [`BookValue`] を Egaroucid の `level` に落とす。
fn level_from_value(value: &BookValue, n_empties: u8) -> i8 {
    if value.is_exact() {
        return n_empties.min(60) as i8;
    }
    value.depth.min(60) as i8
}

/// `.egbk3` の 1 レコード (25 バイト) をデコードする。
///
/// 正規化は 1 レコードにつき 1 回だけ行い、得られた変換インデックスで
/// leaf 座標も同時に移す。表には触らないのでスレッドから呼べる。
fn decode_egbk3_record(record: &[u8]) -> Option<Imported> {
    debug_assert_eq!(record.len(), EGBK3_RECORD_SIZE);
    let player = u64::from_le_bytes(record[0..8].try_into().unwrap());
    let opponent = u64::from_le_bytes(record[8..16].try_into().unwrap());
    let value = record[16] as i8;

    // Egaroucid と同じ検査。壊れたレコードは黙って読み飛ばす。
    if !is_valid_score(value) || player & opponent != 0 {
        return None;
    }

    let board = Board { player, opponent };
    let n_empties = board.empties_count() as u8;
    let (key, idx) = representative_board(&board);

    let leaf_value = record[22] as i8;
    let leaf_move = record[23] as i8;
    let leaf_level = record[24] as i8;
    let frontier = if (0..64).contains(&leaf_move) && is_valid_score(leaf_value) {
        Frontier {
            mv: convert_coord_to_representative(leaf_move as u8, idx) as i8,
            score: leaf_value,
            depth: leaf_level.max(0) as u8,
        }
    } else {
        Frontier::unset()
    };

    Some((
        key,
        value_from_level(value, n_empties, record[17] as i8),
        frontier,
    ))
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

/// 取り込んだ局面を ply ごとに仕分ける入れ物。
///
/// 層が違えば索引が衝突しないので、最後の組み立てを層ごとに並列化できる。
#[derive(Default)]
struct PlyBuckets {
    buckets: Vec<Vec<Imported>>,
}

impl PlyBuckets {
    fn new() -> Self {
        Self {
            buckets: (0..N_LAYERS).map(|_| Vec::new()).collect(),
        }
    }

    fn push(&mut self, entry: Imported) {
        let ply = ply_of(&entry.0);
        if ply < N_LAYERS {
            self.buckets[ply].push(entry);
        }
    }

    fn extend(&mut self, entries: impl IntoIterator<Item = Imported>) {
        for entry in entries {
            self.push(entry);
        }
    }

    /// 層ごとに並列に索引を張って book を組み立てる。
    fn into_book(self, threads: NonZeroUsize) -> Book {
        let mut layers: Vec<Option<PlyLayer>> = (0..self.buckets.len()).map(|_| None).collect();
        let buckets = self.buckets;
        let n_threads = threads.get().min(layers.len());

        std::thread::scope(|scope| {
            let chunk = layers.len().div_ceil(n_threads.max(1));
            let mut rest = layers.as_mut_slice();
            let mut entries = buckets.into_iter();
            while !rest.is_empty() {
                let take = chunk.min(rest.len());
                let (head, tail) = rest.split_at_mut(take);
                rest = tail;
                let work: Vec<Vec<Imported>> = (&mut entries).take(take).collect();
                scope.spawn(move || {
                    for (out, bucket) in head.iter_mut().zip(work) {
                        if bucket.is_empty() {
                            continue;
                        }
                        let mut layer = PlyLayer::with_capacity(bucket.len());
                        for (board, value, frontier) in bucket {
                            layer.upsert(board, value, frontier);
                        }
                        *out = Some(layer);
                    }
                });
            }
        });

        let mut book = Book::empty();
        for (ply, layer) in layers.into_iter().enumerate() {
            if let Some(layer) = layer {
                *book.table_mut().layer_mut(ply) = layer;
            }
        }
        book
    }
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
        let records = r.take(n_boards as usize * EGBK3_RECORD_SIZE)?;

        let threads = default_load_threads();
        let mut buckets = PlyBuckets::new();
        buckets.extend(decode_records(records, threads));
        Ok(buckets.into_book(threads))
    }

    /// `.egbk3` をファイルからストリームで読み込む。
    ///
    /// 数千万局面の book はファイルだけで数百 MB になるので、全体をメモリに
    /// 載せずにチャンク単位で取り込む。`.egbk3` でなければ `None` を返す。
    pub(super) fn load_egbk3_streaming(path: &str) -> Option<Result<Self, EngineError>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(err) => return Some(Err(EngineError::Io(err))),
        };
        let file_len = file.metadata().ok()?.len();
        let mut reader = BufReader::with_capacity(1 << 20, file);

        let mut header = [0u8; EGBK3_HEADER_SIZE];
        if reader.read_exact(&mut header).is_err() {
            return None;
        }
        if &header[0..9] != EGBK_MAGIC || header[9] != 3 {
            return None;
        }

        let n_boards = i32::from_le_bytes(header[10..14].try_into().unwrap());
        if n_boards < 0 {
            return Some(Err(EngineError::InvalidData(format!(
                "negative board count: {n_boards}"
            ))));
        }
        // 壊れたヘッダで巨大な確保をしないよう、ファイル長と突き合わせる。
        let available = (file_len - EGBK3_HEADER_SIZE as u64) / EGBK3_RECORD_SIZE as u64;
        if n_boards as u64 > available {
            return Some(Err(EngineError::InvalidData(format!(
                "book claims {n_boards} positions but the file only holds {available}"
            ))));
        }

        Some(Self::from_egbk3_reader(
            reader,
            n_boards as usize,
            default_load_threads(),
        ))
    }

    /// `.egbk3` をストリームから読み込む。
    ///
    /// 25 バイト固定長なので、読んだ塊をそのままスレッドに切り分けられる。
    /// デコードと正規化を並列に行い、仕分けだけを直列にする。
    fn from_egbk3_reader<R: Read>(
        mut reader: R,
        n_boards: usize,
        threads: NonZeroUsize,
    ) -> Result<Self, EngineError> {
        /// 1 回に読むレコード数。512K レコード = 12.8 MB。
        const CHUNK: usize = 1 << 19;

        let mut buckets = PlyBuckets::new();
        let mut buf = vec![0u8; CHUNK.min(n_boards.max(1)) * EGBK3_RECORD_SIZE];
        let mut remaining = n_boards;
        while remaining > 0 {
            let n = remaining.min(buf.len() / EGBK3_RECORD_SIZE);
            let bytes = &mut buf[..n * EGBK3_RECORD_SIZE];
            reader.read_exact(bytes).map_err(|_| {
                EngineError::InvalidData(format!(
                    "book claims {n_boards} positions but the file ended early"
                ))
            })?;
            buckets.extend(decode_records(bytes, threads));
            remaining -= n;
        }
        Ok(buckets.into_book(threads))
    }

    /// `.egbk2` を読み込む。着手リストは読み飛ばす。
    pub fn from_egbk2_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        let mut r = Reader::new(bytes);
        let n_boards = read_egbk_header(&mut r, 2)?;

        let mut buckets = PlyBuckets::new();
        for _ in 0..n_boards {
            let player = r.u64()?;
            let opponent = r.u64()?;
            let value = r.i8()?;
            let level = r.i8()?;
            let n_moves = r.u8()?;
            r.take(n_moves as usize * 2)?;

            if !is_valid_score(value) || player & opponent != 0 {
                continue;
            }
            let board = Board { player, opponent };
            let n_empties = board.empties_count() as u8;
            let (key, _) = representative_board(&board);
            buckets.push((
                key,
                value_from_level(value, n_empties, level),
                Frontier::unset(),
            ));
        }
        Ok(buckets.into_book(default_load_threads()))
    }

    /// マジックの無い最初期の `.egbk` を読み込む。
    pub(super) fn from_egbk1_bytes(bytes: &[u8], _version: u8) -> Result<Self, EngineError> {
        let mut r = Reader::new(bytes);
        let n_boards = r.i32()?;
        if n_boards < 0 {
            return Err(EngineError::InvalidData(format!(
                "negative board count: {n_boards}"
            )));
        }

        let mut buckets = PlyBuckets::new();
        for _ in 0..n_boards {
            let player = r.u64()?;
            let opponent = r.u64()?;
            // 0..128 で保存されているので 64 を引いて符号を反転する。
            let value = -((r.u8()? as i8).wrapping_sub(64));
            if !is_valid_score(value) || player & opponent != 0 {
                continue;
            }
            let board = Board { player, opponent };
            let n_empties = board.empties_count() as u8;
            let (key, _) = representative_board(&board);
            // 探索の素性が分からないので、いちばん緩い扱いにする。
            buckets.push((
                key,
                value_from_level(value, n_empties, 1),
                Frontier::unset(),
            ));
        }
        Ok(buckets.into_book(default_load_threads()))
    }

    /// `.egbk3` 形式でバイト列に書き出す。
    ///
    /// Egaroucid が読める形にするため、`n_lines` は子から数え直す。
    ///
    /// **値がまだ入っていない局面は書き出さない**。`.egbk3` は「値が無い」を
    /// 表せず、書いても Egaroucid 側で読み捨てられるため。落とした数は
    /// [`Book::n_undefined`] で分かる。
    pub fn to_egbk3_bytes(&self) -> Vec<u8> {
        let n_lines = self.count_lines();
        let n_written = self.len() - self.n_undefined();
        let mut out = Vec::with_capacity(EGBK3_HEADER_SIZE + n_written * EGBK3_RECORD_SIZE);
        out.extend_from_slice(EGBK_MAGIC);
        out.push(EGBK_VERSION);
        out.extend_from_slice(&(n_written as i32).to_le_bytes());

        for (id, board, value) in self.iter() {
            if !value.is_defined() {
                continue;
            }
            let n_empties = board.empties_count() as u8;
            out.extend_from_slice(&board.player.to_le_bytes());
            out.extend_from_slice(&board.opponent.to_le_bytes());
            out.push(value.score as u8);
            out.push(level_from_value(value, n_empties) as u8);
            let lines = n_lines[id.ply as usize][id.slot as usize];
            out.extend_from_slice(&lines.to_le_bytes());

            let frontier = self.table().frontier(id);
            if frontier.has_move() {
                out.push(frontier.score as u8);
                // 正規形の座標のまま書く。Egaroucid も正規形で保存する。
                out.push(convert_coord_from_representative(frontier.mv as u8, 0));
                out.push(frontier.depth.min(60));
            } else {
                out.push(super::value::SCORE_UNDEFINED as u8);
                out.push(super::value::MOVE_NONE as u8);
                out.push(0);
            }
        }
        out
    }

    /// `.egbk3` 形式で保存する。
    pub fn save_egbk3(&self, path: &str) -> Result<(), EngineError> {
        let tmp = format!("{path}.tmp");
        {
            let mut writer = BufWriter::new(File::create(&tmp)?);
            writer.write_all(&self.to_egbk3_bytes())?;
            writer.flush()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// 各局面を通る変化の本数を数える。
    ///
    /// 子は必ず 1 つ深い層にいるので、深い層から 1 回舐めれば求まる。
    /// Egaroucid の `n_lines` に相当し、この book では保持せず必要なときに
    /// 数え直す (保持すると育成のたびに張り直すことになる)。
    pub(super) fn count_lines(&self) -> Vec<Vec<u32>> {
        let max_ply = self.max_ply().unwrap_or(0);
        let mut counts: Vec<Vec<u32>> = (0..N_LAYERS)
            .map(|ply| vec![0u32; self.table().layer(ply).len()])
            .collect();

        for ply in (0..=max_ply).rev() {
            for slot in 0..self.table().layer(ply).len() {
                let board = *self.table().layer(ply).board(slot as u32);
                let children = self.children(&board);
                let mut sum = 0u32;
                for child in &children {
                    sum = sum.saturating_add(
                        counts[child.id.ply as usize][child.id.slot as usize].max(1),
                    );
                }
                counts[ply][slot] = sum;
            }
        }
        counts
    }
}

/// レコード列を並列にデコードする。
fn decode_records(records: &[u8], threads: NonZeroUsize) -> Vec<Imported> {
    let n = records.len() / EGBK3_RECORD_SIZE;
    let n_threads = threads.get().min(n.div_ceil(4096).max(1));
    if n_threads <= 1 {
        return records
            .chunks_exact(EGBK3_RECORD_SIZE)
            .filter_map(decode_egbk3_record)
            .collect();
    }

    let chunk = n.div_ceil(n_threads) * EGBK3_RECORD_SIZE;
    let mut parts: Vec<Vec<Imported>> = (0..n_threads).map(|_| Vec::new()).collect();
    std::thread::scope(|scope| {
        for (out, slice) in parts.iter_mut().zip(records.chunks(chunk)) {
            scope.spawn(move || {
                *out = slice
                    .chunks_exact(EGBK3_RECORD_SIZE)
                    .filter_map(decode_egbk3_record)
                    .collect();
            });
        }
    });
    parts.concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::value::SCORE_UNDEFINED;

    const D3: u8 = 19;

    /// Egaroucid の `save_egbk3` と同じ順序の書き出しを並べた C プログラムが
    /// 実際に出力したバイト列。ヘッダ 14 B + 局面 1 つ (25 B)。
    ///
    /// 初期盤面の正規形、value = 2, level = 21, n_lines = 7,
    /// leaf = (value 1, move 26, level 21)。
    const EGBK3_GOLDEN: [u8; 39] = [
        0x44, 0x49, 0x43, 0x55, 0x4f, 0x52, 0x41, 0x47, 0x45, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x10, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x10, 0x00, 0x00, 0x00,
        0x02, 0x15, 0x07, 0x00, 0x00, 0x00, 0x01, 0x1a, 0x15,
    ];

    // 上のバイト列は C 版と 1 バイトも違わないこと。
    const _: () = assert!(EGBK3_GOLDEN.len() == EGBK3_HEADER_SIZE + EGBK3_RECORD_SIZE);

    #[test]
    fn golden_egbk3_is_read_as_the_initial_position() {
        let book = Book::from_egbk3_bytes(&EGBK3_GOLDEN).unwrap();
        assert_eq!(book.len(), 1);
        let value = book.value_of(&Board::new()).unwrap();
        assert_eq!(value.score, 2);
        // level 21 < 空きマス 60 なので中盤探索扱い。
        assert!(!value.is_exact());
        assert_eq!(value.depth, 21);

        let frontier = book.frontier_of(&Board::new()).unwrap();
        assert!(frontier.has_move());
        assert_ne!(Board::new().moves() & (1u64 << frontier.mv), 0);
    }

    #[test]
    fn egbk3_round_trip_keeps_the_positions_and_scores() {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        book.insert(&root, BookValue::searched(2, 60, 21, 3));
        book.insert(&child, BookValue::exact(-2));
        book.set_frontier(
            &child,
            Frontier {
                mv: child.moves().trailing_zeros() as i8,
                score: -1,
                depth: 12,
            },
        );

        let loaded = Book::from_egbk3_bytes(&book.to_egbk3_bytes()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.value_of(&root).unwrap().score, 2);
        assert_eq!(loaded.value_of(&child).unwrap().score, -2);
        assert!(
            loaded.value_of(&child).unwrap().is_exact(),
            "level >= 空きマス数なら完全読みとして復元される"
        );
        let frontier = loaded.frontier_of(&child).unwrap();
        assert!(frontier.has_move());
        assert_eq!(frontier.score, -1);
    }

    #[test]
    fn a_position_without_a_frontier_writes_the_undefined_marker() {
        let mut book = Book::empty();
        book.insert(&Board::new(), BookValue::exact(0));
        let bytes = book.to_egbk3_bytes();
        assert_eq!(bytes.len(), EGBK3_HEADER_SIZE + EGBK3_RECORD_SIZE);
        assert_eq!(bytes[EGBK3_HEADER_SIZE + 22] as i8, SCORE_UNDEFINED);
        assert_eq!(bytes[EGBK3_HEADER_SIZE + 23] as i8, -1);
    }

    #[test]
    fn header_matches_egaroucid() {
        let mut book = Book::empty();
        book.insert(&Board::new(), BookValue::exact(0));
        let bytes = book.to_egbk3_bytes();
        assert_eq!(&bytes[0..9], EGBK_MAGIC);
        assert_eq!(bytes[9], EGBK_VERSION);
        assert_eq!(i32::from_le_bytes(bytes[10..14].try_into().unwrap()), 1);
    }

    #[test]
    fn n_lines_counts_the_variations_through_a_position() {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        book.insert(&root, BookValue::exact(0));
        book.insert(&child, BookValue::exact(0));

        let counts = book.count_lines();
        let root_id = book.table().locate(&root).unwrap();
        // 初期盤面の 4 手はすべて同じ子に落ちるので 4 本と数える
        // (Egaroucid も link ごとに数えるので同じ挙動)。
        assert_eq!(counts[0][root_id.slot as usize], 4);
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

        let bytes = book.to_egbk3_bytes();
        assert_eq!(bytes.len(), EGBK3_HEADER_SIZE + EGBK3_RECORD_SIZE);
        assert_eq!(
            i32::from_le_bytes(bytes[10..14].try_into().unwrap()),
            1,
            "ヘッダの局面数が実際のレコード数と一致すること"
        );
        assert_eq!(Book::from_egbk3_bytes(&bytes).unwrap().len(), 1);
    }

    #[test]
    fn corrupted_records_are_skipped() {
        let mut bytes = EGBK3_GOLDEN.to_vec();
        // value を範囲外にする。
        bytes[30] = 100;
        let book = Book::from_egbk3_bytes(&bytes).unwrap();
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn overlapping_stones_are_skipped() {
        let mut bytes = EGBK3_GOLDEN.to_vec();
        // opponent を player と同じにする。
        let (player, opponent) = bytes.split_at_mut(22);
        opponent[..8].copy_from_slice(&player[14..22]);
        let book = Book::from_egbk3_bytes(&bytes).unwrap();
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn a_bad_magic_is_rejected() {
        let mut bytes = EGBK3_GOLDEN.to_vec();
        bytes[0] = b'X';
        assert!(Book::from_egbk3_bytes(&bytes).is_err());
    }

    #[test]
    fn a_truncated_file_is_rejected() {
        assert!(Book::from_egbk3_bytes(&EGBK3_GOLDEN[..30]).is_err());
    }

    #[test]
    fn parallel_and_serial_decodes_agree() {
        let mut book = Book::empty();
        let mut board = Board::new();
        for _ in 0..20 {
            if board.moves() == 0 {
                break;
            }
            book.insert(&board, BookValue::searched(1, 60, 10, 3));
            board = board.make_move(1u64 << board.moves().trailing_zeros());
        }
        let bytes = book.to_egbk3_bytes();
        let records = &bytes[EGBK3_HEADER_SIZE..];

        let serial = decode_records(records, NonZeroUsize::MIN);
        let mut parallel = decode_records(records, NonZeroUsize::new(4).unwrap());
        assert_eq!(serial.len(), parallel.len());
        parallel.sort_by_key(|(b, _, _)| (b.player, b.opponent));
        let mut serial = serial;
        serial.sort_by_key(|(b, _, _)| (b.player, b.opponent));
        assert_eq!(serial.len(), parallel.len());
        for (a, b) in serial.iter().zip(&parallel) {
            assert_eq!(a.0, b.0);
            assert_eq!(a.1, b.1);
        }
    }

    #[test]
    fn duplicate_records_keep_the_more_trustworthy_value() {
        let mut bytes = EGBK3_GOLDEN.to_vec();
        let mut second = EGBK3_GOLDEN[EGBK3_HEADER_SIZE..].to_vec();
        second[16] = 9i8 as u8; // value
        second[17] = 60; // level (= 完全読み扱い)
        bytes.extend_from_slice(&second);
        bytes[10..14].copy_from_slice(&2i32.to_le_bytes());

        let book = Book::from_egbk3_bytes(&bytes).unwrap();
        assert_eq!(book.len(), 1);
        let value = book.value_of(&Board::new()).unwrap();
        assert_eq!(value.score, 9);
        assert!(value.is_exact());
    }

    #[test]
    fn egbk1_scores_are_decoded_with_the_offset() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&Board::new().player.to_le_bytes());
        bytes.extend_from_slice(&Board::new().opponent.to_le_bytes());
        bytes.push(60); // -(60 - 64) = 4
        let book = Book::from_bytes(&bytes).unwrap();
        assert_eq!(book.value_of(&Board::new()).unwrap().score, 4);
    }

    #[test]
    fn egbk2_skips_the_move_list() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(EGBK_MAGIC);
        bytes.push(2);
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&Board::new().player.to_le_bytes());
        bytes.extend_from_slice(&Board::new().opponent.to_le_bytes());
        bytes.push(3i8 as u8); // value
        bytes.push(21); // level
        bytes.push(2); // n_moves
        bytes.extend_from_slice(&[3, 19, 4, 26]);

        let book = Book::from_bytes(&bytes).unwrap();
        assert_eq!(book.len(), 1);
        assert_eq!(book.value_of(&Board::new()).unwrap().score, 3);
    }
}
