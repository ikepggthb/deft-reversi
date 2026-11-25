//! モデルの Multi-ProbCut（MPC）パラメータを調整（校正）するモジュール。
//!
//! ProbCut（MPC）はまず「浅い探索」を行って「深い探索」のスコアを一次式
//! `deep ≈ slope * shallow + intercept` で予測し、その予測値が探索窓（α-β 窓）から
//! `sigma * deviation` 以上外れていて見込みがないと判断した手を早めに切り落とす（枝刈りする）手法である。
//! 予測の傾き・切片・ばらつき（`deviation`）は評価関数モデルの精度によって変わるため、
//! モデルごとに実測データから校正する必要がある。
//!
//! - **中盤ルール**（空きマス `empties`、探索深さ `depth`）: 深さ `4..=max_depth` を実際に探索して計測し、
//!   `(empties, depth, shallow_depth)` を説明変数とする線形回帰モデルを当てはめて全深さへ外挿する。
//! - **終盤ルール**（完全読み）: 実測した完全読みスコアと浅い深さ `s` の評価値ペアから、
//!   浅い深さ `s` の偶奇別に空きマス `e` と浅い深さ `s` の数式（[`ExactFormula`]）をフィットする。
//!   `slope` と `intercept` は `(e, s)` の一次関数、
//!   `deviation² = (c0 + c1·s + c2·s²)·(1 − exp(−(e − s)/λ))` としてフィッティングする。
//!   これにより実測範囲内の任意の浅い深さに適用でき、[`exact_shallow`] で適切な浅い深さを選べる。
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

/// `mpc` サブコマンドの CLI 引数。
#[derive(Args, Debug)]
pub struct MpcArgs {
    /// 校正対象のモデルファイル（エンジンが読み込める任意の形式）。
    #[arg(short, long)]
    model: PathBuf,
    /// データセットディレクトリ（`--split` から局面を取得）。`--fit-only` 時は不要。
    #[arg(long, required_unless_present = "fit_only")]
    data: Option<PathBuf>,
    /// 局面を取得するデータセット分割名。
    #[arg(long, default_value = "valid")]
    split: String,
    /// `positions.tsv`, `cells.tsv`, `tails.tsv`, `rules.json`, `model.dpr2`
    /// （校正パラメータを埋め込んだモデル）の出力先ディレクトリ。
    #[arg(long)]
    out: PathBuf,
    /// 空きマス数ごとの計測局面数。
    #[arg(long, default_value_t = 64)]
    positions: usize,
    /// 計測対象の空きマス数範囲（両端含む）。
    #[arg(long, default_value = "12..=52")]
    empties: EmptiesRange,
    /// 実測する最大探索深さ（`empties - 1` でも上限制限される）。
    #[arg(long, default_value_t = 14)]
    max_depth: u8,
    /// 完全読みも併せて実測する空きマス数範囲（両端含む）。
    #[arg(long, default_value = "12..=24")]
    exact_empties: EmptiesRange,
    /// 探索を行わず、既存の `--out/positions.tsv` からルールのみを再フィッティングする。
    #[arg(long)]
    fit_only: bool,
    /// 局面抽出の乱数シード。
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// 並列計測スレッド数。
    #[arg(short, long)]
    threads: Option<usize>,
}

/// 空きマス数範囲 `a..=b`（または両端含む `a..b`、単一値 `a`）。有効範囲は `1..=60`。
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

/// 旧 Deft に基づく、各中盤深さに対応する浅い探索深さテーブル。
const SHALLOW_BY_DEPTH: [u8; 61] = [
    0, 0, 0, 0, 0, 1, 2, 1, 2, 1, 2, 3, 4, 3, 4, 5, 6, 5, 6, 5, 6, 5, 6, 5, 6, 7, 8, 7, 8, 7, 8, 7,
    8, 7, 8, 7, 8, 7, 8, 7, 8, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9,
    10,
];

/// FFO 40-59 の解決時間ベンチマークに基づいて選定された、終盤完全読み用の浅い探索深さ。
fn exact_shallow(empties: u8) -> u8 {
    match empties {
        ..=15 => 1,
        16 => 4,
        17 | 19 => 5,
        18 => 6,
        e => 8 - e % 2,
    }
}

