//! 標準の book ファイル形式 `.dbk` (deft book)。
//!
//! すべて little endian。
//!
//! # ヘッダ (32 バイト)
//!
//! | offset | size | 内容 |
//! |--------|------|------|
//! | 0  | 8 | magic `"DEFTBOOK"` |
//! | 8  | 2 | `u16` version (= 1) |
//! | 10 | 2 | `u16` flags (bit 0: frontier 列を含む) |
//! | 12 | 4 | `u32` 局面数 |
//! | 16 | 1 | `u8` 育成レベル |
//! | 17 | 1 | `u8` 育成の最大 ply |
//! | 18 | 1 | `u8` 自分の手で許した損 |
//! | 19 | 1 | `u8` 相手の手で許した損 |
//! | 20 | 4 | `u32` 層の数 |
//! | 24 | 8 | `u64` 本体のハッシュ (0 なら未計算) |
//!
//! # 層 (ヘッダの直後に「層の数」だけ並ぶ)
//!
//! | size | 内容 |
//! |------|------|
//! | 1 | `u8` ply |
//! | 4 | `u32` この層の局面数 `n` |
//! | 8n | `u64 player` の列 |
//! | 8n | `u64 opponent` の列 |
//! | n | `i8 score` の列 |
//! | n | `i8 lower` の列 |
//! | n | `i8 upper` の列 |
//! | n | `u8 depth` の列 |
//! | n | `u8 selectivity` の列 |
//! | n | `u8 flags` の列 |
//! | 3n | frontier の列 (`i8 move`, `i8 score`, `u8 depth`)。flags bit 0 のときだけ |
//!
//! # 末尾 (定石名)
//!
//! | size | 内容 |
//! |------|------|
//! | 4 | `u32` 定石数 |
//! | .. | 定石ごとに `u16` 名前長 + 名前 + `u16` 手順長 + 手順 (どちらも UTF-8) |
//! | 2 + .. | `u16` 備考長 + 備考 |
//!
//! # なぜこの形にしたか
//!
//! Egaroucid の `.egbk3` は 1 局面 25 バイトのレコードが並ぶ行指向。
//! ここは **列指向**にしてある。
//!
//! - 同じ列は値の分布が揃うので、`zstd` などに通したときによく縮む
//!   (行指向だと u64 の盤面と 1 バイトの値が交互に来て統計が混ざる)
//! - 局面数だけ数えたい、値の分布を見たい、といったときに必要な列だけ読める
//! - frontier 列は配布用の book では丸ごと省ける (12% 削れる)
//!
//! 層ごとに区切ってあるので、読み込みは **層単位でそのまま並列化できる**。
//! 索引 (ハッシュ表) の構築が読み込みの大半を占めるが、層が違えば衝突しない。
//!
//! ハッシュはファイルの破損を検出するためのもので、暗号学的な強度は無い。

use super::layer::{PlyLayer, N_LAYERS};
use super::names::NameTable;
use super::value::{BookValue, Frontier, ValueFlags};
use super::{Book, BookMeta};
use crate::board::board::Board;
use crate::EngineError;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::num::NonZeroUsize;

/// `.dbk` の magic。
pub const DBK_MAGIC: &[u8; 8] = b"DEFTBOOK";
/// このエンジンが書き出す `.dbk` のバージョン。
pub const DBK_VERSION: u16 = 1;
/// ヘッダ長。
pub const DBK_HEADER_SIZE: usize = 32;

/// frontier の列を含む。
pub const DBK_FLAG_FRONTIERS: u16 = 1 << 0;

/// 1 局面あたりのバイト数 (frontier 込み)。
const BYTES_PER_POSITION: usize = 16 + 6 + 3;

/// 局面数の上限。壊れたファイルで巨大な確保をしないための歯止め。
const MAX_POSITIONS: u32 = 1 << 30;

/// 破損検出用のハッシュ (FNV-1a 64)。
#[derive(Clone, Copy)]
struct BodyHash(u64);

impl BodyHash {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = h;
    }

    fn finish(self) -> u64 {
        self.0
    }
}

/// 本体を書き出しながらハッシュを取るライタ。
struct HashingWriter<W: Write> {
    inner: W,
    hash: BodyHash,
}

impl<W: Write> HashingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hash: BodyHash::new(),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.hash.write(bytes);
        self.inner.write_all(bytes)
    }
}

