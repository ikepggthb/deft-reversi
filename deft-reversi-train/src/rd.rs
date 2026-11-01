//! The `.rd` dataset: `phase_<stones>/<split>.rd` files of positions labelled
//! with the final disc difference.
//!
//! Each file is `RDGBBVAL1\n` followed by 18-byte records: the mover's discs
//! (u64 LE), the opponent's discs (u64 LE) and the label (i16 LE), all from
//! the side to move.
use deft_reversi_engine::Board;
use std::{
    fs::File,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
};

pub const MAGIC: &[u8] = b"RDGBBVAL1\n";
const RECORD_SIZE: usize = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub board: Board,
    /// Final disc difference for the side to move.
    pub value: i8,
}

/// The file holding `split` ("train", "valid" or "test") for positions with
/// `stones` discs.
pub fn path(data: &Path, stones: u8, split: &str) -> PathBuf {
    data.join(format!("phase_{stones}"))
        .join(format!("{split}.rd"))
}

/// Number of records in `path`, from its size.
pub fn count(path: &Path) -> io::Result<u64> {
    let size = path.metadata()?.len();
    let payload = size
        .checked_sub(MAGIC.len() as u64)
        .filter(|n| n % RECORD_SIZE as u64 == 0)
        .ok_or_else(|| invalid(path, "truncated file"))?;
    Ok(payload / RECORD_SIZE as u64)
}

/// Identifies the contents of `path` without reading all of it: the record
/// count and a hash of the first and last 4096 records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    records: u64,
    hash: u64,
}

pub fn fingerprint(path: &Path) -> io::Result<Fingerprint> {
    use std::io::{Seek, SeekFrom};
    let records = count(path)?;
    let mut file = File::open(path)?;
    let edge = records.min(4096);
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

/// Reads every record of `path`, checking that each has `stones` discs.
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
