//! Builds an `.rd` dataset from game transcripts, in the layout of
//! reversi-dataset-gen (rdg, <https://github.com/ikepggthb/reversi-dataset-gen>).
//!
//! The input is a directory of directories named by the number of random
//! opening moves `N` (as in Egaroucid's training data), each holding text
//! files of one game per line (`f5d6c3...`). The first `N` moves of a game
//! were random, so the positions before them say little about the final
//! result and are left out. Every later position with a move to play becomes
//! a record.
//!
//! As rdg writes them:
//! - the label is the final disc difference, black minus white, from the
//!   side to move; empty squares count for nobody
//! - a position goes to `train`, `valid` or `test` by its canonical hash
//!   (`canonical_own_opponent_v1`), so a position and its symmetric copies
//!   always land in the same split
//! - `phase_<NN>/` holds the positions of phase `NN` = discs − 4, with a
//!   `metadata.json` and a `stats.json`; `summary.json` covers every phase
//!
//! Unlike rdg's generator, repeated positions are all kept: each comes from
//! a different game and carries that game's result.
use crate::{Result, rd};
use clap::Args;
use deft_reversi_engine::{Action, Board, Square};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::PathBuf,
};

#[derive(Args, Debug)]
pub struct ConvertArgs {
    /// Directory of `<random moves>/` directories of transcript files.
    #[arg(long)]
    transcripts: PathBuf,
    /// Output dataset directory.
    #[arg(long)]
    out: PathBuf,
    /// Share of positions in `train`.
    #[arg(long, default_value_t = 0.98)]
    train: f64,
    /// Share of positions in `valid`; the rest go to `test`.
    #[arg(long, default_value_t = 0.01)]
    valid: f64,
}

const SPLITS: [&str; 3] = ["train", "valid", "test"];
const RECORD_SPEC: &str = "u64 own_bits little-endian, u64 opponent_bits little-endian, i16 value little-endian; side-to-move perspective; bit0=a1, bit63=h8";

/// The positions a game contributes and the label of each.
pub fn game_records(moves: &str, random: usize) -> Option<Vec<rd::Record>> {
    let bytes = moves.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut board = Board::new();
    // Black moves first; `black_to_move` follows the side to move.
    let mut black_to_move = true;
    let mut positions = Vec::new();
    for (i, pair) in bytes.chunks(2).enumerate() {
        let square: Square = std::str::from_utf8(pair).ok()?.parse().ok()?;
        if board.must_pass() {
            board = board.play(Action::Pass).ok()?;
            black_to_move = !black_to_move;
        }
        if i >= random {
            positions.push((board, black_to_move));
        }
        board = board.play(Action::Place(square)).ok()?;
        black_to_move = !black_to_move;
    }
    if !board.is_terminal() {
        return None;
    }
    let mover = board.player_bits().count_ones() as i32 - board.opponent_bits().count_ones() as i32;
    let black_margin = if black_to_move { mover } else { -mover };
    Some(
        positions
            .into_iter()
            .map(|(board, black)| rd::Record {
                board,
                value: (if black { black_margin } else { -black_margin }) as i8,
            })
            .collect(),
    )
}

/// rdg's transform `t` of a bitboard (bit `8 y + x` is square (x, y)).
fn transform(bits: u64, t: u8) -> u64 {
    let mut out = 0;
    let mut rest = bits;
    while rest != 0 {
        let from = rest.trailing_zeros() as i32;
        rest &= rest - 1;
        let (x, y) = (from % 8, from / 8);
        let (tx, ty) = match t {
            0 => (x, y),
            1 => (7 - y, x),
            2 => (7 - x, 7 - y),
            3 => (y, 7 - x),
            4 => (7 - x, y),
            5 => (x, 7 - y),
            6 => (y, x),
            _ => (7 - y, 7 - x),
        };
        out |= 1 << (ty * 8 + tx);
    }
    out
}