/// 実測された浅い探索と深い探索の評価値ペア 1 件。
#[derive(Clone, Copy, Debug)]
struct Sample {
    exact: bool,
    empties: u8,
    depth: u8,
    shallow_depth: u8,
    shallow: i32,
    deep: i32,
}

/// `rules.json` に書き出される 1 件の ProbCut ルール。
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

/// `rules.json` に書き出されるエンジンの選択性ステージ（[`ProbCutSigmas::STAGES`]、精度低い順）ごとの偏差倍率。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sigmas {
    pub midgame: [f64; 5],
    pub exact: [f64; 5],
}

impl From<ProbCutSigmas> for Sigmas {
    fn from(s: ProbCutSigmas) -> Self {
        Sigmas {
            midgame: s.midgame,
            exact: s.exact,
        }
    }
}

impl From<Sigmas> for ProbCutSigmas {
    fn from(s: Sigmas) -> Self {
        ProbCutSigmas {
            midgame: s.midgame,
            exact: s.exact,
        }
    }
}

/// `rules.json` の全体構造。`stages` に各倍率の信頼確率（%）を記録し、
/// 異なるステージ構成向けに校正されたファイルの誤読込を防止する。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Calibration {
    pub stages: Vec<u8>,
    pub sigmas: Sigmas,
    pub rules: Vec<Rule>,
}

/// エンジンの各選択性ステージの信頼確率（%）。
fn stage_percents() -> Vec<u8> {
    ProbCutSigmas::STAGES.iter().map(|s| s.percent()).collect()
}

/// エンジンの各選択性ステージの信頼確率（0..=1）。
fn stage_probabilities() -> [f64; 5] {
    ProbCutSigmas::STAGES.map(|s| f64::from(s.percent()) / 100.0)
}

/// ソート済み `values` から各ステージ確率の分位点を求め、単調非減少に補正する。
fn stage_quantiles(sorted: &[f64]) -> [f64; 5] {
    let mut t = stage_probabilities().map(|p| quantile(sorted, p));
    for i in 1..t.len() {
        t[i] = t[i].max(t[i - 1]);
    }
    t
}

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

/// `pack` サブコマンドの CLI 引数。
#[derive(Args, Debug)]
pub struct PackArgs {
    /// 重みを保持する入力モデル（任意の DPR2 バージョン）。
    #[arg(short, long)]
    model: PathBuf,
    /// `mpc` サブコマンドが生成した校正ファイル（`rules.json`）。
    #[arg(long, conflicts_with = "legacy_main")]
    rules: Option<PathBuf>,
    /// main ブランチ由来のレガシールールを埋め込む（旧モデル変換用）。
    #[arg(long)]
    legacy_main: bool,
    /// 校正パラメータ付きモデルの出力パス（DPR2 version 2）。
    #[arg(short, long)]
    out: PathBuf,
}

/// モデルと ProbCut 校正パラメータを 1 ファイルに統合して書き出す。
pub fn pack(args: PackArgs) -> Result<()> {
    let model = Model::weights_from_file(&args.model)?;
    let model = match (&args.rules, args.legacy_main) {
        (Some(path), false) => {
            let c: Calibration = serde_json::from_slice(&fs::read(path)?).map_err(|e| {
                format!(
                    "{}: {e}; refit it with `train mpc --fit-only`",
                    path.display()
                )
            })?;
            if c.stages != stage_percents() {
                return Err(format!(
                    "{}: multiples for the stages {:?}, the engine has {:?}; \
                     refit it with `train mpc --fit-only`",
                    path.display(),
                    c.stages,
                    stage_percents()
                )
                .into());
            }
            model.with_calibration(
                c.rules.into_iter().map(Into::into).collect(),
                c.sigmas.into(),
            )?
        }
        (None, true) => model.with_calibration(
            deft_reversi_engine::legacy_main_rules(),
            ProbCutSigmas::NORMAL,
        )?,
        _ => return Err("give --rules or --legacy-main".into()),
    };
    let mut bytes = Vec::new();
    model.write_to(&mut bytes)?;
    fs::write(&args.out, bytes)?;
    println!("{} written with its calibration", args.out.display());
    Ok(())
}

