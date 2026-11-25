//! 問題集ファイル（OBF 形式）を読み込み、各局面を探索して正答一致や探索性能を検証する `solve` コマンド。
//!
//! 人間向けの整形テーブル出力と、ベンチマーク計測向けの TSV（タブ区切り）出力の両方に対応する。

use crate::{EngineArgs, GoalArgs, Result, display, stop};
use clap::Args;
use deft_reversi_engine::{Action, Engine, Goal, Position, ResultKind, SearchResult, Selectivity};
use std::{path::PathBuf, time::Instant};

/// `solve` サブコマンドの引数。
#[derive(Args, Debug)]
pub struct SolveArgs {
    /// 問題集ファイル（1 行 1 局面: 64 マスと手番、続けて任意の正解例 `; A2:+38; C7:+36; ...`）。
    file: PathBuf,
    #[command(flatten)]
    engine: EngineArgs,
    /// 探索レベルや深さの指定（省略時は終局まで枝刈りなしの完全読み）。
    #[command(flatten)]
    pub(super) goal: GoalArgs,
    /// 各局面を探索する前に置換表（探索済み局面のハッシュ表）を消去する。
    #[arg(long)]
    clear_tt: bool,
    /// PV（予想手順）を省略せず全文表示し、選択的終盤探索で欠けた PV の復元も試みる。
    #[arg(long, conflicts_with = "tsv")]
    full_pv: bool,
    /// ベンチマーク集計向けにタブ区切り（TSV）で詳細結果を出力する。
    #[arg(long)]
    tsv: bool,
}

/// 問題集 1 問分のデータ。
pub struct Problem {
    /// 探索対象の局面。
    pub position: Position,
    /// ファイルに記載された候補手と期待石差の組（通常は最善手順）。
    pub answers: Vec<(Action, i32)>,
}

/// 問題集の 1 行を解析して [`Problem`] を返す。
pub fn parse(line: &str) -> std::result::Result<Problem, String> {
    let mut fields = line.split(';');
    let mut header = fields.next().ok_or("empty line")?.split_whitespace();
    let cells = header.next().ok_or("missing board")?;
    let side = header.next().ok_or("missing side to move")?;
    let position = format!("{cells} {side}")
        .parse::<Position>()
        .map_err(|e| e.to_string())?;
    // 旧形式の同梱ファイルは正解例を空白区切り、新しい OBF 形式は `;` 区切りで持つため両方を読み取る。
    let answers = header
        .chain(fields.map(str::trim).filter(|f| !f.is_empty()))
        .map(|field| {
            let (action, score) = field
                .split_once(':')
                .ok_or_else(|| format!("answer must be MOVE:SCORE: {field}"))?;
            let action = action.trim().parse::<Action>()?;
            let score = score.trim().parse::<i32>().map_err(|e| e.to_string())?;
            Ok((action, score))
        })
        .collect::<std::result::Result<_, String>>()?;
    Ok(Problem { position, answers })
}

/// 問題集ファイルを読み込み、空行と `#` コメント行を除いた [`Problem`] 一覧を返す。
pub fn read(path: &PathBuf) -> Result<Vec<Problem>> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|(i, line)| parse(line).map_err(|e| format!("line {}: {e}", i + 1).into()))
        .collect()
}

/// 探索結果 `result` が問題の正解（最高石差と、その石差に達する手のいずれか）と一致するかを返す。
///
/// 問題に正解が記載されていない場合は `None` を返す。
pub fn check(problem: &Problem, result: &SearchResult) -> Option<bool> {
    let best = problem.answers.iter().map(|a| a.1).max()?;
    let move_ok = problem
        .answers
        .iter()
        .any(|&(a, s)| Some(a) == result.best_move && s == best);
    Some(result.score == Some(best) && move_ok)
}