/// rdg's `CanonicalHashBits`: the SHA-256, in lowercase hex, of the smallest
/// (own, opponent) pair over the eight symmetries, as 16 little-endian bytes.
pub fn canonical_hash(own: u64, opponent: u64) -> String {
    let best = (0..8)
        .map(|t| (transform(own, t), transform(opponent, t)))
        .min()
        .unwrap();
    let mut payload = [0u8; 16];
    payload[..8].copy_from_slice(&best.0.to_le_bytes());
    payload[8..].copy_from_slice(&best.1.to_le_bytes());
    Sha256::digest(payload)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// rdg's `chooseSplit`: the first 8 bytes of the SHA-256 of the hash text,
/// big endian, as a fraction of `u64::MAX`.
fn split_of(hash: &str, train: f64, valid: f64) -> usize {
    let sum = Sha256::digest(hash.as_bytes());
    let x = u64::from_be_bytes(sum[..8].try_into().unwrap());
    let f = x as f64 / u64::MAX as f64;
    if f < train {
        0
    } else if f < train + valid {
        1
    } else {
        2
    }
}

/// Labels seen in one phase, for `stats.json`.
#[derive(Default)]
struct Values {
    histogram: BTreeMap<i8, u64>,
    split: [u64; 3],
}

struct Writers {
    out: PathBuf,
    files: Vec<[Option<BufWriter<File>>; 3]>,
    values: Vec<Values>,
}

impl Writers {
    fn write(&mut self, split: usize, r: &rd::Record) -> std::io::Result<()> {
        let stones = 64 - r.board.empty_count();
        let slot = &mut self.files[usize::from(stones)][split];
        if slot.is_none() {
            let path = rd::path(&self.out, stones, SPLITS[split]);
            fs::create_dir_all(path.parent().unwrap())?;
            let mut file = BufWriter::with_capacity(1 << 20, File::create(path)?);
            file.write_all(rd::MAGIC)?;
            *slot = Some(file);
        }
        let file = slot.as_mut().unwrap();
        file.write_all(&r.board.player_bits().to_le_bytes())?;
        file.write_all(&r.board.opponent_bits().to_le_bytes())?;
        file.write_all(&i16::from(r.value).to_le_bytes())?;
        let values = &mut self.values[usize::from(stones)];
        *values.histogram.entry(r.value).or_default() += 1;
        values.split[split] += 1;
        Ok(())
    }
}

pub fn run(args: ConvertArgs) -> Result<()> {
    let share = |x: f64| x.is_finite() && (0.0..=1.0).contains(&x);
    if !share(args.train) || !share(args.valid) || args.train + args.valid > 1.0 {
        return Err("--train and --valid must be shares with a sum of at most 1".into());
    }
    let mut dirs: Vec<(usize, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&args.transcripts)? {
        let path = entry?.path();
        if let Some(n) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse().ok())
            && path.is_dir()
        {
            dirs.push((n, path));
        }
    }
    if dirs.is_empty() {
        return Err(format!(
            "{}: no directories named by the number of random moves",
            args.transcripts.display()
        )
        .into());
    }
    dirs.sort();
    let started = now();
    let mut writers = Writers {
        out: args.out.clone(),
        files: (0..=64).map(|_| [None, None, None]).collect(),
        values: (0..=64).map(|_| Values::default()).collect(),
    };
    let (mut games, mut skipped) = (0u64, 0u64);
    for (random, dir) in &dirs {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        files.retain(|p| p.extension().is_some_and(|e| e == "txt"));
        files.sort();
        for file in &files {
            for line in BufReader::new(File::open(file)?).lines() {
                let line = line?;
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                games += 1;
                let Some(game) = game_records(line, *random) else {
                    skipped += 1;
                    continue;
                };
                for r in &game {
                    let hash = canonical_hash(r.board.player_bits(), r.board.opponent_bits());
                    writers.write(split_of(&hash, args.train, args.valid), r)?;
                }
            }
        }
        let total: u64 = writers
            .values
            .iter()
            .map(|v| v.split.iter().sum::<u64>())
            .sum();
        println!("random {random:>2}: {games} games, {skipped} skipped, {total} positions");
    }
    for slot in writers.files.iter_mut().flatten().flatten() {
        slot.flush()?;
    }
    write_metadata(&args, &writers.values, games, skipped, &started)?;
    println!("wrote {}", args.out.display());
    Ok(())
}

