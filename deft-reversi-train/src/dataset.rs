use deft_reversi_engine::train_api::N_PHASES;
use deft_reversi_engine::Board;
use rand::seq::SliceRandom;
use rand::Rng;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub const RD_MAGIC: &[u8] = b"RDGBBVAL1\n";
pub const RD_RECORD_SIZE: usize = 18;

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub board: Board,
    pub target: f32,
    pub phase: usize,
}

pub struct RdReader {
    reader: BufReader<File>,
    remaining: u64,
}

impl RdReader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        if size < RD_MAGIC.len() as u64 {
            return Err(invalid_data(format!(
                "{}: missing rd header",
                path.display()
            )));
        }
        let payload = size - RD_MAGIC.len() as u64;
        if payload % RD_RECORD_SIZE as u64 != 0 {
            return Err(invalid_data(format!("{}: invalid rd size", path.display())));
        }

        let mut reader = BufReader::new(file);
        let mut header = [0u8; RD_MAGIC.len()];
        reader.read_exact(&mut header)?;
        if header != RD_MAGIC {
            return Err(invalid_data(format!(
                "{}: invalid rd header",
                path.display()
            )));
        }

        Ok(Self {
            reader,
            remaining: payload / RD_RECORD_SIZE as u64,
        })
    }
}

impl Iterator for RdReader {
    type Item = io::Result<Sample>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let mut rec = [0u8; RD_RECORD_SIZE];
        if let Err(err) = self.reader.read_exact(&mut rec) {
            return Some(Err(err));
        }
        self.remaining -= 1;

        let own = u64::from_le_bytes(rec[0..8].try_into().unwrap());
        let opponent = u64::from_le_bytes(rec[8..16].try_into().unwrap());
        let value = i16::from_le_bytes(rec[16..18].try_into().unwrap());
        Some(Ok(Sample {
            board: Board {
                player: own,
                opponent,
            },
            target: value as f32,
            phase: phase_from_stones((own | opponent).count_ones() as usize),
        }))
    }
}

pub fn split_paths(data_dir: &Path, split: &str, range: &PhaseRange) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for phase in range.start..=range.end {
        let path = data_dir
            .join(format!("phase_{phase}"))
            .join(format!("{split}.rd"));
        if path.exists() {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err(invalid_data(format!(
            "{}: no {split}.rd files in phase range {}..{}",
            data_dir.display(),
            range.start,
            range.end
        )));
    }
    Ok(paths)
}

pub fn read_split_shuffled<R: Rng>(
    data_dir: &Path,
    split: &str,
    range: &PhaseRange,
    rng: &mut R,
) -> io::Result<Vec<PathBuf>> {
    let mut paths = split_paths(data_dir, split, range)?;
    paths.shuffle(rng);
    Ok(paths)
}

pub fn count_records(paths: &[PathBuf]) -> io::Result<u64> {
    let mut total = 0u64;
    for path in paths {
        let size = path.metadata()?.len();
        if size < RD_MAGIC.len() as u64 {
            return Err(invalid_data(format!(
                "{}: missing rd header",
                path.display()
            )));
        }
        let payload = size - RD_MAGIC.len() as u64;
        if payload % RD_RECORD_SIZE as u64 != 0 {
            return Err(invalid_data(format!("{}: invalid rd size", path.display())));
        }
        total += payload / RD_RECORD_SIZE as u64;
    }
    Ok(total)
}

pub struct PrefetchedSamples {
    receiver: Receiver<io::Result<Sample>>,
}

impl Iterator for PrefetchedSamples {
    type Item = io::Result<Sample>;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv().ok()
    }
}

pub fn prefetch_paths(paths: Vec<PathBuf>, capacity: usize) -> PrefetchedSamples {
    let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
    thread::spawn(move || {
        for path in paths {
            let reader = match RdReader::open(&path) {
                Ok(reader) => reader,
                Err(err) => {
                    let _ = sender.send(Err(err));
                    return;
                }
            };
            for sample in reader {
                if sender.send(sample).is_err() {
                    return;
                }
            }
        }
    });
    PrefetchedSamples { receiver }
}

#[derive(Clone, Copy, Debug)]
pub struct PhaseRange {
    pub start: usize,
    pub end: usize,
}

impl std::str::FromStr for PhaseRange {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (start, end) = value
            .split_once("..")
            .ok_or_else(|| format!("invalid phase range: {value}"))?;
        let start = start
            .parse::<usize>()
            .map_err(|_| format!("invalid phase range start: {start}"))?;
        let end = end
            .parse::<usize>()
            .map_err(|_| format!("invalid phase range end: {end}"))?;
        if start > end {
            return Err(format!("invalid phase range: {value}"));
        }
        Ok(Self { start, end })
    }
}

impl Default for PhaseRange {
    fn default() -> Self {
        Self { start: 12, end: 63 }
    }
}

pub fn phase_from_stones(stones: usize) -> usize {
    stones.saturating_sub(4).min(N_PHASES - 1)
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn stones_map_to_engine_phase() {
        assert_eq!(phase_from_stones(12), 8);
        assert_eq!(phase_from_stones(63), 59);
        assert_eq!(phase_from_stones(64), 59);
    }

    #[test]
    fn rd_reader_reads_little_endian_record() {
        let dir = std::env::temp_dir().join(format!(
            "deft-rd-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("phase_12")).unwrap();
        let path = dir.join("phase_12").join("train.rd");
        let own = 0x0000_0000_0000_00f0u64;
        let opponent = 0x0000_0000_0000_0f00u64;
        let value = -12i16;
        let mut bytes = RD_MAGIC.to_vec();
        bytes.extend_from_slice(&own.to_le_bytes());
        bytes.extend_from_slice(&opponent.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
        fs::write(&path, bytes).unwrap();

        let sample = RdReader::open(&path).unwrap().next().unwrap().unwrap();
        assert_eq!(sample.board.player, own);
        assert_eq!(sample.board.opponent, opponent);
        assert_eq!(sample.target, value as f32);
        assert_eq!(sample.phase, 4);

        fs::remove_dir_all(dir).unwrap();
    }
}
