use crate::eval_mae::{read_rd_records, RdRecord};
use clap::ValueEnum;
use deft_reversi_engine::{Board, Solver, SolverOptions, NO_MPC_SELECTIVITY_LV};
use rand::prelude::*;
use rand::rngs::StdRng;
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MpcCollectMode {
    Eval,
    Final,
}

#[derive(Debug)]
pub struct MpcCollectArgs {
    pub eval_path: String,
    pub data_root: String,
    pub split: String,
    pub mode: MpcCollectMode,
    pub levels: String,
    pub empties: String,
    pub probe_depths: Option<String>,
    pub probe_levels: Option<String>,
    pub positions_per_bucket: usize,
    pub out: String,
    pub seed: u64,
}

pub fn run(args: MpcCollectArgs) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(&args.out)?;
    let solver = Solver::from_file(&args.eval_path, SolverOptions::default())?;
    let start = Instant::now();
    let mut searches = 0usize;
    let positions = match args.mode {
        MpcCollectMode::Eval => {
            let levels = parse_list(&args.levels)?;
            let positions = sample_for_eval(
                &args.data_root,
                &args.split,
                args.positions_per_bucket,
                args.seed,
            )?;
            for level in levels {
                let path = Path::new(&args.out).join(format!("mpc_lv{level}_serach.txt"));
                searches += if let Some(probe_depths) = &args.probe_depths {
                    let probe_depths = parse_list(probe_depths)?;
                    collect_eval_level_probe_sweep(
                        &solver,
                        &positions,
                        level,
                        &probe_depths,
                        &path,
                    )?
                } else {
                    collect_eval_level(&solver, &positions, level, &path)?
                };
            }
            positions.len()
        }
        MpcCollectMode::Final => {
            let empties = parse_range(&args.empties)?;
            let buckets = sample_by_empties(
                &args.data_root,
                &args.split,
                &empties,
                args.positions_per_bucket,
                args.seed,
            )?;
            let mut positions = 0usize;
            for empties in empties {
                let records = buckets.get(&empties).cloned().unwrap_or_default();
                positions += records.len();
                let path = Path::new(&args.out).join(format!("mpc_perfect_e{empties}.txt"));
                searches += if let Some(probe_levels) = &args.probe_levels {
                    let probe_levels = parse_list(probe_levels)?;
                    collect_final_empties_probe_sweep(
                        &solver,
                        &records,
                        empties,
                        &probe_levels,
                        &path,
                    )?
                } else {
                    collect_final_empties(&solver, &records, empties, &path)?
                };
            }
            positions
        }
    };
    let elapsed = start.elapsed();
    let seconds_per_position = if positions == 0 {
        0.0
    } else {
        elapsed.as_secs_f64() / positions as f64
    };
    eprintln!(
        "mpc-collect: positions={positions}, searches={searches}, elapsed={:.3}s, cost={:.6}s/position",
        elapsed.as_secs_f64(),
        seconds_per_position
    );
    Ok(())
}

fn collect_eval_level(
    solver: &Solver,
    positions: &[Board],
    level: i32,
    path: &Path,
) -> io::Result<usize> {
    let shallow_level = solver.mpc_config().eval_search.search_lv_by_depth[level as usize];
    let mut out = open_output(path)?;
    for (idx, board) in positions.iter().enumerate() {
        solver.clear_tt();
        let shallow = solver.solve_eval(board, shallow_level, NO_MPC_SELECTIVITY_LV);
        solver.clear_tt();
        let deep = solver.solve_eval(board, level, NO_MPC_SELECTIVITY_LV);
        writeln!(
            out,
            "{},{},{},{},{}",
            board.empties_count(),
            level,
            shallow_level,
            deep.score,
            shallow.score
        )?;
        eprintln!("eval lv{level}: {}/{}", idx + 1, positions.len());
    }
    Ok(positions.len() * 2)
}

fn collect_eval_level_probe_sweep(
    solver: &Solver,
    positions: &[Board],
    level: i32,
    probe_depths: &[i32],
    path: &Path,
) -> io::Result<usize> {
    let mut out = open_output(path)?;
    let mut searches = 0usize;
    for (idx, board) in positions.iter().enumerate() {
        solver.clear_tt();
        let deep = solver.solve_eval(board, level, NO_MPC_SELECTIVITY_LV);
        searches += 1;

        for &probe_depth in probe_depths.iter().filter(|&&depth| depth <= level - 2) {
            solver.clear_tt();
            let shallow = solver.solve_eval(board, probe_depth, NO_MPC_SELECTIVITY_LV);
            searches += 1;
            writeln!(
                out,
                "{},{},{},{},{}",
                board.empties_count(),
                level,
                probe_depth,
                deep.score,
                shallow.score
            )?;
        }
        eprintln!("eval lv{level}: {}/{}", idx + 1, positions.len());
    }
    Ok(searches)
}

