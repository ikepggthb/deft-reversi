//! ProbCut (MPC) calibration for a model.
//!
//! ProbCut predicts a deep search value from a shallow one,
//! `deep ≈ slope * shallow + intercept`, and cuts when the prediction is
//! beyond the window by `sigma * deviation`. The coefficients depend on how
//! well the evaluation predicts, so every model needs its own.
//!
//! Midgame rules (a search of `depth` plies at `empties` empties) are
//! measured for depths 4..=`max_depth` and fitted with the reference
//! regression in empties, depth and shallow depth, which extends them to
//! every depth. Endgame rules (an exact solve) are measured for every
//! empties count in `--exact-empties`; larger counts reuse the last measured
//! count of the same parity, as the main-branch table does.
use crate::{Result, for_each_parallel, rd};
use clap::Args;
use deft_reversi_engine::{
    Board, Engine, EngineOptions, Goal, Model, ProbCutRule, ProbCutSigmas, SearchRequest,
};
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fmt::Write as _,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Args, Debug)]
pub struct MpcArgs {
    /// The model to calibrate (any format the engine reads).
    #[arg(short, long)]
    model: PathBuf,
    /// Dataset directory; positions come from `--split`.
    #[arg(long)]
    data: PathBuf,
    #[arg(long, default_value = "valid")]
    split: String,
    /// Writes `positions.tsv`, `cells.tsv` and `rules.json` here.
    #[arg(long)]
    out: PathBuf,
    /// Positions per empties count.
    #[arg(long, default_value_t = 64)]
    positions: usize,
    /// Empties counts of the positions, inclusive.
    #[arg(long, default_value = "12..=52")]
    empties: EmptiesRange,
    /// Deepest search measured (also capped at empties - 1).
    #[arg(long, default_value_t = 14)]
    max_depth: u8,
    /// Empties counts whose positions are also solved exactly, inclusive.
    #[arg(long, default_value = "12..=24")]
    exact_empties: EmptiesRange,
    /// Fit the rules again from `--out/positions.tsv` without searching.
    #[arg(long)]
    fit_only: bool,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(short, long)]
    threads: Option<usize>,
}

/// Empties counts `a..=b` (also `a..b`, inclusive) or `a`, within 1..=60.
#[derive(Clone, Copy, Debug)]
pub struct EmptiesRange {
    first: u8,
    last: u8,
}

impl std::str::FromStr for EmptiesRange {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let (a, b) = match s.split_once("..") {
            Some((a, b)) => (a, b.strip_prefix('=').unwrap_or(b)),
            None => (s, s),
        };
        let parse = |x: &str| {
            x.trim()
                .parse::<u8>()
                .map_err(|_| format!("bad empties count `{x}`"))
        };
        let (first, last) = (parse(a)?, parse(b)?);
        if !(1..=60).contains(&first) || !(first..=60).contains(&last) {
            return Err(format!("empties counts must be within 1..=60: {s}"));
        }
        Ok(Self { first, last })
    }
}

/// The shallow depth for each midgame depth, from the reference engine.
const SHALLOW_BY_DEPTH: [u8; 61] = [
    0, 0, 0, 0, 0, 1, 2, 1, 2, 1, 2, 3, 4, 3, 4, 5, 6, 5, 6, 5, 6, 5, 6, 5, 6, 7, 8, 7, 8, 7, 8, 7,
    8, 7, 8, 7, 8, 7, 8, 7, 8, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9,
    10,
];

/// The shallow depth for an exact solve, as in the main-branch table.
fn exact_shallow(empties: u8) -> u8 {
    match empties {
        12 | 14 | 16 => 4,
        13 | 15 => 3,
        17 | 19 => 5,
        18 | 20 => 6,
        e if e >= 29 => 8 - e % 2,
        e => 6 - e % 2,
    }
}

/// One measured pair of search values.
#[derive(Clone, Copy, Debug)]
struct Sample {
    exact: bool,
    empties: u8,
    depth: u8,
    shallow_depth: u8,
    shallow: i32,
    deep: i32,
}