/// The local time in RFC 3339, as rdg writes it.
fn now() -> String {
    std::process::Command::new("date")
        .arg("--iso-8601=seconds")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

fn write_metadata(
    args: &ConvertArgs,
    values: &[Values],
    games: u64,
    skipped: u64,
    started: &str,
) -> Result<()> {
    let split = serde_json::json!({
        "train": args.train,
        "valid": args.valid,
        "test": 1.0 - args.train - args.valid,
    });
    let source = serde_json::json!({
        "transcripts": args.transcripts.display().to_string(),
        "random_prefix": "excluded",
        "games_seen": games,
        "games_skipped": skipped,
        "label": "final disc difference (black minus white, empty squares not counted) from the side to move",
        "dedupe": "none",
    });
    let generated_at = now();
    let mut phases = Vec::new();
    let mut summaries = Vec::new();
    let mut totals = [0u64; 3];
    for (stones, v) in values.iter().enumerate() {
        let n: u64 = v.split.iter().sum();
        if n == 0 {
            continue;
        }
        let phase = stones - 4;
        let dir = rd::path(&args.out, stones as u8, "train")
            .parent()
            .unwrap()
            .to_owned();
        let metadata = serde_json::json!({
            "schema": "rdg_phase_metadata_v1",
            "mode": "import",
            "generation": "transcripts",
            "phase": phase,
            "format": "rdgbitboard_v1",
            "record_magic": "RDGBBVAL1\n",
            "record_size_bytes": 18,
            "record_spec": RECORD_SPEC,
            "hash_definition": "canonical_own_opponent_v1",
            "split": split,
            "source": source,
            "output_files": {"train": "train.rd", "valid": "valid.rd", "test": "test.rd"},
            "generated_at": generated_at,
        });
        fs::write(
            dir.join("metadata.json"),
            serde_json::to_string_pretty(&metadata)?,
        )?;
        let sum: i64 = v
            .histogram
            .iter()
            .map(|(&x, &c)| i64::from(x) * c as i64)
            .sum();
        let histogram: BTreeMap<String, u64> = v
            .histogram
            .iter()
            .map(|(x, &c)| (x.to_string(), c))
            .collect();
        let stats = serde_json::json!({
            "schema": "rdg_phase_stats_v1",
            "mode": "import",
            "phase": phase,
            "emitted_samples": n,
            "value": {
                "min": v.histogram.keys().next(),
                "max": v.histogram.keys().next_back(),
                "mean": sum as f64 / n as f64,
                "histogram": histogram,
            },
            "split": {"train": v.split[0], "valid": v.split[1], "test": v.split[2]},
        });
        fs::write(
            dir.join("stats.json"),
            serde_json::to_string_pretty(&stats)?,
        )?;
        phases.push(phase);
        summaries.push(serde_json::json!({"phase": phase, "emitted_samples": n}));
        for (t, s) in totals.iter_mut().zip(v.split) {
            *t += s;
        }
    }
    let summary = serde_json::json!({
        "schema": "rdg_summary_v1",
        "mode": "import",
        "generation": "transcripts",
        "started_at": started,
        "finished_at": now(),
        "phases": phases,
        "total_samples": totals.iter().sum::<u64>(),
        "split": {"train": totals[0], "valid": totals[1], "test": totals[2]},
        "phase_stats": summaries,
        "source": source,
    });
    fs::write(
        args.out.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng, rngs::StdRng};

    /// A random finished game as a transcript, with the positions before
    /// each placement and whether black was to move there.
    fn random_game(random: &mut StdRng) -> (String, Vec<(Board, bool)>, i32) {
        let mut board = Board::new();
        let mut black = true;
        let (mut text, mut before) = (String::new(), Vec::new());
        while !board.is_terminal() {
            if board.must_pass() {
                board = board.play(Action::Pass).unwrap();
                black = !black;
            }
            let moves: Vec<Square> = board.legal_moves().collect();
            let square = moves[random.gen_range(0..moves.len())];
            before.push((board, black));
            text += &square.to_string().to_ascii_lowercase();
            board = board.play(Action::Place(square)).unwrap();
            black = !black;
        }
        let (p, o) = (
            board.player_bits().count_ones() as i32,
            board.opponent_bits().count_ones() as i32,
        );
        let black_margin = if black { p - o } else { o - p };
        (text, before, black_margin)
    }

    #[test]
    fn records_skip_the_random_moves_and_label_the_mover() {
        let mut random = StdRng::seed_from_u64(3);
        for _ in 0..200 {
            let (text, before, black_margin) = random_game(&mut random);
            let skip = random.gen_range(0..before.len());
            let records = game_records(&text, skip).unwrap();
            assert_eq!(records.len(), before.len() - skip);
            for (r, (board, black)) in records.iter().zip(&before[skip..]) {
                assert_eq!(r.board, *board);
                let expected = if *black { black_margin } else { -black_margin };
                assert_eq!(i32::from(r.value), expected);
            }
            // An unfinished game is rejected.
            assert!(game_records(&text[..text.len() - 2], 0).is_none());
        }
        assert!(game_records("f5f5", 0).is_none());
        assert!(game_records("f5d", 0).is_none());
    }

    #[test]
    fn empty_squares_count_for_nobody() {
        // Random games until one ends with empty squares.
        let mut random = StdRng::seed_from_u64(8);
        loop {
            let (text, _, black_margin) = random_game(&mut random);
            if text.len() / 2 == 60 {
                continue;
            }
            let first = game_records(&text, 0).unwrap()[0];
            assert_eq!(i32::from(first.value), black_margin);
            let mut end = Board::new();
            for pair in text.as_bytes().chunks(2) {
                if end.must_pass() {
                    end = end.play(Action::Pass).unwrap();
                }
                let square: Square = std::str::from_utf8(pair).unwrap().parse().unwrap();
                end = end.play(Action::Place(square)).unwrap();
            }
            // The engine's score gives the empty squares to the winner.
            assert!(end.final_score().unsigned_abs() > black_margin.unsigned_abs());
            break;
        }
    }

    #[test]
    fn hashes_and_splits_follow_rdg() {
        // Reference values computed with rdg's algorithm (CanonicalHashBits,
        // chooseSplit) outside this code.
        let initial = Board::new();
        assert_eq!(
            canonical_hash(initial.player_bits(), initial.opponent_bits()),
            "5ca9061332dc4d68db2d7e543ffd3961d39dd0c84db3dba26ea191805f394211"
        );
        assert_eq!(
            canonical_hash(0xff, 0xff00_0000_0000_0000),
            "d580b85cc60b3c505c484bb0773e72f9d6eb63ff360e4ceef1acbd1b01be118d"
        );
        assert_eq!(
            canonical_hash(123_456_789, 987_654_321 << 20),
            "3fa47e439d40d40abe49c17bc0ecb62c296ea0be58fd6b030b7680f2cbeb2451"
        );
        for (i, split) in [(1u64, 0), (15, 2), (140, 1)] {
            assert_eq!(split_of(&canonical_hash(i, i << 20), 0.98, 0.01), split);
        }
        // Every symmetry of a position has the same hash.
        let after = initial.play("f5".parse().unwrap()).unwrap();
        let hashes: Vec<String> = (0..8)
            .map(|t| {
                canonical_hash(
                    transform(after.player_bits(), t),
                    transform(after.opponent_bits(), t),
                )
            })
            .collect();
        assert!(hashes.iter().all(|h| *h == hashes[0]));
    }
}
