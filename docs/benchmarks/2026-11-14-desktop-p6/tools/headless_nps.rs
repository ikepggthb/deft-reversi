//! Scratch: self-play at one level on EngineWorker, for the desktop's
//! search-impact comparison.
use deft_reversi_engine::{
    Action, Engine, EngineOptions, EngineWorker, Goal, Model, Position, SearchRequest,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let model = args.next().ok_or("usage: MODEL LEVEL GAMES")?;
    let level: u8 = args.next().ok_or("level")?.parse()?;
    let games: u32 = args.next().ok_or("games")?.parse()?;
    let pause: u64 = args.next().map_or(Ok(0), |s| s.parse())?;
    let worker = EngineWorker::new(Engine::new(
        Model::from_file(model)?,
        EngineOptions::default().threads(8).tt_mib(128),
    )?);
    let (mut nodes, mut ms) = (0u64, 0f64);
    for _ in 0..games {
        worker.with(|e| e.clear_tt())?;
        let mut position = Position::initial();
        while !position.is_terminal() {
            if position.must_pass() {
                position = position.play(Action::Pass)?;
                continue;
            }
            let r = worker
                .start(position, SearchRequest::new(Goal::Level(level)))?
                .wait()?;
            nodes += r.stats.nodes;
            ms += r.elapsed.as_secs_f64() * 1000.;
            position = position.play(r.best_move.ok_or("no move")?)?;
            std::thread::sleep(std::time::Duration::from_millis(pause));
        }
    }
    println!("nodes {nodes} ms {ms:.0} nps {:.1} M", nodes as f64 / ms / 1000.);
    Ok(())
}