/// A ProbCut rule, as written to `rules.json`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub exact: bool,
    pub depth: u8,
    pub empties: u8,
    pub shallow_depth: u8,
    pub slope: f64,
    pub intercept: f64,
    pub deviation: f64,
}

/// The deviation multiples per selectivity stage, as written to
/// `rules.json`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sigmas {
    pub midgame: [f64; 6],
    pub exact: [f64; 6],
}

impl From<Sigmas> for ProbCutSigmas {
    fn from(s: Sigmas) -> Self {
        ProbCutSigmas {
            midgame: s.midgame,
            exact: s.exact,
        }
    }
}

/// `rules.json`: what `Engine::set_probcut_calibration` takes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Calibration {
    pub sigmas: Sigmas,
    pub rules: Vec<Rule>,
}

/// The stage probabilities of `Selectivity::Percent68..=Percent99`.
const STAGE_PROBABILITIES: [f64; 6] = [0.68, 0.77, 0.85, 0.95, 0.98, 0.99];

impl From<Rule> for ProbCutRule {
    fn from(r: Rule) -> Self {
        ProbCutRule {
            exact: r.exact,
            depth: r.depth,
            empties: r.empties,
            shallow_depth: r.shallow_depth,
            slope: r.slope,
            intercept: r.intercept,
            deviation: r.deviation,
        }
    }
}

pub fn run(args: MpcArgs) -> Result<()> {
    fs::create_dir_all(&args.out)?;
    if args.max_depth < 4 || args.max_depth > 60 {
        return Err("--max-depth must be within 4..=60".into());
    }
    let positions_path = args.out.join("positions.tsv");
    if !args.fit_only {
        measure(&args, &positions_path)?;
    }
    let positions = read_positions(&positions_path)?.1;
    let samples = pairs(&positions);
    let (rules, report) = fit(&samples)?;
    let (sigmas, tails) = tail_sigmas(&samples, &rules)?;
    // The engine checks the calibration the same way when it is installed.
    let mut engine = Engine::new(
        Model::from_file(&args.model)?,
        EngineOptions::default().tt_mib(1),
    )?;
    engine.set_probcut_calibration(
        rules.iter().copied().map(Into::into).collect(),
        sigmas.into(),
    )?;
    fs::write(args.out.join("cells.tsv"), report)?;
    fs::write(args.out.join("tails.tsv"), tails)?;
    fs::write(
        args.out.join("rules.json"),
        serde_json::to_string_pretty(&Calibration {
            sigmas,
            rules: rules.clone(),
        })?,
    )?;
    println!(
        "{} positions, {} samples, {} rules written to {}",
        positions.len(),
        samples.len(),
        rules.len(),
        args.out.display()
    );
    Ok(())
}

/// The search values of one position: full-width searches of depth
/// `0..values.len()` and, when solved, the exact score. `index` is the
/// position's place in the seeded shuffle of its empties count, so a run
/// with more `--positions` extends the same selection.
#[derive(Clone, Debug, PartialEq)]
struct Position {
    empties: u8,
    index: usize,
    board: Board,
    exact: Option<i32>,
    values: Vec<i32>,
}

/// What a positions file was measured with; resuming requires the same.
fn settings_line(args: &MpcArgs) -> String {
    format!(
        "# split={} seed={} max_depth={} exact_empties={}..={} model_bytes={}",
        args.split,
        args.seed,
        args.max_depth,
        args.exact_empties.first,
        args.exact_empties.last,
        fs::metadata(&args.model).map_or(0, |m| m.len())
    )
}

