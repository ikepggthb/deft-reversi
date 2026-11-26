//! エンジン同士の自己対戦を行い、棋譜をファイルへ書き出す `self-play` コマンド。
//!
//! 序盤の指定手数をランダムに打って変化を付け、残りをエンジン探索で終局まで指し切る。

use crate::{EngineArgs, GoalArgs, Result, game::Game, stop};
use clap::Args;
use deft_reversi_engine::{Action, Engine, Goal, SearchRequest};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::{fs::File, io::Write, path::PathBuf};

/// `self-play` サブコマンドの引数。
#[derive(Args, Debug)]
pub struct SelfPlayArgs {
    /// 対局数。
    games: usize,
    #[command(flatten)]
    engine: EngineArgs,
    /// 探索レベル・深さ・完全読みの指定（省略時はレベル 16）。
    #[command(flatten)]
    goal: GoalArgs,
    /// 各局の開始時にランダムで打つ手数。
    #[arg(long, default_value_t = 20)]
    random: usize,
    /// ランダム序盤用の乱数シード（省略時は環境エントロピーから生成）。
    #[arg(long)]
    seed: Option<u64>,
    /// 1 行 1 局で棋譜を書き出す出力先ファイル。
    #[arg(short, long, default_value = "self-play.txt")]
    out: PathBuf,
}

/// 指定された局数の自己対戦を実行し、完了した対局の棋譜をファイルへ書き出す。
pub fn run(args: SelfPlayArgs) -> Result<()> {
    let mut engine = args.engine.engine()?;
    let mut rng = args
        .seed
        .map_or_else(StdRng::from_entropy, StdRng::seed_from_u64);
    let goal = args.goal.goal(Goal::Level(16));
    crate::settings::report(&engine, &args.engine, goal, args.goal.time_limit());
    let request = args.goal.request(goal);
    let mut out = File::create(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    for n in 1..=args.games {
        let _armed = stop::arm(request.stop_handle());
        let game = play_game(&mut engine, &request, args.random, &mut rng)?;
        if request.stop_handle().is_stopped() {
            println!("stopped; {} games written to {}", n - 1, args.out.display());
            return Ok(());
        }
        writeln!(out, "{}", game.record())?;
        // 前局の探索結果が次局に影響しないよう、1 局ごとに置換表を消去する。
        engine.clear_tt()?;
        println!("{n} / {} games", args.games);
    }
    println!("records written to {}", args.out.display());
    Ok(())
}

/// 1 局分の自己対戦を実行する。
///
/// 序盤 `random` 手までは合法手から一様ランダムに選び、その後は `request` に従う
/// エンジン探索で終局まで進める。中断要求を受けた場合は途中のまま返す。
pub fn play_game(
    engine: &mut Engine,
    request: &SearchRequest,
    random: usize,
    rng: &mut impl Rng,
) -> Result<Game> {
    let mut game = Game::new();
    while game.placements() < random && !game.current().is_terminal() {
        if request.stop_handle().is_stopped() {
            return Ok(game);
        }
        let moves: Vec<_> = game.current().legal_moves().into_iter().collect();
        game.play(Action::Place(moves[rng.gen_range(0..moves.len())]))?;
    }
    while !game.current().is_terminal() {
        if request.stop_handle().is_stopped() {
            return Ok(game);
        }
        let result = engine.search(game.current(), request)?;
        if result.stop_reason == deft_reversi_engine::StopReason::WorkerFailed {
            return Err("engine search worker failed; unfinished record not written".into());
        }
        if request.stop_handle().is_stopped() {
            return Ok(game);
        }
        game.play(
            result
                .best_move
                .ok_or("no move before the end of the game")?,
        )?;
    }
    Ok(game)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{EngineOptions, Model, PatternModel};

    #[test]
    fn games_reach_the_end() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(16),
        )
        .unwrap();
        let mut rng = StdRng::seed_from_u64(1);
        let request = SearchRequest::new(Goal::Depth(1));
        let game = play_game(&mut engine, &request, 10, &mut rng).unwrap();
        assert!(game.current().is_terminal());
        let mut replay = Game::new();
        replay.play_record(&game.record()).unwrap();
        assert_eq!(replay.current(), game.current());
    }
    #[test]
    fn stopped_games_do_not_play_or_search() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let request = SearchRequest::new(Goal::Depth(1));
        request.stop_handle().stop();
        let mut rng = StdRng::seed_from_u64(1);
        for random in [0, 20] {
            let game = play_game(&mut engine, &request, random, &mut rng).unwrap();
            assert_eq!(game.current(), deft_reversi_engine::Position::initial());
        }
    }
}
