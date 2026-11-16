use crate::{EngineArgs, GoalArgs, Result, display, game::Game, settings, stop};
use clap::Args;
use deft_reversi_engine::{Action, Engine, Goal, Position, SearchRequest, StopReason};
use std::{path::PathBuf, time::Duration};

#[derive(Args, Debug)]
pub struct ReviewArgs {
    /// Record such as f5d6c3 (passes are inferred).
    #[arg(required_unless_present = "file", conflicts_with = "file")]
    record: Option<String>,
    /// One record per line; blanks and # comments are ignored.
    #[arg(long)]
    file: Option<PathBuf>,
    #[command(flatten)]
    engine: EngineArgs,
    #[command(flatten)]
    goal: GoalArgs,
}

pub fn positions(start: Position, record: &str) -> Result<Vec<(Position, Action)>> {
    if record.is_empty() {
        return Err("no moves to review".into());
    }
    // Validate the entire record before performing or printing any searches.
    let mut game = Game::from_position(start);
    if game.current().must_pass() {
        game.play(Action::Pass)?;
    }
    game.play_record(record)?;
    let mut game = Game::from_position(start);
    if game.current().must_pass() {
        game.play(Action::Pass)?;
    }
    let mut positions = Vec::new();
    for chunk in record.as_bytes().as_chunks::<2>().0 {
        let action = std::str::from_utf8(chunk)?.parse::<Action>()?;
        positions.push((game.current(), action));
        game.play(action)?;
    }
    Ok(positions)
}

#[derive(Default, Debug)]
pub struct ReviewSummary {
    pub scored: usize,
    pub unscored: usize,
    pub worst_loss: i32,
    pub stopped: bool,
}

pub fn review(
    engine: &mut Engine,
    start: Position,
    record: &str,
    goal: Goal,
    time: Option<Duration>,
) -> Result<ReviewSummary> {
    let positions = positions(start, record)?;
    let mut summary = ReviewSummary::default();
    println!("Search: {}", settings::summary(goal, time));
    println!("Move Side  Played Best  PlayedScore BestScore Loss Search");
    for (i, &(pos, played)) in positions.iter().enumerate() {
        let mut request = SearchRequest::new(goal).all_moves();
        if let Some(time) = time {
            request = request.time_limit(time);
        }
        let _armed = stop::arm(request.stop_handle());
        let result = engine.search(pos, &request)?;
        let best = result.moves.iter().max_by_key(|m| m.score);
        let actual = result.moves.iter().find(|m| m.action == played);
        let complete = result.stop_reason == StopReason::Completed;
        if let (true, Some(best), Some(actual)) = (complete, best, actual) {
            let loss = best.score - actual.score;
            let best_action = if actual.score == best.score {
                played
            } else {
                best.action
            };
            summary.scored += 1;
            summary.worst_loss = summary.worst_loss.max(loss);
            println!(
                "{:>4} {:<5} {:<6} {:<5} {:+11} {:+9} {:>4} {}",
                i + 1,
                if pos.side_to_move() == deft_reversi_engine::Color::Black {
                    "Black"
                } else {
                    "White"
                },
                played,
                best_action,
                actual.score,
                best.score,
                loss,
                display::kind(result.kind)
            );
        } else {
            summary.unscored += 1;
            println!(
                "{:>4} {:?} {played} - ? ? ? {} (unscored: {:?}; complete candidates required)",
                i + 1,
                pos.side_to_move(),
                display::kind(result.kind),
                result.stop_reason
            );
        }
        if result.stop_reason == StopReason::Requested {
            summary.stopped = true;
            break;
        }
        if result.stop_reason == StopReason::WorkerFailed {
            return Err("review search worker failed".into());
        }
    }
    println!(
        "Review: scored {}/{}; unscored {}; not visited {}; largest loss {} discs{}.",
        summary.scored,
        positions.len(),
        summary.unscored,
        positions.len() - summary.scored - summary.unscored,
        if summary.scored == 0 {
            "unknown".into()
        } else {
            summary.worst_loss.to_string()
        },
        if summary.stopped { " (stopped)" } else { "" }
    );
    println!(
        "Scores/losses are for the player to move. Only 'exact' rows are proven; other rows are estimates at the displayed search quality."
    );
    Ok(summary)
}

pub fn run(args: ReviewArgs) -> Result<()> {
    let records = if let Some(file) = &args.file {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let records: Vec<_> = text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .map(|(i, l)| (i + 1, l.trim().to_string()))
            .collect();
        if records.is_empty() {
            return Err("no records in file".into());
        }
        records
    } else {
        vec![(1, args.record.unwrap())]
    };
    for (line, record) in &records {
        positions(Position::initial(), record).map_err(|e| format!("record line {line}: {e}"))?;
    }
    let mut engine = args.engine.engine()?;
    let goal = args.goal.goal(Goal::Level(10));
    settings::report(&engine, &args.engine, goal, args.goal.time_limit());
    for (line, record) in records {
        engine.clear_tt()?;
        println!("Record line {line}: {record}");
        if review(
            &mut engine,
            Position::initial(),
            &record,
            goal,
            args.goal.time_limit(),
        )?
        .stopped
        {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{EngineOptions, Model, PatternModel};
    fn engine() -> Engine {
        Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap()
    }
    fn brute(p: Position) -> i32 {
        if p.is_terminal() {
            return p.final_score(p.side_to_move());
        }
        if p.must_pass() {
            return -brute(p.play(Action::Pass).unwrap());
        }
        p.legal_moves()
            .map(|s| -brute(p.play(s).unwrap()))
            .max()
            .unwrap()
    }
    #[test]
    fn exact_losses_agree_with_exhaustive_and_pass_is_inferred() {
        let start: Position = format!("{}{}-- X", "O".repeat(32), "X".repeat(30))
            .parse()
            .unwrap();
        assert!(start.must_pass());
        let next = start.play(Action::Pass).unwrap();
        for square in next.legal_moves() {
            let expected = brute(next) + brute(next.play(square).unwrap());
            let summary = review(
                &mut engine(),
                start,
                &square.to_string(),
                Goal::Endgame,
                None,
            )
            .unwrap();
            assert_eq!(summary.scored, 1);
            assert_eq!(summary.worst_loss, expected);
            assert_eq!(summary.unscored, 0);
        }
    }
    #[test]
    fn invalid_records_and_incomplete_analysis_do_not_become_zero_loss() {
        assert!(positions(Position::initial(), "f5f5").is_err());
        assert!(positions(Position::initial(), "f5x").is_err());
        assert!(positions(Position::initial(), "").is_err());
        let summary = review(
            &mut engine(),
            Position::initial(),
            "f5",
            Goal::Depth(0),
            None,
        )
        .unwrap();
        assert_eq!(summary.scored, 0);
        assert_eq!(summary.unscored, 1);
    }
}