/// Measures every selected position not yet in `path`, appending each job's
/// positions as it finishes, so an interrupted run resumes where it stopped.
fn measure(args: &MpcArgs, path: &Path) -> Result<()> {
    let settings = settings_line(args);
    let done: HashSet<(u8, usize)> = if path.exists() {
        let (found, positions) = read_positions(path)?;
        if found != settings {
            return Err(format!(
                "{} was measured with `{found}`, not `{settings}`",
                path.display()
            )
            .into());
        }
        positions.iter().map(|p| (p.empties, p.index)).collect()
    } else {
        fs::write(path, format!("{settings}\n{HEADER}\n"))?;
        HashSet::new()
    };
    let model = Model::from_file(&args.model)?;
    let mut jobs: Vec<(u8, Vec<(usize, Board)>)> = Vec::new();
    for empties in args.empties.first..=args.empties.last {
        let stones = 64 - empties;
        let path = rd::path(&args.data, stones, &args.split);
        let mut boards: Vec<Board> = rd::read(&path, stones)?
            .into_iter()
            .map(|r| r.board)
            .filter(|b| !b.is_terminal() && !b.must_pass())
            .collect();
        // A shuffle per empties count, so the selection of one count does
        // not depend on the others.
        let mut random = StdRng::seed_from_u64(args.seed ^ (u64::from(empties) << 32));
        boards.shuffle(&mut random);
        boards.truncate(args.positions);
        if boards.len() < args.positions {
            return Err(
                format!("{}: only {} usable positions", path.display(), boards.len()).into(),
            );
        }
        let todo: Vec<(usize, Board)> = boards
            .into_iter()
            .enumerate()
            .filter(|&(i, _)| !done.contains(&(empties, i)))
            .collect();
        // Small jobs keep the threads busy until the end.
        for chunk in todo.chunks(4) {
            jobs.push((empties, chunk.to_vec()));
        }
    }
    let solved =
        |empties: u8| (args.exact_empties.first..=args.exact_empties.last).contains(&empties);
    // The slowest jobs first: exact solves of many empties, then deep searches.
    jobs.sort_by_key(|&(empties, _)| std::cmp::Reverse((solved(empties), empties)));
    println!(
        "{} positions already measured, {} jobs to go",
        done.len(),
        jobs.len()
    );
    let file = Mutex::new(fs::OpenOptions::new().append(true).open(path)?);
    let finished = Mutex::new(0usize);
    let threads = args
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    for_each_parallel(&jobs, threads, |(empties, boards)| {
        let empties = *empties;
        let mut engine = Engine::new(model.clone(), EngineOptions::default().tt_mib(64))?;
        let value = |engine: &mut Engine, board: Board, goal: Goal| -> Result<i32> {
            let result = engine.search(board, &SearchRequest::new(goal))?;
            result.score.ok_or_else(|| "search gave no score".into())
        };
        let mut text = String::new();
        for &(index, board) in boards {
            let deepest = args.max_depth.min(empties - 1);
            let values = (0..=deepest)
                .map(|d| value(&mut engine, board, Goal::Depth(d)))
                .collect::<Result<Vec<_>>>()?;
            engine.clear_tt()?;
            let exact = if solved(empties) {
                let v = value(&mut engine, board, Goal::Endgame)?;
                engine.clear_tt()?;
                Some(v)
            } else {
                None
            };
            write_position(
                &mut text,
                &Position {
                    empties,
                    index,
                    board,
                    exact,
                    values,
                },
            )?;
        }
        {
            let mut file = file.lock().unwrap();
            file.write_all(text.as_bytes())?;
            file.flush()?;
        }
        let mut finished = finished.lock().unwrap();
        *finished += 1;
        if finished.is_multiple_of(50) || *finished == jobs.len() {
            println!("{}/{} jobs", *finished, jobs.len());
        }
        Ok(())
    })
}

/// The pairs the reference rules use: each midgame depth with its table
/// shallow depth, and each exact score with the main-branch shallow depth.
fn pairs(positions: &[Position]) -> Vec<Sample> {
    let mut samples = Vec::new();
    for p in positions {
        if let Some(exact) = p.exact {
            let shallow_depth = exact_shallow(p.empties);
            if let Some(&shallow) = p.values.get(usize::from(shallow_depth)) {
                samples.push(Sample {
                    exact: true,
                    empties: p.empties,
                    depth: p.empties,
                    shallow_depth,
                    shallow,
                    deep: exact,
                });
            }
        }
        for depth in 4..p.values.len() as u8 {
            let shallow_depth = SHALLOW_BY_DEPTH[usize::from(depth)];
            samples.push(Sample {
                exact: false,
                empties: p.empties,
                depth,
                shallow_depth,
                shallow: p.values[usize::from(shallow_depth)],
                deep: p.values[usize::from(depth)],
            });
        }
    }
    samples.sort_by_key(|s| (s.exact, s.empties, s.depth));
    samples
}

