//! 棋譜の各手をエンジン探索と Book で検証し、最善手との評価値差（悪手度）を報告する `review` コマンド。
//!
//! 強制パスは棋譜文字列から自動補完し、実戦手と最善手の両方の評価値が揃った手だけを集計対象とする。

use crate::{EngineArgs, GoalArgs, Result, book_view, display, game::Game, settings, stop};
use clap::Args;
use deft_reversi_book::{BookPlayer, MovePolicy};
use deft_reversi_engine::{Action, Engine, Goal, Position, SearchRequest, StopReason};
use std::{path::PathBuf, time::Duration};

/// `review` サブコマンドの引数。
#[derive(Args, Debug)]
pub struct ReviewArgs {
    /// 棋譜文字列（例: `f5d6c3`。パスは自動補完される）。
    #[arg(required_unless_present = "file", conflicts_with = "file")]
    record: Option<String>,
    /// 1 行 1 棋譜のテキストファイル（空行と `#` コメント行は無視する）。
    #[arg(long)]
    file: Option<PathBuf>,
    /// 探索と併せて Book との比較も行う `.dbook` ファイル。
    #[arg(long)]
    book: Option<PathBuf>,
    #[command(flatten)]
    engine: EngineArgs,
    #[command(flatten)]
    goal: GoalArgs,
}

/// 開始局面 `start` から棋譜 `record` を再生し、パスを除いた `(着手前局面, 着手)` の一覧を返す。
pub fn positions(start: Position, record: &str) -> Result<Vec<(Position, Action)>> {
    if record.is_empty() {
        return Err("no moves to review".into());
    }
    // 探索や出力を始める前に、棋譜全体の合法性を先に検証しておく。
    let mut game = Game::from_position(start);
    if game.current().must_pass() {
        game.play(Action::Pass)?;
    }
    game.play_record(record)?;
    Ok(game.steps().filter(|&(_, a)| a != Action::Pass).collect())
}

/// 1 局分の棋譜検討の集計結果。
#[derive(Default, Debug)]
pub struct ReviewSummary {
    /// 実戦手と最善手の両方の評価値が得られた手数。
    pub scored: usize,
    /// 探索が中断されたなどの理由で評価値が揃わなかった手数。
    pub unscored: usize,
    /// 評価済み手の中での最大損失石数。
    pub worst_loss: i32,
    /// Ctrl-C などにより途中で検討を打ち切ったかどうか。
    pub stopped: bool,
}

/// 1 局分の棋譜 `record` を `start` から順に検討し、各手の評価表と要約を標準出力へ出す。
pub fn review(
    engine: &mut Engine,
    start: Position,
    record: &str,
    goal: Goal,
    time: Option<Duration>,
    book: Option<&BookPlayer>,
) -> Result<ReviewSummary> {
    let positions = positions(start, record)?;
    let mut summary = ReviewSummary::default();
    let mut first_departure = None;
    println!("Search: {}", settings::summary(goal, time));
    println!("Move Side  Played Best  PlayedScore BestScore Loss Search");
    for (i, &(pos, played)) in positions.iter().enumerate() {
        let Action::Place(square) = played else {
            unreachable!("review positions include only placements");
        };
        let mut request = SearchRequest::new(goal).review_move(square);
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
                "{:>4} {:?} {played} - ? ? ? {} (unscored: {:?}; both scores required)",
                i + 1,
                pos.side_to_move(),
                display::kind(result.kind),
                result.stop_reason
            );
        }
        if let Some(book) = book
            && review_book_move(book, pos, played)
            && first_departure.is_none()
        {
            first_departure = Some(i);
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
    if let Some(index) = first_departure {
        let prefix: String = positions[..=index]
            .iter()
            .filter(|(_, action)| *action != Action::Pass)
            .map(|(_, action)| action.to_string().to_ascii_lowercase())
            .collect();
        if start == Position::initial() {
            println!(
                "First Book departure: move {} (record prefix {prefix})",
                index + 1
            );
        } else {
            println!(
                "First Book departure: move {} (from the loaded start)",
                index + 1
            );
        }
    } else if book.is_some() {
        println!("No Book departure in the reviewed moves.");
    }
    Ok(summary)
}

/// 実戦手 `played` が Book に登録されているかを調べ、比較結果を 1 行出力する。
///
/// 登録済みの Book 手から外れていた場合（局面自体が未登録のときも含む）に `true` を返す。
fn review_book_move(book: &BookPlayer, pos: Position, played: Action) -> bool {
    let Some(entry) = book.probe(pos) else {
        println!("     Book: position absent");
        return true;
    };
    let actual = entry.moves().find(|m| m.action == played);
    let best = entry
        .moves()
        .filter(|m| m.registered)
        .filter_map(|m| book.point_value(m).ok().map(|score| (m, score)))
        .min_by_key(|(m, score)| (std::cmp::Reverse(*score), m.action != played));
    let played_detail = actual.map_or_else(|| "absent".into(), |m| book_view::candidate(book, m));
    print!("     Book: played {played} {played_detail}");
    if let Some((best, score)) = best {
        print!(
            " | best linked {} {}",
            best.action,
            book_view::candidate(book, best)
        );
        if let Some(actual) = actual
            && let Ok(played_score) = book.point_value(actual)
        {
            if score >= played_score {
                print!(" | Book loss {}", score - played_score);
            } else {
                print!(" | played exceeds best linked by {}", played_score - score);
            }
        }
    } else {
        print!(" | no usable linked point value");
    }
    println!();
    !actual.is_some_and(|m| m.registered)
}

/// `review` サブコマンドのエントリーポイント。全棋譜の事前検証後に 1 局ずつ検討する。
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
    let book = args
        .book
        .as_ref()
        .map(|path| book_view::load(&engine, path, MovePolicy::Best, 0))
        .transpose()?
        .map(|(book, _, _)| book);
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
            book.as_ref(),
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
            None,
        )
        .unwrap();
        assert_eq!(summary.scored, 0);
        assert_eq!(summary.unscored, 1);
    }
}
