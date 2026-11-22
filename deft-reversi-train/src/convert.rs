//! Builds an `.rd` dataset from game transcripts or labelled positions, in
//! the layout of reversi-dataset-gen (rdg,
//! <https://github.com/ikepggthb/reversi-dataset-gen>).
//!
//! Two inputs, as Egaroucid publishes its training data:
//! - `--transcripts`: a directory of directories named by the number of
//!   random opening moves `N` (training data v0002), each holding text
//!   files of one game per line (`f5d6c3...`). The first `N` moves of a game
//!   were random, so the positions before them say little about the final
//!   result and are left out, and so are the next `--after-random` moves'.
//!   Every later position with a move to play becomes a record, labelled
//!   with the game's final disc difference from its side to move. Empty
//!   squares go to the winner, as in the engine's final score.
//! - `--boards`: directories of text files of one position per line, 64
//!   characters (a1, b1, ..., h8; `X` the side to move, `O` the opponent,
//!   `-` empty) and its score from the side to move (training data of
//!   Egaroucid 7.4.0 / 7.5.1). The score is kept as it is.
//!
//! `--stones` keeps the positions with that many discs only.
//!
//! As rdg writes them:
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
#[command(group(clap::ArgGroup::new("input").required(true).args(["transcripts", "boards"])))]
pub struct ConvertArgs {
    /// Directory of `<random moves>/` directories of transcript files.
    #[arg(long)]
    transcripts: Option<PathBuf>,
    /// Directory of directories of `<64 squares> <score>` position files.
    #[arg(long)]
    boards: Option<PathBuf>,
    /// With `--transcripts`, also leave out this many moves' positions after
    /// the random ones.
    #[arg(long, default_value_t = 0, requires = "transcripts")]
    after_random: usize,
    /// Keep the positions with this many discs only.
    #[arg(long, default_value = "4..=63")]
    stones: crate::StoneRange,
    /// Output dataset directory.
    #[arg(long)]
    out: PathBuf,
    /// Files converted at once (default: every core). The output is the same
    /// for any number.
    #[arg(short, long)]
    threads: Option<usize>,
    /// Share of positions in `train`.
    #[arg(long, default_value_t = 0.98)]
    train: f64,
    /// Share of positions in `valid`; the rest go to `test`.
    #[arg(long, default_value_t = 0.01)]
    valid: f64,
}

const SPLITS: [&str; 3] = ["train", "valid", "test"];
const RECORD_SPEC: &str = "u64 own_bits little-endian, u64 opponent_bits little-endian, i16 value little-endian; side-to-move perspective; bit0=a1, bit63=h8";

/// The positions a game contributes, from the one before move `skip` (0
/// based, passes not counted) on, and the label of each: the final disc
/// difference from its side to move, empty squares to the winner.
pub fn game_records(moves: &str, skip: usize) -> Option<Vec<rd::Record>> {
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
        if i >= skip {
            positions.push((board, black_to_move));
        }
        board = board.play(Action::Place(square)).ok()?;
        black_to_move = !black_to_move;
    }
    if !board.is_terminal() {
        return None;
    }
    let mover = board.final_score();
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