const HEADER: &str = "empties\tindex\tplayer\topponent\texact\tvalues";

fn write_position(text: &mut String, p: &Position) -> std::fmt::Result {
    let exact = p.exact.map_or("-".to_string(), |v| v.to_string());
    let values: Vec<String> = p.values.iter().map(i32::to_string).collect();
    writeln!(
        text,
        "{}\t{}\t{:016x}\t{:016x}\t{exact}\t{}",
        p.empties,
        p.index,
        p.board.player_bits(),
        p.board.opponent_bits(),
        values.join(",")
    )
}

/// The settings line and the positions, in a fixed order whatever the
/// order the jobs finished in. A torn last line (an interrupted write) is
/// skipped.
fn read_positions(path: &Path) -> Result<(String, Vec<Position>)> {
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines();
    let settings = lines.next().unwrap_or_default().to_string();
    if !settings.starts_with("# ") || lines.next() != Some(HEADER) {
        return Err(format!("{}: not a positions file", path.display()).into());
    }
    let complete = text.ends_with('\n');
    let lines: Vec<&str> = lines.collect();
    let count = lines.len();
    let mut positions = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        match parse_position(line) {
            Some(p) => positions.push(p),
            None if i + 1 == count && !complete => {}
            None => return Err(format!("{}: bad line `{line}`", path.display()).into()),
        }
    }
    positions.sort_by_key(|p| (p.empties, p.index));
    positions.dedup_by_key(|p| (p.empties, p.index));
    Ok((settings, positions))
}

fn parse_position(line: &str) -> Option<Position> {
    let f: Vec<&str> = line.split('\t').collect();
    let [empties, index, player, opponent, exact, values] = f[..] else {
        return None;
    };
    let board = Board::from_bits(
        u64::from_str_radix(player, 16).ok()?,
        u64::from_str_radix(opponent, 16).ok()?,
    )?;
    let values: Vec<i32> = values
        .split(',')
        .map(|v| v.parse().ok())
        .collect::<Option<_>>()?;
    let empties: u8 = empties.parse().ok()?;
    if board.empty_count() != empties || values.is_empty() {
        return None;
    }
    Some(Position {
        empties,
        index: index.parse().ok()?,
        board,
        exact: match exact {
            "-" => None,
            v => Some(v.parse().ok()?),
        },
        values,
    })
}

/// Least-squares line through one cell's samples and the standard deviation
/// of its residuals.
#[derive(Clone, Copy, Debug)]
struct Cell {
    exact: bool,
    empties: u8,
    depth: u8,
    shallow_depth: u8,
    samples: usize,
    slope: f64,
    intercept: f64,
    deviation: f64,
}

fn cell(samples: &[Sample]) -> Option<Cell> {
    let n = samples.len() as f64;
    if samples.len() < 8 {
        return None;
    }
    let mx = samples.iter().map(|s| f64::from(s.shallow)).sum::<f64>() / n;
    let my = samples.iter().map(|s| f64::from(s.deep)).sum::<f64>() / n;
    let sxx: f64 = samples
        .iter()
        .map(|s| (f64::from(s.shallow) - mx).powi(2))
        .sum();
    let sxy: f64 = samples
        .iter()
        .map(|s| (f64::from(s.shallow) - mx) * (f64::from(s.deep) - my))
        .sum();
    if sxx == 0.0 {
        return None;
    }
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let residuals: f64 = samples
        .iter()
        .map(|s| (f64::from(s.deep) - slope * f64::from(s.shallow) - intercept).powi(2))
        .sum();
    let s = samples[0];
    Some(Cell {
        exact: s.exact,
        empties: s.empties,
        depth: s.depth,
        shallow_depth: s.shallow_depth,
        samples: samples.len(),
        slope,
        intercept,
        deviation: (residuals / (n - 2.0)).sqrt(),
    })
}