/// バイト列から固定長を読み進めるカーソル。
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], EngineError> {
        if self.bytes.len() - self.pos < n {
            return Err(EngineError::InvalidData(
                "dbk: file ended in the middle of a record".to_string(),
            ));
        }
        let out = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, EngineError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, EngineError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, EngineError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// 読み込んだヘッダ。
#[derive(Clone, Debug)]
struct Header {
    flags: u16,
    n_positions: u32,
    n_layers: u32,
    body_hash: u64,
    meta: BookMeta,
}

fn parse_header(bytes: &[u8]) -> Result<Header, EngineError> {
    if bytes.len() < DBK_HEADER_SIZE {
        return Err(EngineError::InvalidData(
            "dbk: header too short".to_string(),
        ));
    }
    if &bytes[0..8] != DBK_MAGIC {
        return Err(EngineError::InvalidData("dbk: bad magic".to_string()));
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    if version != DBK_VERSION {
        return Err(EngineError::InvalidData(format!(
            "dbk: unsupported version {version} (this build reads {DBK_VERSION})"
        )));
    }
    let n_positions = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    if n_positions > MAX_POSITIONS {
        return Err(EngineError::InvalidData(format!(
            "dbk: implausible position count {n_positions}"
        )));
    }
    let n_layers = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    if n_layers as usize > N_LAYERS {
        return Err(EngineError::InvalidData(format!(
            "dbk: too many layers ({n_layers})"
        )));
    }
    Ok(Header {
        flags: u16::from_le_bytes(bytes[10..12].try_into().unwrap()),
        n_positions,
        n_layers,
        body_hash: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        meta: BookMeta {
            level: bytes[16],
            max_ply: bytes[17],
            player_error: bytes[18],
            opponent_error: bytes[19],
            note: String::new(),
        },
    })
}

/// 層 1 つ分を読む。読み終えた位置は `cursor` が進む。
fn read_layer(
    cursor: &mut Cursor<'_>,
    with_frontiers: bool,
) -> Result<(usize, PlyLayer), EngineError> {
    let ply = cursor.u8()? as usize;
    if ply >= N_LAYERS {
        return Err(EngineError::InvalidData(format!(
            "dbk: ply {ply} out of range"
        )));
    }
    let n = cursor.u32()? as usize;
    if n > MAX_POSITIONS as usize {
        return Err(EngineError::InvalidData(format!(
            "dbk: implausible layer size {n}"
        )));
    }

    let players = cursor.take(n * 8)?;
    let opponents = cursor.take(n * 8)?;
    let scores = cursor.take(n)?;
    let lowers = cursor.take(n)?;
    let uppers = cursor.take(n)?;
    let depths = cursor.take(n)?;
    let selectivities = cursor.take(n)?;
    let vflags = cursor.take(n)?;
    let frontiers_raw = if with_frontiers {
        Some((cursor.take(n)?, cursor.take(n)?, cursor.take(n)?))
    } else {
        None
    };

    let mut boards = Vec::with_capacity(n);
    let mut values = Vec::with_capacity(n);
    let mut frontiers = Vec::with_capacity(n);
    for i in 0..n {
        let player = u64::from_le_bytes(players[i * 8..i * 8 + 8].try_into().unwrap());
        let opponent = u64::from_le_bytes(opponents[i * 8..i * 8 + 8].try_into().unwrap());
        // 壊れたレコードは黙って捨てる。読み込み全体を落とすより実用的。
        if player & opponent != 0 {
            continue;
        }
        if (player | opponent).count_ones() as usize != ply + 4 {
            continue;
        }
        boards.push(Board { player, opponent });
        values.push(BookValue {
            score: scores[i] as i8,
            lower: lowers[i] as i8,
            upper: uppers[i] as i8,
            depth: depths[i],
            selectivity: selectivities[i],
            flags: ValueFlags::from_bits(vflags[i]),
        });
        frontiers.push(match &frontiers_raw {
            Some((mv, score, depth)) => Frontier {
                mv: mv[i] as i8,
                score: score[i] as i8,
                depth: depth[i],
            },
            None => Frontier::unset(),
        });
    }

    Ok((ply, PlyLayer::from_columns(boards, values, frontiers)))
}

/// 末尾の定石名と備考を読む。
fn read_trailer(cursor: &mut Cursor<'_>) -> Result<(NameTable, String), EngineError> {
    let mut names = NameTable::default();
    let n_openings = cursor.u32()?;
    for _ in 0..n_openings {
        let name_len = cursor.u16()? as usize;
        let name = std::str::from_utf8(cursor.take(name_len)?)
            .map_err(|_| EngineError::InvalidData("dbk: opening name is not UTF-8".to_string()))?
            .to_string();
        let record_len = cursor.u16()? as usize;
        let record = std::str::from_utf8(cursor.take(record_len)?)
            .map_err(|_| EngineError::InvalidData("dbk: opening record is not UTF-8".to_string()))?
            .to_string();
        // 手順が壊れている定石は捨てる。名前が引けなくなるだけで実害が無い。
        let _ = names.insert(&name, &record);
    }
    let note_len = cursor.u16()? as usize;
    let note = std::str::from_utf8(cursor.take(note_len)?)
        .map_err(|_| EngineError::InvalidData("dbk: note is not UTF-8".to_string()))?
        .to_string();
    Ok((names, note))
}

impl Book {
    /// `.dbk` のバイト列から読み込む。
    pub fn from_dbk_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        Self::from_dbk_bytes_with_threads(bytes, default_load_threads())
    }

    /// スレッド数を指定して `.dbk` のバイト列から読み込む。
    ///
    /// 層は互いに独立なので、索引の構築だけを層ごとに割り振る。
    pub fn from_dbk_bytes_with_threads(
        bytes: &[u8],
        threads: NonZeroUsize,
    ) -> Result<Self, EngineError> {
        let header = parse_header(bytes)?;
        let body = &bytes[DBK_HEADER_SIZE..];
        verify_body_hash(body, header.body_hash)?;

        let with_frontiers = header.flags & DBK_FLAG_FRONTIERS != 0;
        let mut cursor = Cursor::new(body);

        // まず層の範囲だけを切り出す (ここは直列だが、なめるだけなので速い)。
        let mut slices: Vec<&[u8]> = Vec::with_capacity(header.n_layers as usize);
        for _ in 0..header.n_layers {
            let start = cursor.pos;
            skip_layer(&mut cursor, with_frontiers)?;
            slices.push(&body[start..cursor.pos]);
        }
        let (names, note) = read_trailer(&mut cursor)?;

        // 層ごとに並列に組み立てる。
        let mut layers: Vec<Option<(usize, PlyLayer)>> = (0..slices.len()).map(|_| None).collect();
        let n_threads = threads.get().min(slices.len().max(1));
        if n_threads <= 1 {
            for (out, slice) in layers.iter_mut().zip(&slices) {
                *out = Some(read_layer(&mut Cursor::new(slice), with_frontiers)?);
            }
        } else {
            let mut errors: Vec<Option<EngineError>> = (0..slices.len()).map(|_| None).collect();
            let chunk = layers.len().div_ceil(n_threads);
            std::thread::scope(|scope| {
                let slices = &slices;
                for (n, (layer_chunk, error_chunk)) in layers
                    .chunks_mut(chunk)
                    .zip(errors.chunks_mut(chunk))
                    .enumerate()
                {
                    let start = n * chunk;
                    scope.spawn(move || {
                        for (i, out) in layer_chunk.iter_mut().enumerate() {
                            match read_layer(&mut Cursor::new(slices[start + i]), with_frontiers) {
                                Ok(layer) => *out = Some(layer),
                                Err(err) => error_chunk[i] = Some(err),
                            }
                        }
                    });
                }
            });
            if let Some(err) = errors.into_iter().flatten().next() {
                return Err(err);
            }
        }

        let mut book = Book::empty();
        for (ply, layer) in layers.into_iter().flatten() {
            *book.table_mut().layer_mut(ply) = layer;
        }
        let mut meta = header.meta;
        meta.note = note;
        *book.meta_mut() = meta;
        book.set_names(names);
        Ok(book)
    }

    /// `.dbk` ファイルをストリームで読み込む。`path` が `.dbk` でなければ `None`。
    pub(super) fn load_dbk_streaming(path: &str) -> Option<Result<Self, EngineError>> {
        if !path.to_ascii_lowercase().ends_with(".dbk") {
            return None;
        }
        Some(Self::read_dbk_file(path))
    }

    fn read_dbk_file(path: &str) -> Result<Self, EngineError> {
        let mut reader = BufReader::new(File::open(path)?);
        let mut head = [0u8; DBK_HEADER_SIZE];
        reader.read_exact(&mut head)?;
        let header = parse_header(&head)?;

        // 局面数から本体の見当をつけて 1 回だけ確保する。
        let hint = header.n_positions as usize * BYTES_PER_POSITION;
        let mut body = Vec::with_capacity(hint + 4096);
        reader.read_to_end(&mut body)?;

        let mut bytes = Vec::with_capacity(DBK_HEADER_SIZE + body.len());
        bytes.extend_from_slice(&head);
        bytes.extend_from_slice(&body);
        Self::from_dbk_bytes(&bytes)
    }

    /// `.dbk` 形式でバイト列に書き出す。
    pub fn to_dbk_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DBK_HEADER_SIZE + self.len() * BYTES_PER_POSITION + 256);
        // 失敗しない書き込み先なので unwrap で問題ない。
        self.write_dbk(&mut out, true).unwrap();
        out
    }

    /// `.dbk` 形式で保存する。
    ///
    /// 一時ファイルに書いてから置き換えるので、途中で落ちても元の book は残る。
    pub fn save_dbk(&self, path: &str) -> Result<(), EngineError> {
        self.save_dbk_with(path, true)
    }

    /// frontier 列を書くかどうかを指定して保存する。
    ///
    /// 配布用の book なら `false` にすると 12% ほど小さくなる (育成は
    /// frontier をもう一度探索し直すことになる)。
    pub fn save_dbk_with(&self, path: &str, with_frontiers: bool) -> Result<(), EngineError> {
        let tmp = format!("{path}.tmp");
        {
            let mut writer = BufWriter::new(File::create(&tmp)?);
            self.write_dbk(&mut writer, with_frontiers)?;
            writer.flush()?;
            writer
                .into_inner()
                .map_err(|e| e.into_error())?
                .sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// `.dbk` 形式で書き出す。
    ///
    /// 本体を組み立ててからヘッダのハッシュを埋めるため、本体は一度
    /// メモリに載せる。局面数の見当がついているので確保は 1 回で済む。
    fn write_dbk<W: Write>(&self, out: &mut W, with_frontiers: bool) -> std::io::Result<()> {
        let table = self.table();
        let max_ply = table.max_ply().unwrap_or(0);
        let plies: Vec<usize> = (0..=max_ply)
            .filter(|&p| !table.layer(p).is_empty())
            .collect();

        let mut body =
            HashingWriter::new(Vec::with_capacity(self.len() * BYTES_PER_POSITION + 256));
        for &ply in &plies {
            let layer = table.layer(ply);
            let n = layer.len();
            body.write_all(&[ply as u8])?;
            body.write_all(&(n as u32).to_le_bytes())?;

            // 列ごとにまとめて書く。
            let mut column = Vec::with_capacity(n * 8);
            for board in layer.boards() {
                column.extend_from_slice(&board.player.to_le_bytes());
            }
            body.write_all(&column)?;
            column.clear();
            for board in layer.boards() {
                column.extend_from_slice(&board.opponent.to_le_bytes());
            }
            body.write_all(&column)?;

            let values = layer.values();
            write_column(&mut body, n, |i| values[i].score as u8)?;
            write_column(&mut body, n, |i| values[i].lower as u8)?;
            write_column(&mut body, n, |i| values[i].upper as u8)?;
            write_column(&mut body, n, |i| values[i].depth)?;
            write_column(&mut body, n, |i| values[i].selectivity)?;
            write_column(&mut body, n, |i| values[i].flags.bits())?;

            if with_frontiers {
                let frontiers = layer.frontiers();
                write_column(&mut body, n, |i| frontiers[i].mv as u8)?;
                write_column(&mut body, n, |i| frontiers[i].score as u8)?;
                write_column(&mut body, n, |i| frontiers[i].depth)?;
            }
        }

        // 定石名と備考。
        let openings = self.names().openings();
        body.write_all(&(openings.len() as u32).to_le_bytes())?;
        for opening in openings {
            body.write_all(&(opening.name.len() as u16).to_le_bytes())?;
            body.write_all(opening.name.as_bytes())?;
            body.write_all(&(opening.record.len() as u16).to_le_bytes())?;
            body.write_all(opening.record.as_bytes())?;
        }
        let note = self.meta().note.as_bytes();
        let note = &note[..note.len().min(u16::MAX as usize)];
        body.write_all(&(note.len() as u16).to_le_bytes())?;
        body.write_all(note)?;

        let hash = body.hash.finish();
        let body = body.inner;

        let meta = self.meta();
        let mut header = [0u8; DBK_HEADER_SIZE];
        header[0..8].copy_from_slice(DBK_MAGIC);
        header[8..10].copy_from_slice(&DBK_VERSION.to_le_bytes());
        let flags = if with_frontiers {
            DBK_FLAG_FRONTIERS
        } else {
            0
        };
        header[10..12].copy_from_slice(&flags.to_le_bytes());
        header[12..16].copy_from_slice(&(self.len() as u32).to_le_bytes());
        header[16] = meta.level;
        header[17] = meta.max_ply;
        header[18] = meta.player_error;
        header[19] = meta.opponent_error;
        header[20..24].copy_from_slice(&(plies.len() as u32).to_le_bytes());
        header[24..32].copy_from_slice(&hash.to_le_bytes());

        out.write_all(&header)?;
        out.write_all(&body)
    }
}

fn write_column<W: Write>(
    out: &mut HashingWriter<W>,
    n: usize,
    mut f: impl FnMut(usize) -> u8,
) -> std::io::Result<()> {
    let mut column = Vec::with_capacity(n);
    for i in 0..n {
        column.push(f(i));
    }
    out.write_all(&column)
}

fn verify_body_hash(body: &[u8], expected: u64) -> Result<(), EngineError> {
    if expected == 0 {
        return Ok(());
    }
    let mut hash = BodyHash::new();
    hash.write(body);
    if hash.finish() != expected {
        return Err(EngineError::InvalidData(
            "dbk: checksum mismatch (the file is corrupted)".to_string(),
        ));
    }
    Ok(())
}

/// 層 1 つ分を読み飛ばす。
fn skip_layer(cursor: &mut Cursor<'_>, with_frontiers: bool) -> Result<(), EngineError> {
    cursor.u8()?;
    let n = cursor.u32()? as usize;
    if n > MAX_POSITIONS as usize {
        return Err(EngineError::InvalidData(format!(
            "dbk: implausible layer size {n}"
        )));
    }
    let per = if with_frontiers { 16 + 6 + 3 } else { 16 + 6 };
    cursor.take(n * per)?;
    Ok(())
}

/// 既定の読み込みスレッド数。
fn default_load_threads() -> NonZeroUsize {
    std::thread::available_parallelism()
        .unwrap_or(NonZeroUsize::MIN)
        .min(NonZeroUsize::new(8).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::value::SELECTIVITY_EXACT;

    const D3: u8 = 19;

    fn sample_book() -> Book {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        let grandchild = child.make_move(1u64 << child.moves().trailing_zeros());

        book.insert(&root, BookValue::searched(2, 60, 20, 3));
        book.insert(&child, BookValue::exact(-2));
        book.insert(&grandchild, BookValue::searched(3, 58, 12, 4));
        book.set_frontier(
            &child,
            Frontier {
                mv: child.moves().trailing_zeros() as i8,
                score: -1,
                depth: 12,
            },
        );
        book.names_mut().insert("虎", "F5D6C3D3C4").unwrap();
        book.meta_mut().level = 21;
        book.meta_mut().max_ply = 30;
        book.meta_mut().note = "テスト用".to_string();
        book
    }

    #[test]
    fn round_trip_preserves_everything() {
        let book = sample_book();
        let bytes = book.to_dbk_bytes();
        let loaded = Book::from_dbk_bytes(&bytes).unwrap();

        assert_eq!(loaded.len(), book.len());
        assert_eq!(loaded.meta(), book.meta());
        assert_eq!(loaded.names().openings(), book.names().openings());
        for (id, board, value) in book.iter() {
            let got = loaded.table().locate(board).unwrap();
            assert_eq!(got, id);
            assert_eq!(loaded.table().value(got), value);
            assert_eq!(loaded.table().frontier(got), book.table().frontier(id));
        }
    }

    #[test]
    fn round_trip_keeps_the_bounds_and_flags() {
        let mut book = Book::empty();
        let board = Board::new();
        let value = BookValue::from_search(4, 40, 12, 3, 5);
        book.insert(&board, value);

        let loaded = Book::from_dbk_bytes(&book.to_dbk_bytes()).unwrap();
        assert_eq!(loaded.value_of(&board).unwrap(), value);
    }

    #[test]
    fn header_is_what_the_documentation_says() {
        let book = sample_book();
        let bytes = book.to_dbk_bytes();

        assert_eq!(&bytes[0..8], DBK_MAGIC);
        assert_eq!(u16::from_le_bytes(bytes[8..10].try_into().unwrap()), 1);
        assert_eq!(
            u16::from_le_bytes(bytes[10..12].try_into().unwrap()),
            DBK_FLAG_FRONTIERS
        );
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 3);
        assert_eq!(bytes[16], 21);
        assert_eq!(bytes[17], 30);
        // 3 局面が ply 0/1/2 に 1 つずつ。
        assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), 3);
        assert_ne!(u64::from_le_bytes(bytes[24..32].try_into().unwrap()), 0);
    }

    #[test]
    fn frontier_columns_can_be_left_out() {
        let book = sample_book();
        let mut full = Vec::new();
        book.write_dbk(&mut full, true).unwrap();
        let mut slim = Vec::new();
        book.write_dbk(&mut slim, false).unwrap();

        assert_eq!(full.len() - slim.len(), 3 * book.len());
        let loaded = Book::from_dbk_bytes(&slim).unwrap();
        assert_eq!(loaded.len(), book.len());
        // 値は残り、frontier だけが未設定に戻る。
        let child = Board::new().make_move(1u64 << D3);
        assert_eq!(loaded.value_of(&child).unwrap().score, -2);
        assert!(loaded.frontier_of(&child).unwrap().is_unset());
    }

    #[test]
    fn a_corrupted_body_is_rejected() {
        let book = sample_book();
        let mut bytes = book.to_dbk_bytes();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let err = Book::from_dbk_bytes(&bytes).unwrap_err();
        assert!(format!("{err}").contains("checksum"), "{err}");
    }

    #[test]
    fn a_bad_magic_is_rejected() {
        let mut bytes = sample_book().to_dbk_bytes();
        bytes[0] = b'X';
        assert!(Book::from_dbk_bytes(&bytes).is_err());
    }

    #[test]
    fn a_future_version_is_rejected_with_a_clear_message() {
        let mut bytes = sample_book().to_dbk_bytes();
        bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
        let err = Book::from_dbk_bytes(&bytes).unwrap_err();
        assert!(format!("{err}").contains("version"), "{err}");
    }

    #[test]
    fn a_truncated_file_is_rejected() {
        let bytes = sample_book().to_dbk_bytes();
        assert!(Book::from_dbk_bytes(&bytes[..bytes.len() - 10]).is_err());
    }

    #[test]
    fn records_whose_board_does_not_match_the_layer_are_dropped() {
        let mut book = Book::empty();
        book.insert(&Board::new(), BookValue::exact(0));
        let mut bytes = book.to_dbk_bytes();
        // ply 0 の層に石数が合わない盤面を仕込む (player を 0 にする)。
        let player_at = DBK_HEADER_SIZE + 5;
        bytes[player_at..player_at + 8].copy_from_slice(&0u64.to_le_bytes());
        // ハッシュを無効にして検査を通す。
        bytes[24..32].copy_from_slice(&0u64.to_le_bytes());

        let loaded = Book::from_dbk_bytes(&bytes).unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn parallel_and_serial_loads_agree() {
        let mut book = Book::empty();
        let mut board = Board::new();
        for _ in 0..12 {
            if board.moves() == 0 {
                break;
            }
            book.insert(&board, BookValue::searched(1, 60, 10, 3));
            board = board.make_move(1u64 << board.moves().trailing_zeros());
        }
        let bytes = book.to_dbk_bytes();

        let serial = Book::from_dbk_bytes_with_threads(&bytes, NonZeroUsize::MIN).unwrap();
        let parallel =
            Book::from_dbk_bytes_with_threads(&bytes, NonZeroUsize::new(4).unwrap()).unwrap();
        assert_eq!(serial.len(), parallel.len());
        for (id, b, v) in serial.iter() {
            let got = parallel.table().locate(b).unwrap();
            assert_eq!(got, id);
            assert_eq!(parallel.table().value(got), v);
        }
    }

    #[test]
    fn save_replaces_the_file_atomically() {
        let dir = std::env::temp_dir().join(format!("dbk-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.dbk");
        let path = path.to_str().unwrap();

        sample_book().save_dbk(path).unwrap();
        let loaded = Book::load(path).unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.names().len(), 1);
        // 一時ファイルが残っていないこと。
        assert!(!std::path::Path::new(&format!("{path}.tmp")).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_exact_value_survives_the_round_trip_as_exact() {
        let mut book = Book::empty();
        book.insert(&Board::new(), BookValue::exact(-8));
        let loaded = Book::from_dbk_bytes(&book.to_dbk_bytes()).unwrap();
        let value = loaded.value_of(&Board::new()).unwrap();
        assert!(value.is_exact());
        assert_eq!(value.selectivity, SELECTIVITY_EXACT);
        assert_eq!(value.uncertainty(), 0);
    }
}