/// A position line of `--boards`: 64 squares from a1 to h8 (`X` the side to
/// move, `O` the opponent, `-` empty), a space and the score from the side
/// to move.
pub fn board_record(line: &str) -> Option<rd::Record> {
    let (squares, score) = line.split_once(' ')?;
    if squares.len() != 64 {
        return None;
    }
    let (mut own, mut opponent) = (0u64, 0u64);
    for (i, c) in squares.bytes().enumerate() {
        match c {
            b'X' => own |= 1 << i,
            b'O' => opponent |= 1 << i,
            b'-' => {}
            _ => return None,
        }
    }
    let value: i8 = score.trim().parse().ok()?;
    if !(-64..=64).contains(&value) {
        return None;
    }
    Some(rd::Record {
        board: Board::from_bits(own, opponent)?,
        value,
    })
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
    let input = args.transcripts.as_ref().or(args.boards.as_ref()).unwrap();
    // Transcripts by their number of random moves; position files in one
    // group of their directory's order.
    let mut dirs: Vec<(usize, PathBuf)> = Vec::new();
    for entry in fs::read_dir(input)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        if args.transcripts.is_some() {
            if let Some(n) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.parse().ok())
            {
                dirs.push((n, path));
            }
        } else {
            dirs.push((0, path));
        }
    }
    if dirs.is_empty() {
        return Err(format!(
            "{}: no directories{}",
            input.display(),
            if args.transcripts.is_some() {
                " named by the number of random moves"
            } else {
                ""
            }
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
    // Every file in order, with its number of random moves.
    let mut jobs: Vec<(usize, PathBuf)> = Vec::new();
    for (random, dir) in &dirs {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        files.retain(|p| p.extension().is_some_and(|e| e == "txt"));
        files.sort();
        jobs.extend(files.into_iter().map(|f| (*random, f)));
    }
    let threads = args
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
        .max(1);
    // Files are read and their records split in parallel, a batch at a
    // time; the records are written in file order, so the output does not
    // depend on the threads.
    let (mut lines, mut skipped) = (0u64, 0u64);
    for batch in jobs.chunks(threads * 2) {
        let results: Vec<std::sync::Mutex<Option<std::io::Result<FileRecords>>>> =
            batch.iter().map(|_| std::sync::Mutex::new(None)).collect();
        let indices: Vec<usize> = (0..batch.len()).collect();
        crate::for_each_parallel(&indices, threads, |&i| {
            let (random, file) = &batch[i];
            *results[i].lock().unwrap() = Some(file_records(&args, *random, file));
            Ok(())
        })?;
        for result in results {
            let r = result.into_inner().unwrap().unwrap()?;
            lines += r.lines;
            skipped += r.skipped;
            for (split, record) in &r.records {
                writers.write(*split, record)?;
            }
        }
        let total: u64 = writers
            .values
            .iter()
            .map(|v| v.split.iter().sum::<u64>())
            .sum();
        println!(
            "{}: {lines} lines, {skipped} skipped, {total} positions",
            batch.last().unwrap().1.display()
        );
    }
    for slot in writers.files.iter_mut().flatten().flatten() {
        slot.flush()?;
    }
    write_metadata(&args, &writers.values, lines, skipped, &started)?;
    println!("wrote {}", args.out.display());
    Ok(())
}

/// The records of one input file with their splits, in line order.
struct FileRecords {
    lines: u64,
    skipped: u64,
    records: Vec<(usize, rd::Record)>,
}

fn file_records(
    args: &ConvertArgs,
    random: usize,
    file: &std::path::Path,
) -> std::io::Result<FileRecords> {
    let mut out = FileRecords {
        lines: 0,
        skipped: 0,
        records: Vec::new(),
    };
    let mut keep = |r: rd::Record| {
        if args.stones.contains(64 - r.board.empty_count()) {
            let hash = canonical_hash(r.board.player_bits(), r.board.opponent_bits());
            out.records
                .push((split_of(&hash, args.train, args.valid), r));
        }
    };
    let (mut lines, mut skipped) = (0, 0);
    for line in BufReader::new(File::open(file)?).lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        lines += 1;
        if args.transcripts.is_some() {
            match game_records(line, random + args.after_random) {
                Some(game) => game.into_iter().for_each(&mut keep),
                None => skipped += 1,
            }
        } else {
            match board_record(line) {
                Some(r) => keep(r),
                None => skipped += 1,
            }
        }
    }
    out.lines = lines;
    out.skipped = skipped;
    Ok(out)
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
    let source = match (&args.transcripts, &args.boards) {
        (Some(transcripts), _) => serde_json::json!({
            "transcripts": transcripts.display().to_string(),
            "random_prefix": "excluded",
            "after_random_excluded": args.after_random,
            "stones": format!("{}..={}", args.stones.first, args.stones.last),
            "games_seen": games,
            "games_skipped": skipped,
            "label": "final disc difference from the side to move, empty squares to the winner",
            "dedupe": "none",
        }),
        (None, boards) => serde_json::json!({
            "boards": boards.as_ref().unwrap().display().to_string(),
            "stones": format!("{}..={}", args.stones.first, args.stones.last),
            "positions_seen": games,
            "positions_skipped": skipped,
            "label": "the score given with each position, from the side to move",
            "dedupe": "none",
        }),
    };
    let generation = if args.transcripts.is_some() {
        "transcripts"
    } else {
        "boards"
    };
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
            "generation": generation,
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
        "generation": generation,
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
        // The engine's final score: empty squares to the winner.
        let score = board.final_score();
        let black_margin = if black { score } else { -score };
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
    fn empty_squares_go_to_the_winner() {
        // Random games until one ends with empty squares.
        let mut random = StdRng::seed_from_u64(8);
        loop {
            let (text, _, _) = random_game(&mut random);
            if text.len() / 2 == 60 {
                continue;
            }
            let mut end = Board::new();
            let mut black = true;
            for pair in text.as_bytes().chunks(2) {
                if end.must_pass() {
                    end = end.play(Action::Pass).unwrap();
                    black = !black;
                }
                let square: Square = std::str::from_utf8(pair).unwrap().parse().unwrap();
                end = end.play(Action::Place(square)).unwrap();
                black = !black;
            }
            let expected = if black {
                end.final_score()
            } else {
                -end.final_score()
            };
            let counted =
                end.player_bits().count_ones() as i32 - end.opponent_bits().count_ones() as i32;
            assert!(expected.unsigned_abs() > counted.unsigned_abs());
            let first = game_records(&text, 0).unwrap()[0];
            assert_eq!(i32::from(first.value), expected);
            break;
        }
    }

    #[test]
    fn board_lines_read_the_mover_and_the_score() {
        let line = "-XO-OOXOOXX-OXOO-XXOXXOOX-OXOOXOOXOOOXXXO-XOOOXXO-O-OO---OOOX-O- 4";
        let r = board_record(line).unwrap();
        assert_eq!(r.value, 4);
        assert_eq!(r.board.player_bits() & 0b111, 0b010);
        assert_eq!(r.board.opponent_bits() & 0b111, 0b100);
        assert_eq!(64 - r.board.empty_count(), 51);
        assert!(board_record("-XO 4").is_none());
        assert!(board_record(&line.replace(' ', "")).is_none());
        assert!(board_record(&line.replace(" 4", " 65")).is_none());
        assert!(board_record(&line.replacen('-', "?", 1)).is_none());
    }

    #[test]
    fn any_number_of_threads_writes_the_same_dataset() {
        let dir = crate::test_dir("convert");
        let mut random = StdRng::seed_from_u64(21);
        for n in [8, 9] {
            let games = dir.join(format!("transcripts/{n}"));
            fs::create_dir_all(&games).unwrap();
            for f in 0..5 {
                let text: Vec<String> = (0..20).map(|_| random_game(&mut random).0).collect();
                fs::write(games.join(format!("{f:07}.txt")), text.join("\n")).unwrap();
            }
        }
        let convert = |threads, out: &str| {
            run(ConvertArgs {
                transcripts: Some(dir.join("transcripts")),
                boards: None,
                after_random: 3,
                stones: "20..=50".parse().unwrap(),
                out: dir.join(out),
                threads: Some(threads),
                train: 0.6,
                valid: 0.2,
            })
            .unwrap()
        };
        convert(1, "one");
        convert(4, "four");
        let mut compared = 0;
        for stones in 20..=50u8 {
            for split in SPLITS {
                let (a, b) = (
                    rd::path(&dir.join("one"), stones, split),
                    rd::path(&dir.join("four"), stones, split),
                );
                assert_eq!(a.exists(), b.exists());
                if a.exists() {
                    assert_eq!(fs::read(a).unwrap(), fs::read(b).unwrap());
                    compared += 1;
                }
            }
        }
        assert!(compared > 30);
        assert!(!rd::path(&dir.join("one"), 19, "train").exists());
        fs::remove_dir_all(dir).unwrap();
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
