//! reversi-dataset-gen（rdg）形式の `.rd` データセットを読み書きし、内容確認用のフィンガープリントを計算するモジュール。
//!
//! `.rd` データセットは、盤面と「その局面から最善を尽くしたときの最終石差」をペアにした学習用バイナリ形式である。
//! ディレクトリ構成は `phase_<NN>/<split>.rd` となっており、フェーズ番号 `NN` は「石数 − 4」の 2 桁の数値（例：12 石なら `phase_08`）を表す。
//! 各ファイルは先頭 10 バイトの識別子 `RDGBBVAL1\n` に続いて、18 バイトのレコードが連続して並ぶ：
//! - 手番側の石ビットボード（`u64` リトルエンディアン）
//! - 相手側の石ビットボード（`u64` リトルエンディアン）
//! - 手番側から見た最終石差ラベル（`i16` リトルエンディアン）
use deft_reversi_engine::Board;
use std::{
    fs::File,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
};

/// `.rd` ファイル先頭の識別バイト列（マジックナンバー）。
pub const MAGIC: &[u8] = b"RDGBBVAL1\n";
/// 1 レコードのバイト長（8 + 8 + 2 = 18 バイト）。
const RECORD_SIZE: usize = 18;

/// `.rd` ファイルから読み込んだ 1 局面分の学習レコード。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// 手番側視点の盤面。
    pub board: Board,
    /// 手番側から見た最終石差（`-64..=64`）。
    pub value: i8,
}

/// 指定した石数 `stones` と分割名 `split`（`"train"` / `"valid"` / `"test"`）に対応する `.rd` ファイルのパスを返す。
pub fn path(data: &Path, stones: u8, split: &str) -> PathBuf {
    data.join(format!("phase_{:02}", stones - 4))
        .join(format!("{split}.rd"))
}

/// ファイルサイズから `path` 内に含まれるレコード数を計算する。
pub fn count(path: &Path) -> io::Result<u64> {
    let size = path.metadata()?.len();
    let payload = size
        .checked_sub(MAGIC.len() as u64)
        .filter(|n| n % RECORD_SIZE as u64 == 0)
        .ok_or_else(|| invalid(path, "truncated file"))?;
    Ok(payload / RECORD_SIZE as u64)
}

/// 巨大なデータセット全体を読み込まずに `path` の内容が同じかを識別するためのフィンガープリント。
/// レコード数と、先頭・末尾それぞれ最大 4096 レコードの FNV-1a ハッシュ値を保持する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    records: u64,
    hash: u64,
}

/// チェックポイント再開時に学習データが差し替わっていないか検証するため、`path` の [`Fingerprint`] を計算する。
pub fn fingerprint(path: &Path) -> io::Result<Fingerprint> {
    use std::io::{Seek, SeekFrom};
    let records = count(path)?;
    let mut file = File::open(path)?;
    let edge = records.min(4096);
    // FNV-1a 64-bit オフセット基底値
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for start in [0, records - edge] {
        file.seek(SeekFrom::Start(
            MAGIC.len() as u64 + start * RECORD_SIZE as u64,
        ))?;
        let mut bytes = vec![0; edge as usize * RECORD_SIZE];
        file.read_exact(&mut bytes)?;
        for b in bytes {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    }
    Ok(Fingerprint { records, hash })
}

/// `path` の全レコードを読み込み、各盤面が `stones` 個の石を持ちラベルが `-64..=64` に収まることを検証する。
pub fn read(path: &Path, stones: u8) -> io::Result<Vec<Record>> {
    let n = count(path)?;
    let mut input = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut magic = [0; MAGIC.len()];
    input.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(invalid(path, "not an RDGBBVAL1 file"));
    }
    let mut records = Vec::with_capacity(n as usize);
    let mut bytes = [0; RECORD_SIZE];
    for i in 0..n {
        input.read_exact(&mut bytes)?;
        let player = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let opponent = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let value = i16::from_le_bytes([bytes[16], bytes[17]]);
        let board = Board::from_bits(player, opponent)
            .filter(|b| 64 - b.empty_count() == stones)
            .ok_or_else(|| invalid(path, format!("record {i}: not a {stones}-disc board")))?;
        let value = i8::try_from(value)
            .ok()
            .filter(|v| (-64..=64).contains(v))
            .ok_or_else(|| invalid(path, format!("record {i}: label {value}")))?;
        records.push(Record { board, value });
    }
    Ok(records)
}

fn invalid(path: &Path, what: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}: {what}", path.display()),
    )
}

#[cfg(test)]
pub fn write(path: &Path, records: &[Record]) {
    let mut bytes = MAGIC.to_vec();
    for r in records {
        bytes.extend_from_slice(&r.board.player_bits().to_le_bytes());
        bytes.extend_from_slice(&r.board.opponent_bits().to_le_bytes());
        bytes.extend_from_slice(&i16::from(r.value).to_le_bytes());
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_bad_files_are_rejected() {
        let dir = crate::test_dir("rd");
        let board = Board::new();
        let records = [Record { board, value: -12 }, Record { board, value: 64 }];
        let file = path(&dir, 4, "train");
        write(&file, &records);
        assert_eq!(count(&file).unwrap(), 2);
        assert_eq!(read(&file, 4).unwrap(), records);
        let print = fingerprint(&file).unwrap();
        write(&file, &[records[1], records[0]]);
        assert_ne!(fingerprint(&file).unwrap(), print);
        assert!(read(&file, 5).is_err());

        let mut bytes = std::fs::read(&file).unwrap();
        bytes.pop();
        std::fs::write(&file, &bytes).unwrap();
        assert!(count(&file).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