/// `mpc` サブコマンドを実行し、局面探索計測・回帰フィッティング・テール倍率算出・モデル書き出しを行う。
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
    let formula = ExactFormula::fit(&positions)?;
    let (rules, mut report) = fit(&samples, &formula)?;
    report.push_str(&formula.describe());
    let exact_multiples = formula.multiples(&positions)?;
    let (sigmas, tails) = tail_sigmas(&samples, &rules, Some(exact_multiples))?;
    // The model checks the calibration as the engine does.
    let model = Model::weights_from_file(&args.model)?.with_calibration(
        rules.iter().copied().map(Into::into).collect(),
        sigmas.into(),
    )?;
    let mut packed = Vec::new();
    model.write_to(&mut packed)?;
    fs::write(args.out.join("model.dpr2"), packed)?;
    fs::write(args.out.join("cells.tsv"), report)?;
    fs::write(args.out.join("tails.tsv"), tails)?;
    fs::write(
        args.out.join("rules.json"),
        serde_json::to_string_pretty(&Calibration {
            stages: stage_percents(),
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

/// 1 局面分の計測結果：深さ `0..values.len()` の全幅探索値および（完全読み対象なら）完全読みスコア。
/// `index` は空きマス数ごとのシード付きシャッフル順における位置であり、
/// `--positions` を増やして再実行した際に既存の選択を拡張できるようになっている。
#[derive(Clone, Debug, PartialEq)]
struct Position {
    empties: u8,
    index: usize,
    board: Board,
    exact: Option<i32>,
    values: Vec<i32>,
}

/// 計測条件ヘッダ行を生成する（中断後の再開時に同一条件であることを検証するため）。
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

/// `path` に未計測の選択局面をすべて計測し、ジョブ完了ごとに追記する（中断時のレジューム対応）。
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
    let data = args.data.as_ref().ok_or("--data is required to measure")?;
    let model = Model::weights_from_file(&args.model)?;
    let mut jobs: Vec<(u8, Vec<(usize, Board)>)> = Vec::new();
    for empties in args.empties.first..=args.empties.last {
        let stones = 64 - empties;
        let path = rd::path(data, stones, &args.split);
        let mut boards: Vec<Board> = rd::read(&path, stones)?
            .into_iter()
            .map(|r| r.board)
            .filter(|b| !b.is_terminal() && !b.must_pass())
            .collect();
        // 空きマス数ごとに独立してシャッフルし、ある空きマス数の選択が他に影響しないようにする。
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
        // 小さなチャンクに分割し、終盤まで全スレッドが遊ばないようにする。
        for chunk in todo.chunks(4) {
            jobs.push((empties, chunk.to_vec()));
        }
    }
    let solved =
        |empties: u8| (args.exact_empties.first..=args.exact_empties.last).contains(&empties);
    // 最も重いジョブ（空きマスの多い完全読み → 深い探索）から先に投入する。
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
    // スレッドごとに 1 つの Engine をプールして使い回す：
    // 置換表エントリは同一深さ・同一選択性の探索のみをカットするため他局面の残留エントリは結果を変えず、
    // 局面ごとに 512 MiB の置換表を確保し直すコストを避けられる。
    let engines: Mutex<Vec<Engine>> = Mutex::new(Vec::new());
    for_each_parallel(&jobs, threads, |(empties, boards)| {
        let empties = *empties;
        let pooled = engines.lock().unwrap().pop();
        let mut engine = match pooled {
            Some(engine) => engine,
            None => Engine::new(model.clone(), EngineOptions::default().tt_mib(512))?,
        };
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
            let exact = if solved(empties) {
                Some(value(&mut engine, board, Goal::Endgame)?)
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
        engines.lock().unwrap().push(engine);
        let mut finished = finished.lock().unwrap();
        *finished += 1;
        if finished.is_multiple_of(50) || *finished == jobs.len() {
            println!("{}/{} jobs", *finished, jobs.len());
        }
        Ok(())
    })
}

/// 参照ルールが用いる `(浅い探索値, 深い探索値)` ペアを抽出する：
/// 中盤深さはテーブル対応する浅い深さ、完全読みは [`exact_shallow`] で決まる浅い深さと組にする。
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

/// 設定行と局面データを（ジョブ完了順に依存しない固定順で）読み込む。
/// 書き込み中断による不完全な最終行は読み飛ばす。
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

/// 1 セル（同一 `(exact, empties, depth)`）のサンプル群に対する最小二乗直線と残差標準偏差。
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

/// 正規方程式をガウス・ジョルダン法で解き、最小二乗解 `x · β ≈ y` を求める。
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

fn fit(samples: &[Sample], formula: &ExactFormula) -> Result<(Vec<Rule>, String)> {
    let cells: Vec<Cell> = samples
        .chunk_by(|a, b| (a.exact, a.empties, a.depth) == (b.exact, b.empties, b.depth))
        .filter_map(cell)
        .collect();
    let (exact, midgame): (Vec<Cell>, Vec<Cell>) = cells.iter().partition(|c| c.exact);
    if midgame.len() < 8 || exact.is_empty() {
        return Err("too few measured cells to fit".into());
    }
    // 中盤：`(empties, depth, shallow_depth)` に対する参照線形回帰。
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
    // 外挿された標準偏差が実測範囲から極端に外れないようクランプする。
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
    // Endgame: the formula at the table's shallow depths.
    rules.extend((12..=60u8).map(|e| formula.rule(e, exact_shallow(e))));

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

/// ソート済み配列 `sorted` の `p` 分位点を隣接要素間の線形補間で求める。
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let x = p * (sorted.len() - 1) as f64;
    let (i, f) = (x.floor() as usize, x.fract());
    let next = sorted[(i + 1).min(sorted.len() - 1)];
    sorted[i] * (1.0 - f) + next * f
}

/// 各選択性ステージで所定の確率通りに枝刈りを行うための偏差倍率（`sigma`）を求める。
///
/// 実際の探索誤差は正規分布よりも裾が重いため、理論値ではなく実測ペア上の標準化残差
/// `|deep - predicted| / deviation` の分位点を中盤・終盤別に算出する。
/// あわせて、空きマス帯域ごとの超過率レポートも生成する。
fn tail_sigmas(
    samples: &[Sample],
    rules: &[Rule],
    exact_multiples: Option<[f64; 5]>,
) -> Result<(Sigmas, String)> {
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
    let measured = |exact: bool| -> Result<[f64; 5]> {
        let all = scores(exact, (0, 60));
        if all.len() < 100 {
            return Err(format!("too few pairs to measure the tails (exact: {exact})").into());
        }
        Ok(stage_quantiles(&all))
    };
    let sigmas = Sigmas {
        midgame: measured(false)?,
        exact: match exact_multiples {
            Some(t) => t,
            None => measured(true)?,
        },
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
            for (i, probability) in stage_probabilities().into_iter().enumerate() {
                writeln!(
                    report,
                    "{}\t{}..={}\t{}\t{:.2}\t{:.4}\t{:.4}\t{:.4}\t{:.4}",
                    u8::from(exact),
                    band.0,
                    band.1,
                    v.len(),
                    probability,
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

/// 空きマス `e` と浅い深さ `s` による終盤完全読みルール式（`s` の偶奇別）。
/// 奇数深さの探索値は「手番側が評価誤差の乗った最善子ノードを選ぶ」ことで生じる未相殺のバイアスを含むため、
/// 偶数深さとは `s` に対する傾きの向きが逆になる。そのため偶奇で別の式をフィットする。
#[derive(Clone, Copy, Debug)]
struct ExactFormula {
    parity: [ParityFit; 2],
    /// 実測した最大の空きマス数。これより大きい空きマスでは傾き・切片をこの値で頭打ちにする。
    last_empties: u8,
    /// 実測した最大の浅い深さ。
    max_shallow: u8,
}

#[derive(Clone, Copy, Debug, Default)]
struct ParityFit {
    slope: [f64; 3],
    intercept: [f64; 3],
    /// `deviation² = (c0 + c1·s + c2·s²)·(1 − exp(−(e − s)/lambda))`。
    deviation: [f64; 3],
    lambda: f64,
}

/// ルールが必要としうる最小の残り深さ（プライ）。これ未満のセルは除外する。
const EXACT_MIN_REMAINING: u8 = 4;

impl ParityFit {
    fn slope(&self, e: f64, s: f64) -> f64 {
        self.slope[0] + self.slope[1] * e + self.slope[2] * s
    }
    fn intercept(&self, e: f64, s: f64) -> f64 {
        self.intercept[0] + self.intercept[1] * e + self.intercept[2] * s
    }
    fn deviation(&self, e: f64, s: f64) -> f64 {
        let c = self.deviation;
        ((c[0] + c[1] * s + c[2] * s * s) * saturation(e - s, self.lambda))
            .max(1e-9)
            .sqrt()
    }
}

fn saturation(remaining: f64, lambda: f64) -> f64 {
    1.0 - (-remaining / lambda).exp()
}

/// 各行に重み `w` を付けた重み付き最小二乗法 `x · β ≈ y`。
fn weighted_least_squares(x: &[Vec<f64>], y: &[f64], w: &[f64]) -> Option<Vec<f64>> {
    let xs: Vec<Vec<f64>> = x
        .iter()
        .zip(w)
        .map(|(row, w)| row.iter().map(|v| v * w.sqrt()).collect())
        .collect();
    let ys: Vec<f64> = y.iter().zip(w).map(|(y, w)| y * w.sqrt()).collect();
    least_squares(&xs, &ys)
}

/// 完全読み済みの全空きマス数および十分な残り深さを持つ全浅い深さ `s` について、
/// 完全読みスコアを深さ `s` 評価値に回帰した [`Cell`] 一覧を生成する。
fn exact_cells(positions: &[Position]) -> Vec<Cell> {
    let mut by_empties: Vec<(u8, Vec<&Position>)> = Vec::new();
    for p in positions.iter().filter(|p| p.exact.is_some()) {
        match by_empties.iter_mut().find(|(e, _)| *e == p.empties) {
            Some((_, v)) => v.push(p),
            None => by_empties.push((p.empties, vec![p])),
        }
    }
    by_empties.sort_by_key(|(e, _)| *e);
    let mut cells = Vec::new();
    for (e, group) in by_empties {
        let depths = group.iter().map(|p| p.values.len()).min().unwrap_or(0);
        for s in 0..depths as u8 {
            if s + EXACT_MIN_REMAINING > e {
                break;
            }
            let samples: Vec<Sample> = group
                .iter()
                .map(|p| Sample {
                    exact: true,
                    empties: e,
                    depth: e,
                    shallow_depth: s,
                    shallow: p.values[usize::from(s)],
                    deep: p.exact.unwrap(),
                })
                .collect();
            cells.extend(cell(&samples));
        }
    }
    cells
}

impl ExactFormula {
    fn fit(positions: &[Position]) -> Result<Self> {
        let cells = exact_cells(positions);
        let mut parity = [ParityFit::default(); 2];
        for (par, fit) in parity.iter_mut().enumerate() {
            let cells: Vec<&Cell> = cells
                .iter()
                .filter(|c| usize::from(c.shallow_depth % 2) == par)
                .collect();
            if cells.len() < 6 {
                return Err("too few exact cells to fit the endgame formula".into());
            }
            let w: Vec<f64> = cells.iter().map(|c| c.samples as f64).collect();
            let x: Vec<Vec<f64>> = cells
                .iter()
                .map(|c| vec![1.0, c.empties.into(), c.shallow_depth.into()])
                .collect();
            let linear = |f: fn(&Cell) -> f64| -> Result<[f64; 3]> {
                let y: Vec<f64> = cells.iter().map(|c| f(c)).collect();
                weighted_least_squares(&x, &y, &w)
                    .map(|b| [b[0], b[1], b[2]])
                    .ok_or_else(|| "endgame regression is singular".into())
            };
            fit.slope = linear(|c| c.slope)?;
            fit.intercept = linear(|c| c.intercept)?;
            // λ は対数等間隔グリッドで探索し、各 λ に対して deviation² の最小二乗係数を求めた上で
            // 実際の偏差 `deviation` の二乗誤差が最小となる λ を採用する。
            let mut best: Option<(f64, f64, [f64; 3])> = None;
            for i in 0..300 {
                let lambda = 0.3 * (200.0f64).powf(f64::from(i) / 299.0);
                let x: Vec<Vec<f64>> = cells
                    .iter()
                    .map(|c| {
                        let (e, s) = (f64::from(c.empties), f64::from(c.shallow_depth));
                        let h = saturation(e - s, lambda);
                        vec![h, s * h, s * s * h]
                    })
                    .collect();
                let y: Vec<f64> = cells.iter().map(|c| c.deviation * c.deviation).collect();
                let Some(b) = weighted_least_squares(&x, &y, &w) else {
                    continue;
                };
                let candidate = ParityFit {
                    deviation: [b[0], b[1], b[2]],
                    lambda,
                    ..*fit
                };
                let error: f64 = cells
                    .iter()
                    .zip(&w)
                    .map(|(c, w)| {
                        let d = candidate.deviation(c.empties.into(), c.shallow_depth.into());
                        w * (d - c.deviation).powi(2)
                    })
                    .sum();
                if best.is_none_or(|(e, _, _)| error < e) {
                    best = Some((error, lambda, candidate.deviation));
                }
            }
            let (_, lambda, deviation) = best.ok_or("endgame deviation fit failed")?;
            fit.lambda = lambda;
            fit.deviation = deviation;
        }
        Ok(Self {
            parity,
            last_empties: cells.iter().map(|c| c.empties).max().unwrap(),
            max_shallow: cells.iter().map(|c| c.shallow_depth).max().unwrap(),
        })
    }

    /// 空きマス `empties`・浅い深さ `shallow` の完全読みルールを生成する。
    /// 浅い深さは同じ偶奇を保ちつつ、実測範囲および最小残り深さ制約内に収まるよう調整する。
    fn rule(&self, empties: u8, shallow: u8) -> Rule {
        let mut s = shallow;
        while s > self.max_shallow || s + EXACT_MIN_REMAINING > empties {
            s -= 2;
        }
        let f = &self.parity[usize::from(s % 2)];
        let (e, sf) = (f64::from(empties), f64::from(s));
        let measured = f64::from(empties.min(self.last_empties));
        Rule {
            exact: true,
            depth: empties,
            empties,
            shallow_depth: s,
            slope: f.slope(measured, sf),
            intercept: f.intercept(measured, sf),
            deviation: f.deviation(e, sf),
        }
    }

    /// 終盤ステージの偏差倍率を算出する：任意の浅い深さテーブルに適用できるよう、
    /// すべての実測 `(empties, shallow_depth)` ペア上の標準化残差 `|z|` から分位点を求める。
    fn multiples(&self, positions: &[Position]) -> Result<[f64; 5]> {
        let mut z = Vec::new();
        for p in positions {
            let Some(exact) = p.exact else { continue };
            for (s, &v) in p.values.iter().enumerate() {
                let s = s as u8;
                if s + EXACT_MIN_REMAINING > p.empties {
                    break;
                }
                let f = &self.parity[usize::from(s % 2)];
                let (e, sf) = (f64::from(p.empties), f64::from(s));
                let predicted = f.slope(e, sf) * f64::from(v) + f.intercept(e, sf);
                z.push((f64::from(exact) - predicted).abs() / f.deviation(e, sf));
            }
        }
        if z.len() < 100 {
            return Err("too few exact pairs to measure the tails".into());
        }
        z.sort_by(f64::total_cmp);
        Ok(stage_quantiles(&z))
    }

    fn describe(&self) -> String {
        let mut out = String::new();
        for (par, f) in self.parity.iter().enumerate() {
            let _ = writeln!(
                out,
                "# exact formula, {} shallow depth: slope {:?} intercept {:?} (in [1, e, s], e capped at {}) deviation² ({:?})·(1-exp(-(e-s)/{:.4}))",
                if par == 0 { "even" } else { "odd" },
                f.slope,
                f.intercept,
                self.last_empties,
                f.deviation,
                f.lambda
            );
        }
        out
    }
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
                exact_shallow(18),
                exact_shallow(25),
                exact_shallow(30)
            ),
            (1, 6, 7, 8)
        );
        assert!("12..=24".parse::<EmptiesRange>().is_ok());
        assert!("0..3".parse::<EmptiesRange>().is_err());
    }

    #[test]
    fn calibrations_name_the_engine_stages() {
        assert_eq!(stage_percents(), [83, 85, 95, 98, 99]);
        let c = Calibration {
            stages: stage_percents(),
            sigmas: ProbCutSigmas::NORMAL.into(),
            rules: Vec::new(),
        };
        let text = serde_json::to_string(&c).unwrap();
        let back: Calibration = serde_json::from_str(&text).unwrap();
        assert_eq!(back.stages, c.stages);
        assert_eq!(back.sigmas, c.sigmas);
        // A file from before the stages were recorded (six multiples, no
        // `stages`) is not read.
        let old = r#"{"sigmas":{"midgame":[1,2,3,4,5,6],"exact":[1,2,3,4,5,6]},"rules":[]}"#;
        assert!(serde_json::from_str::<Calibration>(old).is_err());
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
        let (sigmas, report) = tail_sigmas(&samples, &[rule, exact], None).unwrap();
        for (t, p) in sigmas.midgame.iter().zip(stage_probabilities()) {
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