/// 問題集を順に探索し、結果表または TSV を標準出力へ出す。正解と食い違いがあればエラーを返す。
pub fn run(args: SolveArgs) -> Result<()> {
    let problems = read(&args.file)?;
    let mut engine = args.engine.engine()?;
    if problems.is_empty() {
        return Err("no positions in input file".into());
    }
    let goal = args.goal.goal(Goal::Endgame);
    crate::settings::report(&engine, &args.engine, goal, args.goal.time_limit());
    const KIND_WIDTH: usize = 8;
    let header = format!(
        "{:>4} | {:>5} | {:>4} | {:>7} | {:>KIND_WIDTH$} | {:>12} | {:>12} | {:>12} | {:>3} | pv",
        "#", "score", "move", "empties", "kind", "nodes", "nps", "time", "ok"
    );
    let rule = "-".repeat(header.len() + 20);
    if args.tsv {
        println!(
            "position\tscore\taction\tmode\tdepth\tnodes\tleaves\tsplits\tjobs\thandoffs\tjoin_seconds\tseconds\tstop"
        );
    } else {
        println!("{rule}\n{header}\n{rule}");
    }
    let (mut nodes, mut wrong, start) = (0, 0, Instant::now());
    let mut processed = 0;
    let mut incomplete = 0;
    for (i, problem) in problems.iter().enumerate() {
        if args.clear_tt {
            engine.clear_tt()?;
        }
        let result = search(
            &mut engine,
            problem.position,
            &args.goal,
            goal,
            args.full_pv,
        )?;
        if result.stop_reason == deft_reversi_engine::StopReason::WorkerFailed {
            return Err("engine search worker failed".into());
        }
        processed += 1;
        let interrupted = result.stop_reason != deft_reversi_engine::StopReason::Completed;
        incomplete += usize::from(interrupted);
        let verdict = match if interrupted {
            None
        } else {
            check(problem, &result)
        } {
            None if interrupted => "?",
            None => "",
            Some(true) => "ok",
            Some(false) => {
                wrong += 1;
                "NG"
            }
        };
        nodes += result.stats.nodes;
        if args.tsv {
            print_tsv(i + 1, problem.position, &result);
            if result.stop_reason == deft_reversi_engine::StopReason::Requested {
                break;
            }
            continue;
        }
        let pv = if args.full_pv {
            let mut pv = display::pv(&result.pv);
            if matches!(result.kind, Some(ResultKind::Endgame { .. })) {
                match result.pv_final_score(problem.position) {
                    None => pv.push_str(" [partial PV]"),
                    Some(score) if Some(score) != result.score => {
                        pv.push_str(&format!(" [PV score {score:+}]"));
                    }
                    Some(_) => {}
                }
            }
            pv
        } else {
            display::pv_prefix(&result.pv, 6)
        };
        println!(
            "{:>4} | {:>5} | {:>4} | {:>7} | {:>KIND_WIDTH$} | {:>12} | {:>12.0} | {:>12} | {:>3} | {}{}",
            i + 1,
            display::score(result.score),
            display::action(result.best_move),
            problem.position.empty_count(),
            display::kind(result.kind),
            result.stats.nodes,
            result.stats.nodes as f64 / result.elapsed.as_secs_f64(),
            display::duration(result.elapsed),
            verdict,
            pv,
            display::stop_note(&result),
        );
        if result.stop_reason == deft_reversi_engine::StopReason::Requested {
            break;
        }
    }
    let elapsed = start.elapsed();
    if !args.tsv {
        println!("{rule}");
        println!(
            "{:>4} | {:>5} | {:>4} | {:>7} | {:>KIND_WIDTH$} | {:>12} | {:>12.0} | {:>12} | {:>3} |",
            "all",
            "",
            "",
            "",
            "",
            nodes,
            nodes as f64 / elapsed.as_secs_f64(),
            display::duration(elapsed),
            ""
        );
        println!("{rule}");
    }
    eprintln!(
        "Solve: {processed}/{} positions visited; {incomplete} incomplete; {wrong} disagree with answers.",
        problems.len()
    );
    if wrong > 0 {
        return Err(format!(
            "{wrong} of {} positions disagree with the answers",
            problems.len()
        )
        .into());
    }
    Ok(())
}

/// ベンチマーク比較用に 1 問分の探索統計を TSV 形式で出力する。
fn print_tsv(index: usize, position: Position, result: &SearchResult) {
    let (mode, depth) = match result.kind {
        None => ("Depth(0)".into(), 0),
        Some(ResultKind::Depth { depth, .. }) => (format!("Depth({depth})"), depth),
        Some(ResultKind::Endgame { selectivity }) => (
            if selectivity == Selectivity::Full {
                "Exact"
            } else {
                "SelectiveEndgame"
            }
            .into(),
            position.empty_count(),
        ),
    };
    let action = result
        .best_move
        .map_or_else(|| "terminal".into(), |a| a.to_string());
    println!(
        "{index}\t{}\t{action}\t{mode}\t{depth}\t{}\t{}\t{}\t{}\t{}\t{:.9}\t{:.9}\t{:?}",
        result
            .score
            .map_or_else(|| "unknown".into(), |s| s.to_string()),
        result.stats.nodes,
        result.stats.leaves,
        result.stats.splits,
        result.stats.helper_jobs,
        result.stats.ancestor_handoffs,
        result.stats.join_time.as_secs_f64(),
        result.elapsed.as_secs_f64(),
        result.stop_reason,
    );
}

/// Ctrl-C 停止を有効にしたうえで 1 局面を探索する。
fn search(
    engine: &mut Engine,
    position: Position,
    args: &GoalArgs,
    goal: Goal,
    full_pv: bool,
) -> Result<SearchResult> {
    let mut request = args.request(goal);
    if full_pv {
        request = request.recover_endgame_pv();
    }
    let _armed = stop::arm(request.stop_handle());
    Ok(engine.search(position, &request)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{EngineOptions, Model, PatternModel, SearchRequest};

    #[test]
    fn answers_are_checked_against_an_exact_solve() {
        // Two empties; the listed best move is the only winning one.
        let line =
            "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX-- X; G8:+0; H8:+0";
        let problem = parse(line).unwrap();
        assert_eq!(problem.answers.len(), 2);
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default(),
        )
        .unwrap();
        let result = engine
            .search(problem.position, &SearchRequest::new(Goal::Endgame))
            .unwrap();
        let score = result.score.unwrap();
        let fixed = |s: i32| Problem {
            position: problem.position,
            answers: vec![(result.best_move.unwrap(), s)],
        };
        assert_eq!(check(&fixed(score), &result), Some(true));
        assert_eq!(check(&fixed(score + 2), &result), Some(false));
        assert_eq!(check(&parse(&line[..66]).unwrap(), &result), None);
    }

    #[test]
    fn legacy_and_semicolon_answers_and_all_bundled_files_are_readable() {
        let cells = "O".repeat(32) + &"X".repeat(30) + "--";
        let old = parse(&format!("{cells} X G8:+0 H8:-2")).unwrap();
        let new = parse(&format!("{cells} X; G8: +0; H8:-2;")).unwrap();
        assert_eq!(old.position, new.position);
        assert_eq!(old.answers, new.answers);
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/problem")).unwrap() {
            assert!(!read(&entry.unwrap().path()).unwrap().is_empty());
        }
    }

    #[test]
    fn malformed_lines_are_rejected() {
        assert!(parse("X X").is_err());
        let board = "-".repeat(64);
        assert!(parse(&format!("{board} X; F5")).is_err());
        assert!(parse(&format!("{board} X; F5:x")).is_err());
    }
}