/// Least squares `x · β ≈ y` by the normal equations.
fn least_squares(x: &[Vec<f64>], y: &[f64]) -> Option<Vec<f64>> {
    let k = x[0].len();
    let mut a = vec![vec![0.0; k + 1]; k];
    for (row, &target) in x.iter().zip(y) {
        for i in 0..k {
            for j in 0..k {
                a[i][j] += row[i] * row[j];
            }
            a[i][k] += row[i] * target;
        }
    }
    for c in 0..k {
        let pivot = (c..k).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        a.swap(c, pivot);
        if a[c][c].abs() < 1e-12 {
            return None;
        }
        let pivot_row = a[c].clone();
        for (r, row) in a.iter_mut().enumerate() {
            if r != c {
                let f = row[c] / pivot_row[c];
                for (x, p) in row[c..].iter_mut().zip(&pivot_row[c..]) {
                    *x -= f * p;
                }
            }
        }
    }
    Some((0..k).map(|i| a[i][k] / a[i][i]).collect())
}

fn fit(samples: &[Sample]) -> Result<(Vec<Rule>, String)> {
    let cells: Vec<Cell> = samples
        .chunk_by(|a, b| (a.exact, a.empties, a.depth) == (b.exact, b.empties, b.depth))
        .filter_map(cell)
        .collect();
    let (exact, midgame): (Vec<Cell>, Vec<Cell>) = cells.iter().partition(|c| c.exact);
    if midgame.len() < 8 || exact.is_empty() {
        return Err("too few measured cells to fit".into());
    }
    // Midgame: the reference regression in (empties, depth, shallow depth).
    let features = |e: f64, d: f64, s: f64| vec![1.0, e, d, s];
    let x: Vec<Vec<f64>> = midgame
        .iter()
        .map(|c| features(c.empties.into(), c.depth.into(), c.shallow_depth.into()))
        .collect();
    let coefficients = |f: fn(&Cell) -> f64| -> Result<Vec<f64>> {
        let y: Vec<f64> = midgame.iter().map(f).collect();
        least_squares(&x, &y).ok_or_else(|| "midgame regression is singular".into())
    };
    let slope = coefficients(|c| c.slope)?;
    let intercept = coefficients(|c| c.intercept)?;
    let deviation = coefficients(|c| c.deviation)?;
    let dot = |b: &[f64], f: &[f64]| b.iter().zip(f).map(|(b, f)| b * f).sum::<f64>();
    // Keep extrapolated deviations within the measured range.
    let (low, high) = midgame.iter().fold((f64::MAX, 0.0f64), |(lo, hi), c| {
        (lo.min(c.deviation), hi.max(c.deviation))
    });
    let mut rules = Vec::new();
    for depth in 4..=60u8 {
        let shallow_depth = SHALLOW_BY_DEPTH[usize::from(depth)];
        for empties in depth..=60u8 {
            let f = features(empties.into(), depth.into(), shallow_depth.into());
            let rule = Rule {
                exact: false,
                depth,
                empties,
                shallow_depth,
                slope: dot(&slope, &f),
                intercept: dot(&intercept, &f),
                deviation: dot(&deviation, &f).clamp(low, high),
            };
            if rule.slope > 0.0 {
                rules.push(rule);
            }
        }
    }
    // Endgame: the measured counts, then the last measured of each parity.
    let last = |parity: u8| exact.iter().rev().find(|c| c.empties % 2 == parity);
    for empties in 12..=60u8 {
        let measured = exact.iter().find(|c| c.empties == empties);
        let Some(c) = measured.or_else(|| last(empties % 2).filter(|c| c.empties < empties)) else {
            continue;
        };
        rules.push(Rule {
            exact: true,
            depth: empties,
            empties,
            shallow_depth: exact_shallow(empties),
            slope: c.slope,
            intercept: c.intercept,
            deviation: c.deviation,
        });
    }

    let mut report = String::from(
        "exact\tempties\tdepth\tshallow_depth\tsamples\tslope\tintercept\tdeviation\tfit_slope\tfit_intercept\tfit_deviation\n",
    );
    for c in &cells {
        let fitted = rules
            .iter()
            .find(|r| (r.exact, r.depth, r.empties) == (c.exact, c.depth, c.empties));
        let (a, b, e) = fitted.map_or((f64::NAN, f64::NAN, f64::NAN), |r| {
            (r.slope, r.intercept, r.deviation)
        });
        writeln!(
            report,
            "{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}",
            u8::from(c.exact),
            c.empties,
            c.depth,
            c.shallow_depth,
            c.samples,
            c.slope,
            c.intercept,
            c.deviation,
            a,
            b,
            e
        )?;
    }
    writeln!(
        report,
        "# midgame regression [1, empties, depth, shallow_depth]: slope {slope:?} intercept {intercept:?} deviation {deviation:?}"
    )?;
    Ok((rules, report))
}

