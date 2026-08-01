use deft_reversi_engine::{Board, Evaluator};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RdRecord {
    pub board: Board,
    pub value: i16,
}

#[derive(Default, Clone, Copy)]
struct Bucket {
    samples: u64,
    abs_error_sum: u64,
}

impl Bucket {
    fn add(&mut self, abs_error: u64) {
        self.samples += 1;
        self.abs_error_sum += abs_error;
    }

    fn mae(self) -> f64 {
        self.abs_error_sum as f64 / self.samples as f64
    }
}

pub fn parse_phase_range(input: &str) -> io::Result<(usize, usize)> {
    let (start, end) = input.split_once("..").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "phase range must be START..END",
        )
    })?;
    let start = start
        .parse::<usize>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let end = end
        .parse::<usize>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    if start > end {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "phase range start must be <= end",
        ));
    }
    Ok((start, end))
}

pub fn parse_rd_record(bytes: &[u8; 18]) -> RdRecord {
    let own = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
    let opponent = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let value = i16::from_le_bytes(bytes[16..18].try_into().unwrap());
    RdRecord {
        board: Board {
            player: own,
            opponent,
        },
        value,
    }
}

pub fn read_rd_records(path: &Path, limit: usize) -> io::Result<Vec<RdRecord>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut prefix = [0u8; 10];
    let mut pending = Vec::new();
    match reader.read_exact(&mut prefix) {
        Ok(()) => {
            if &prefix != b"RDGBBVAL1\n" {
                pending.extend_from_slice(&prefix);
            }
        }
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(Vec::new()),
        Err(e) => return Err(e),
    }
    let mut records = Vec::new();
    for _ in 0..limit {
        let mut bytes = [0u8; 18];
        if !pending.is_empty() {
            bytes[..pending.len()].copy_from_slice(&pending);
            if let Err(e) = reader.read_exact(&mut bytes[pending.len()..]) {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break;
                }
                return Err(e);
            }
            pending.clear();
            records.push(parse_rd_record(&bytes));
            continue;
        }
        match reader.read_exact(&mut bytes) {
            Ok(()) => records.push(parse_rd_record(&bytes)),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }
    Ok(records)
}

pub fn run(
    eval_path: &str,
    data_root: &str,
    split: &str,
    phase_range: &str,
    limit_per_phase: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let evaluator = Evaluator::from_path(eval_path)?;
    let (phase_start, phase_end) = parse_phase_range(phase_range)?;
    let mut total = Bucket::default();
    let mut by_empties: BTreeMap<u32, Bucket> = BTreeMap::new();

    for phase in phase_start..=phase_end {
        let path: PathBuf = [data_root, &format!("phase_{phase}"), &format!("{split}.rd")]
            .iter()
            .collect();
        if !path.exists() {
            continue;
        }
        for record in read_rd_records(&path, limit_per_phase)? {
            let score = evaluator.evaluate_board_slow(&record.board);
            // .rd の value は手番側視点の最終石差そのもの(学習側と同じ解釈)
            let target = i32::from(record.value);
            let abs_error = (score - target).unsigned_abs() as u64;
            total.add(abs_error);
            by_empties
                .entry(record.board.empties_count())
                .or_default()
                .add(abs_error);
        }
    }

    println!("scope,samples,mae");
    if total.samples == 0 {
        println!("overall,0,NaN");
        return Ok(());
    }
    println!("overall,{},{}", total.samples, total.mae());
    println!("empties,samples,mae");
    for (empties, bucket) in by_empties {
        println!("{empties},{},{}", bucket.samples, bucket.mae());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rd_record() {
        let own = 0x0102_0304_0506_0708u64;
        let opponent = 0x1112_1314_1516_1718u64;
        let value = -12i16;
        let mut bytes = [0u8; 18];
        bytes[0..8].copy_from_slice(&own.to_le_bytes());
        bytes[8..16].copy_from_slice(&opponent.to_le_bytes());
        bytes[16..18].copy_from_slice(&value.to_le_bytes());

        let record = parse_rd_record(&bytes);
        assert_eq!(record.board.player, own);
        assert_eq!(record.board.opponent, opponent);
        assert_eq!(record.value, value);
    }

    #[test]
    fn parses_inclusive_phase_range() {
        assert_eq!(parse_phase_range("3..5").unwrap(), (3, 5));
        assert!(parse_phase_range("5..3").is_err());
    }
}