fn collect_final_empties(
    solver: &Solver,
    positions: &[Board],
    empties: u32,
    path: &Path,
) -> io::Result<usize> {
    let shallow_level = solver.mpc_config().final_search.search_lv_by_empties[empties as usize];
    let mut out = open_output(path)?;
    for (idx, board) in positions.iter().enumerate() {
        solver.clear_tt();
        let shallow = solver.solve_eval(board, shallow_level, NO_MPC_SELECTIVITY_LV);
        solver.clear_tt();
        let deep = solver.solve_final(board, NO_MPC_SELECTIVITY_LV);
        writeln!(
            out,
            "{},{},{},{},{}",
            empties, empties, shallow_level, deep.score, shallow.score
        )?;
        eprintln!("final e{empties}: {}/{}", idx + 1, positions.len());
    }
    Ok(positions.len() * 2)
}

fn collect_final_empties_probe_sweep(
    solver: &Solver,
    positions: &[Board],
    empties: u32,
    probe_levels: &[i32],
    path: &Path,
) -> io::Result<usize> {
    let mut out = open_output(path)?;
    let mut searches = 0usize;
    for (idx, board) in positions.iter().enumerate() {
        solver.clear_tt();
        let deep = solver.solve_final(board, NO_MPC_SELECTIVITY_LV);
        searches += 1;

        for &probe_level in probe_levels
            .iter()
            .filter(|&&level| level <= empties as i32 - 2)
        {
            solver.clear_tt();
            let shallow = solver.solve_eval(board, probe_level, NO_MPC_SELECTIVITY_LV);
            searches += 1;
            writeln!(
                out,
                "{},{},{},{},{}",
                empties, empties, probe_level, deep.score, shallow.score
            )?;
        }
        eprintln!("final e{empties}: {}/{}", idx + 1, positions.len());
    }
    Ok(searches)
}

fn open_output(path: &Path) -> io::Result<fs::File> {
    let is_new = !path.exists() || path.metadata()?.len() == 0;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if is_new {
        writeln!(file, "# deft-reversi mpc collect")?;
        writeln!(file, "# n_empties,search_level,search_depth_for_prob_cut,search_score,search_score_for_prob_cut")?;
        writeln!(file, "# data")?;
    }
    Ok(file)
}

fn sample_for_eval(
    data_root: &str,
    split: &str,
    limit: usize,
    seed: u64,
) -> io::Result<Vec<Board>> {
    let mut records = read_dataset_records(data_root, split, 12..=63)?;
    shuffle_unique_take(&mut records, limit, seed)
}

fn sample_by_empties(
    data_root: &str,
    split: &str,
    empties: &[u32],
    limit: usize,
    seed: u64,
) -> io::Result<HashMap<u32, Vec<Board>>> {
    // phase はゲーム段階ラベルで empties の厳密なバケットではないため、
    // 全 phase を読んでから empties でフィルタする。
    let mut records = read_dataset_records(data_root, split, 12..=63)?;
    let mut rng = StdRng::seed_from_u64(seed);
    records.shuffle(&mut rng);
    let wanted: HashSet<u32> = empties.iter().copied().collect();
    let mut seen = HashSet::new();
    let mut buckets: HashMap<u32, Vec<Board>> = HashMap::new();
    for record in records {
        let e = record.board.empties_count();
        if !wanted.contains(&e) || !seen.insert((record.board.player, record.board.opponent)) {
            continue;
        }
        let bucket = buckets.entry(e).or_default();
        if bucket.len() < limit {
            bucket.push(record.board);
        }
        if wanted
            .iter()
            .all(|e| buckets.get(e).is_some_and(|bucket| bucket.len() >= limit))
        {
            break;
        }
    }
    Ok(buckets)
}

fn read_dataset_records(
    data_root: &str,
    split: &str,
    phases: std::ops::RangeInclusive<usize>,
) -> io::Result<Vec<RdRecord>> {
    let mut records = Vec::new();
    for phase in phases {
        let path: PathBuf = [data_root, &format!("phase_{phase}"), &format!("{split}.rd")]
            .iter()
            .collect();
        if path.exists() {
            records.extend(read_rd_records(&path, usize::MAX)?);
        }
    }
    Ok(records)
}

fn shuffle_unique_take(
    records: &mut [RdRecord],
    limit: usize,
    seed: u64,
) -> io::Result<Vec<Board>> {
    let mut rng = StdRng::seed_from_u64(seed);
    records.shuffle(&mut rng);
    let mut seen = HashSet::new();
    let mut boards = Vec::new();
    for record in records {
        if seen.insert((record.board.player, record.board.opponent)) {
            boards.push(record.board);
            if boards.len() >= limit {
                break;
            }
        }
    }
    Ok(boards)
}

fn parse_list(input: &str) -> io::Result<Vec<i32>> {
    input
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            s.trim()
                .parse::<i32>()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
                .and_then(|v| {
                    if (0..=60).contains(&v) {
                        Ok(v)
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "level must be in 0..=60",
                        ))
                    }
                })
        })
        .collect()
}

fn parse_range(input: &str) -> io::Result<Vec<u32>> {
    let normalized = input.replace("..=", "..");
    let (start, end) = normalized
        .split_once("..")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "range must be START..=END"))?;
    let start = start
        .parse::<u32>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let end = end
        .parse::<u32>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    if start > end || end > 60 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "range must satisfy start <= end <= 60",
        ));
    }
    Ok((start..=end).collect())
}
