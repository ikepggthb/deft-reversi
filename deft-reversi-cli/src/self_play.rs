use crate::{EngineArgs, GoalArgs, Result, game::Game, stop};
use clap::Args;
use deft_reversi_engine::{Action, Engine, Goal, SearchRequest};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::{fs::File, io::Write, path::PathBuf};

#[derive(Args, Debug)]
pub struct SelfPlayArgs {
    /// Number of games.
    games: usize,
    #[command(flatten)]
    engine: EngineArgs,
    /// Level 16 unless a level, depth or exact solve is given.
    #[command(flatten)]
    goal: GoalArgs,
    /// Random placements at the start of each game.
    #[arg(long, default_value_t = 20)]
    random: usize,
    /// Seed for the random openings; a fresh one when omitted.
    #[arg(long)]
    seed: Option<u64>,
    /// One game record per line.
    #[arg(short, long, default_value = "self-play.txt")]
    out: PathBuf,
}

pub fn run(args: SelfPlayArgs) -> Result<()> {
    let mut engine = args.engine.engine()?;
    let mut rng = args
        .seed
        .map_or_else(StdRng::from_entropy, StdRng::seed_from_u64);
    let request = args.goal.request(args.goal.goal(Goal::Level(16)));
    let mut out = File::create(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    for n in 1..=args.games {
        let _armed = stop::arm(request.stop_handle());
        let game = play_game(&mut engine, &request, args.random, &mut rng)?;
        if request.stop_handle().is_stopped() {
            println!("stopped; {} games written to {}", n - 1, args.out.display());
            return Ok(());
        }
        writeln!(out, "{}", game.record())?;
        engine.clear_tt()?;
        println!("{n} / {} games", args.games);
    }
    println!("records written to {}", args.out.display());
    Ok(())
}

pub fn play_game(
    engine: &mut Engine,
    request: &SearchRequest,
    random: usize,
    rng: &mut impl Rng,
) -> Result<Game> {
    let mut game = Game::new();
    while game.placements() < random && !game.current().is_terminal() {
        let moves: Vec<_> = game.current().legal_moves().into_iter().collect();
        game.play(Action::Place(moves[rng.gen_range(0..moves.len())]))?;
    }
    while !game.current().is_terminal() {
        let result = engine.search(game.current(), request)?;
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
}
