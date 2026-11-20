use crate::{
    Engine, EngineArgs, Result, book_view,
    game_clock::{self, Budget},
};
use clap::Args;
use deft_reversi_book::{BookPlayer, MovePolicy};
use deft_reversi_engine::{Action, Color, Moves, Position, Square, StopReason};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

const REPLY_MARGIN: Duration = Duration::from_millis(50);

#[derive(Args, Clone, Debug)]
pub struct FlipsightArgs {
    #[command(flatten)]
    engine: EngineArgs,
    /// Opening book to use before searching.
    #[arg(long)]
    book: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Turn {
    board: Vec<u8>,
    color: u8,
    legal_moves: Option<Vec<u8>>,
    game_mode: Option<String>,
    board_size: Option<u8>,
    #[serde(default)]
    blocked_squares: Vec<u8>,
    remaining_ms: u64,
    budget_ms: u64,
}

impl Turn {
    fn position(&self) -> Result<(Position, Moves)> {
        if self.game_mode.as_deref().unwrap_or("std") != "std"
            || self.board_size.unwrap_or(8) != 8
            || !self.blocked_squares.is_empty()
        {
            return Err("FlipSight engine supports standard 8x8 Othello only".into());
        }
        if self.board.len() != 64 {
            return Err("FlipSight board must have 64 cells".into());
        }
        let side = match self.color {
            1 => Color::Black,
            2 => Color::White,
            _ => return Err("invalid FlipSight color".into()),
        };
        let mut black = 0;
        let mut white = 0;
        // Both protocols number squares A1..H8 in row-major order, starting at zero.
        for (index, cell) in self.board.iter().enumerate() {
            match cell {
                0 => {}
                1 => black |= 1 << index,
                2 => white |= 1 << index,
                _ => return Err("invalid FlipSight board cell".into()),
            }
        }
        let position = Position::from_colors(black, white, side).ok_or("invalid board")?;
        let moves = position.legal_moves();
        if let Some(indices) = &self.legal_moves {
            let mut mask = 0;
            for &index in indices {
                mask |= Square::new(index).ok_or("invalid legal move index")?.bit();
            }
            if mask != moves.bits() {
                return Err("FlipSight legal moves disagree with the engine".into());
            }
        }
        Ok((position, moves))
    }
}

#[derive(Serialize)]
struct Reply {
    #[serde(rename = "move")]
    square: i16,
}

enum SearchEngine {
    Loading(Receiver<std::result::Result<ReadyEngine, String>>),
    Ready(ReadyEngine),
}

struct ReadyEngine {
    engine: Engine,
    book: Option<BookPlayer>,
}

impl ReadyEngine {
    fn load(args: FlipsightArgs) -> Result<Self> {
        let engine = args.engine.engine()?;
        let book = args
            .book
            .map(|path| book_view::load(&engine, &path, MovePolicy::Best, 0))
            .transpose()?
            .map(|(player, _, _)| player);
        Ok(Self { engine, book })
    }
}

impl SearchEngine {
    fn start(args: FlipsightArgs) -> Result<Self> {
        let (send, receive) = mpsc::channel();
        thread::Builder::new()
            .name("flipsight-model".into())
            .spawn(move || {
                let _ = send.send(ReadyEngine::load(args).map_err(|error| error.to_string()));
            })?;
        Ok(Self::Loading(receive))
    }

    fn wait(&mut self, timeout: Duration) -> Result<Option<&mut ReadyEngine>> {
        if let Self::Loading(receive) = self {
            match receive.recv_timeout(timeout) {
                Ok(engine) => *self = Self::Ready(engine?),
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("engine initialization failed".into());
                }
            }
        }
        match self {
            Self::Ready(engine) => Ok(Some(engine)),
            Self::Loading(_) => Ok(None),
        }
    }
}

pub fn run(args: FlipsightArgs) -> Result<()> {
    let mut engine = None;
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let received = Instant::now();
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let turn: Turn = serde_json::from_str(&line)?;
        let (position, mut moves) = turn.position()?;
        let square = match moves.next() {
            None => -1,
            Some(only) if moves.is_empty() => i16::from(only.index()),
            Some(first) => {
                let engine = match &mut engine {
                    Some(engine) => engine,
                    None => engine.insert(SearchEngine::start(args.clone())?),
                };
                i16::from(choose(engine, &turn, position, first, received)?.index())
            }
        };
        serde_json::to_writer(&mut out, &Reply { square })?;
        writeln!(out)?;
        out.flush()?;
    }
    Ok(())
}

fn choose(
    engine: &mut SearchEngine,
    turn: &Turn,
    position: Position,
    first: Square,
    received: Instant,
) -> Result<Square> {
    let hard_limit = Duration::from_millis(turn.budget_ms.min(turn.remaining_ms));
    let time_left = || hard_limit.saturating_sub(received.elapsed() + REPLY_MARGIN);
    // The connector spawns us on the first move, so model loading must also respect its deadline.
    let Some(engine) = engine.wait(time_left())? else {
        return Ok(first);
    };
    if let Some(choice) = engine.book.as_mut().and_then(|book| book.choose(position))
        && let Action::Place(square) = choice.action
    {
        return Ok(square);
    }
    let remaining =
        Duration::from_millis(turn.remaining_ms).saturating_sub(received.elapsed() + REPLY_MARGIN);
    let budget = Budget::new(position.empty_count(), remaining, time_left());
    let result = if budget.hard.is_zero() {
        game_clock::reflex(&mut engine.engine, position)?
    } else {
        budget.search(&mut engine.engine, position, |_| {})?
    };
    match (result.stop_reason, result.best_move) {
        (StopReason::WorkerFailed, _) => Err("search worker failed".into()),
        (_, Some(Action::Place(square))) => Ok(square),
        _ => Err("search returned no placement for a playable position".into()),
    }
}