/// The `p` quantile of sorted values, interpolating between neighbours.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let x = p * (sorted.len() - 1) as f64;
    let (i, f) = (x.floor() as usize, x.fract());
    let next = sorted[(i + 1).min(sorted.len() - 1)];
    sorted[i] * (1.0 - f) + next * f
}

/// The multiples of the deviation at which each stage really cuts with its
/// probability: the quantiles of |deep - predicted| / deviation over the
/// measured pairs, for midgame and exact rules separately. The normal
/// multiples assume normal errors; the measured errors have heavier tails.
/// Also reports how often the errors pass each multiple, by empties band.
fn tail_sigmas(samples: &[Sample], rules: &[Rule]) -> Result<(Sigmas, String)> {
    let rule = |s: &Sample| {
        rules
            .iter()
            .find(|r| (r.exact, r.depth, r.empties) == (s.exact, s.depth, s.empties))
    };
    let z = |s: &Sample, r: &Rule| {
        (f64::from(s.deep) - r.slope * f64::from(s.shallow) - r.intercept).abs() / r.deviation
    };
    let scores = |exact: bool, band: (u8, u8)| -> Vec<f64> {
        let mut v: Vec<f64> = samples
            .iter()
            .filter(|s| s.exact == exact && (band.0..=band.1).contains(&s.empties))
            .filter_map(|s| rule(s).filter(|r| r.deviation > 0.0).map(|r| z(s, r)))
            .collect();
        v.sort_by(f64::total_cmp);
        v
    };
    let measured = |exact: bool| -> Result<[f64; 6]> {
        let all = scores(exact, (0, 60));
        if all.len() < 100 {
            return Err(format!("too few pairs to measure the tails (exact: {exact})").into());
        }
        let mut t = STAGE_PROBABILITIES.map(|p| quantile(&all, p));
        for i in 1..6 {
            t[i] = t[i].max(t[i - 1]);
        }
        Ok(t)
    };
    let sigmas = Sigmas {
        midgame: measured(false)?,
        exact: measured(true)?,
    };
    let normal = ProbCutSigmas::NORMAL.midgame;
    let mut report = String::from(
        "exact\tempties\tpairs\tprobability\tnormal_sigma\tbeyond_normal\tsigma\tbeyond_sigma\n",
    );
    let bands: &[(u8, u8)] = &[(0, 60), (12, 24), (25, 36), (37, 52)];
    for exact in [false, true] {
        let t = if exact { sigmas.exact } else { sigmas.midgame };
        for &band in bands {
            let v = scores(exact, band);
            if v.is_empty() {
                continue;
            }
            let beyond = |x: f64| v.iter().filter(|&&z| z > x).count() as f64 / v.len() as f64;
            for i in 0..6 {
                writeln!(
                    report,
                    "{}\t{}..={}\t{}\t{:.2}\t{:.4}\t{:.4}\t{:.4}\t{:.4}",
                    u8::from(exact),
                    band.0,
                    band.1,
                    v.len(),
                    STAGE_PROBABILITIES[i],
                    normal[i],
                    beyond(normal[i]),
                    t[i],
                    beyond(t[i])
                )?;
            }
        }
    }
    Ok((sigmas, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_recover_a_line_and_regression_solves() {
        let samples: Vec<Sample> = (0..40)
            .map(|i| Sample {
                exact: true,
                empties: 20,
                depth: 20,
                shallow_depth: 6,
                shallow: i - 20,
                deep: 2 * (i - 20) + 3 + if i % 2 == 0 { 1 } else { -1 },
            })
            .collect();
        let c = cell(&samples).unwrap();
        assert!((c.slope - 2.0).abs() < 0.01 && (c.intercept - 3.0).abs() < 0.1);
        assert!((c.deviation - 1.0).abs() < 0.05);
        let x = vec![vec![1.0, 0.0], vec![1.0, 1.0], vec![1.0, 2.0]];
        let b = least_squares(&x, &[1.0, 3.0, 5.0]).unwrap();
        assert!((b[0] - 1.0).abs() < 1e-9 && (b[1] - 2.0).abs() < 1e-9);
        assert_eq!(
            (
                exact_shallow(12),
                exact_shallow(25),
                exact_shallow(29),
                exact_shallow(30)
            ),
            (4, 5, 7, 8)
        );
        assert!("12..=24".parse::<EmptiesRange>().is_ok());
        assert!("0..3".parse::<EmptiesRange>().is_err());
    }

    #[test]
    fn tail_sigmas_are_the_quantiles_of_standardized_errors() {
        // Errors -50..=50 with deviation 10: |z| is uniform on 0..=5.
        let rule = Rule {
            exact: false,
            depth: 8,
            empties: 30,
            shallow_depth: 2,
            slope: 1.0,
            intercept: 0.0,
            deviation: 10.0,
        };
        let exact = Rule {
            exact: true,
            depth: 20,
            empties: 20,
            shallow_depth: 6,
            ..rule
        };
        let samples: Vec<Sample> = (-50..=50)
            .flat_map(|e| {
                [rule, exact].map(|r| Sample {
                    exact: r.exact,
                    empties: r.empties,
                    depth: r.depth,
                    shallow_depth: r.shallow_depth,
                    shallow: 0,
                    deep: e,
                })
            })
            .collect();
        let (sigmas, report) = tail_sigmas(&samples, &[rule, exact]).unwrap();
        for (t, p) in sigmas.midgame.iter().zip(STAGE_PROBABILITIES) {
            assert!((t - 5.0 * p).abs() < 0.06, "{t} {p}");
        }
        assert_eq!(sigmas.midgame, sigmas.exact);
        assert!(report.lines().count() > 12);
        assert_eq!(quantile(&[1.0, 2.0, 3.0], 0.5), 2.0);
        assert_eq!(quantile(&[1.0, 2.0], 0.25), 1.25);
    }

    #[test]
    fn positions_round_trip_and_a_torn_last_line_is_skipped() {
        let p = Position {
            empties: 60,
            index: 7,
            board: Board::new(),
            exact: Some(-4),
            values: vec![0, 3, -1],
        };
        let q = Position {
            index: 2,
            exact: None,
            ..p.clone()
        };
        let mut text = format!("# settings\n{HEADER}\n");
        write_position(&mut text, &p).unwrap();
        write_position(&mut text, &q).unwrap();
        write_position(&mut text, &p).unwrap();
        let dir = std::env::temp_dir().join(format!("mpc-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("positions.tsv");
        fs::write(&path, format!("{text}60\t9\t00")).unwrap();
        let (settings, read) = read_positions(&path).unwrap();
        assert_eq!(settings, "# settings");
        assert_eq!(read, vec![q, p]);
        fs::write(&path, format!("{text}60\t9\t00\n")).unwrap();
        assert!(read_positions(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
