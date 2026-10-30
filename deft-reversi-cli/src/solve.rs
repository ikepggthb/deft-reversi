use crate::{EngineArgs, GoalArgs, Result, display, stop};
use clap::Args;
use deft_reversi_engine::{Action, Engine, Goal, Position, SearchResult};
use std::{path::PathBuf, time::Instant};

#[derive(Args, Debug)]
pub struct SolveArgs {
    /// One position per line: 64 squares and the side to move, optionally
    /// followed by `; A2:+38; C7:+36; ...` answers, best first.
    file: PathBuf,
    #[command(flatten)]
    engine: EngineArgs,
    /// Searches read to the end of the game without pruning unless a level
    /// or depth is given.
    #[command(flatten)]
    goal: GoalArgs,
    /// Clear the transposition table before each position.
    #[arg(long)]
    clear_tt: bool,
}

pub struct Problem {
    pub position: Position,
    /// Scores of the moves the file lists, best first.
    pub answers: Vec<(Action, i32)>,
}

pub fn parse(line: &str) -> std::result::Result<Problem, String> {
    let mut fields = line.split(';').map(str::trim).filter(|f| !f.is_empty());
    let position = fields
        .next()
        .ok_or("empty line")?
        .parse::<Position>()
        .map_err(|e| e.to_string())?;
    let answers = fields
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

pub fn read(path: &PathBuf) -> Result<Vec<Problem>> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|(i, line)| parse(line).map_err(|e| format!("line {}: {e}", i + 1).into()))
        .collect()
}

/// Whether `result` agrees with the answers: the best score, and a move that
/// reaches it. None when the file has no answers.
pub fn check(problem: &Problem, result: &SearchResult) -> Option<bool> {
    let best = problem.answers.first()?.1;
    let move_ok = problem
        .answers
        .iter()
        .any(|&(a, s)| Some(a) == result.best_move && s == best);
    Some(result.score == Some(best) && move_ok)
}

pub fn run(args: SolveArgs) -> Result<()> {
    let problems = read(&args.file)?;
    let mut engine = args.engine.engine()?;
    let goal = args.goal.goal(Goal::Endgame);
    let header = format!(
        "{:>4} | {:>5} | {:>4} | {:>7} | {:>6} | {:>12} | {:>12} | {:>12} | {:>3} | pv",
        "#", "score", "move", "empties", "kind", "nodes", "nps", "time", "ok"
    );
    let rule = "-".repeat(header.len() + 30);
    println!("{rule}\n{header}\n{rule}");
    let (mut nodes, mut wrong, start) = (0, 0, Instant::now());
    for (i, problem) in problems.iter().enumerate() {
        if args.clear_tt {
            engine.clear_tt()?;
        }
        let result = search(&mut engine, problem.position, &args.goal, goal)?;
        let verdict = match check(problem, &result) {
            None => "",
            Some(true) => "ok",
            Some(false) => {
                wrong += 1;
                "NG"
            }
        };
        nodes += result.stats.nodes;
        println!(
            "{:>4} | {:>5} | {:>4} | {:>7} | {:>6} | {:>12} | {:>12.0} | {:>12} | {:>3} | {}{}",
            i + 1,
            display::score(result.score),
            display::action(result.best_move),
            problem.position.empty_count(),
            display::kind(result.kind),
            result.stats.nodes,
            result.stats.nodes as f64 / result.elapsed.as_secs_f64(),
            display::duration(result.elapsed),
            verdict,
            display::pv(&result.pv),
            display::stop_note(&result),
        );
    }
    let elapsed = start.elapsed();
    println!("{rule}");
    println!(
        "{:>4} | {:>5} | {:>4} | {:>7} | {:>6} | {:>12} | {:>12.0} | {:>12} |",
        "all",
        "",
        "",
        "",
        "",
        nodes,
        nodes as f64 / elapsed.as_secs_f64(),
        display::duration(elapsed)
    );
    println!("{rule}");
    if wrong > 0 {
        return Err(format!(
            "{wrong} of {} positions disagree with the answers",
            problems.len()
        )
        .into());
    }
    Ok(())
}

fn search(
    engine: &mut Engine,
    position: Position,
    args: &GoalArgs,
    goal: Goal,
) -> Result<SearchResult> {
    let request = args.request(goal);
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
    fn malformed_lines_are_rejected() {
        assert!(parse("X X").is_err());
        let board = "-".repeat(64);
        assert!(parse(&format!("{board} X; F5")).is_err());
        assert!(parse(&format!("{board} X; F5:x")).is_err());
    }
}
