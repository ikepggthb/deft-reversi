use crate::{EngineArgs, GoalArgs, Result, display, game::Game, stop};
use clap::Args;
use deft_reversi_book::{Book, BookChoice, BookPlayer, MovePolicy, ReadLimits, UsePolicy};
use deft_reversi_engine::{Action, Color, Engine, Goal, SearchRequest, Selectivity};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

#[derive(Args, Debug)]
pub struct PlayArgs {
    #[command(flatten)]
    engine: EngineArgs,
    /// Level 10 unless a level, depth or exact solve is given.
    #[command(flatten)]
    goal: GoalArgs,
    /// Opening book to play from before searching.
    #[arg(short, long)]
    book: Option<PathBuf>,
    /// Pick at random among book moves at most this many discs worse than
    /// the best one, instead of always the best.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=128))]
    book_loss: Option<u8>,
    /// 0: you play black, 1: you play white, 2: engine against itself,
    /// 3: two players.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
    mode: u8,
}

struct Session {
    game: Game,
    engine: Engine,
    book: Option<BookPlayer>,
    book_on: bool,
    goal: Goal,
    time: Option<Duration>,
    /// Which colors the engine plays: black, white.
    computer: [bool; 2],
}

pub fn run(args: PlayArgs) -> Result<()> {
    let engine = args.engine.engine()?;
    let book = match &args.book {
        None => None,
        Some(path) => {
            let book = Book::load(path, ReadLimits::default())
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let moves = match args.book_loss {
                None => MovePolicy::Best,
                Some(max_loss) => MovePolicy::RandomWithinLoss { max_loss },
            };
            let policy = UsePolicy {
                moves,
                ..UsePolicy::default()
            };
            Some(BookPlayer::new(
                &engine,
                Some(Arc::new(book)),
                policy,
                seed(),
            )?)
        }
    };
    let mut session = Session {
        game: Game::new(),
        engine,
        book_on: book.is_some(),
        book,
        goal: args.goal.goal(Goal::Level(10)),
        time: args.goal.time_ms.map(Duration::from_millis),
        computer: [false; 2],
    };
    session.set_mode(args.mode);
    println!("Type `help` for commands.");
    session.run()
}

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

impl Session {
    fn run(&mut self) -> Result<()> {
        let mut input = io::stdin().lock();
        loop {
            self.show();
            let position = self.game.current();
            if position.is_terminal() {
                self.show_result();
            } else if self.is_computer(position.side_to_move()) {
                self.think()?;
                continue;
            }
            print!("> ");
            io::stdout().flush()?;
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                return Ok(());
            }
            loop {
                match self.command(line.trim())? {
                    Next::Show => break,
                    Next::Quit => return Ok(()),
                    Next::Prompt => {}
                }
                print!("> ");
                io::stdout().flush()?;
                line.clear();
                if input.read_line(&mut line)? == 0 {
                    return Ok(());
                }
            }
        }
    }

    fn is_computer(&self, color: Color) -> bool {
        self.computer[color as usize]
    }

    fn set_mode(&mut self, mode: u8) {
        self.computer = match mode {
            0 => [false, true],
            1 => [true, false],
            2 => [true, true],
            _ => [false, false],
        };
    }

    fn command(&mut self, line: &str) -> Result<Next> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let arg = |i: usize| words.get(i).copied().unwrap_or("");
        match arg(0) {
            "" => return Ok(Next::Prompt),
            "new" | "init" => {
                self.game = Game::new();
                self.engine.clear_tt()?;
            }
            "level" | "l" => match arg(1).parse::<u8>() {
                Ok(n @ 1..=60) => self.goal = Goal::Level(n),
                _ => return usage("level <1-60>"),
            },
            "depth" => match arg(1).parse::<u8>() {
                Ok(n @ 0..=60) => self.goal = Goal::Depth(n),
                _ => return usage("depth <0-60>"),
            },
            "exact" => self.goal = Goal::Endgame,
            "time" => match arg(1).parse::<u64>() {
                Ok(0) => self.time = None,
                Ok(ms) => self.time = Some(Duration::from_millis(ms)),
                Err(_) => return usage("time <milliseconds, 0 for none>"),
            },
            "book" => match (arg(1), &self.book) {
                (_, None) => {
                    println!("No book loaded; start with --book FILE.");
                    return Ok(Next::Prompt);
                }
                ("on", _) => self.book_on = true,
                ("off", _) => self.book_on = false,
                _ => return usage("book on|off"),
            },
            "play" => {
                if let Err(e) = self.game.play_record(arg(1)) {
                    println!("{e}");
                    return Ok(Next::Prompt);
                }
            }
            "undo" | "u" => {
                if !self.game.undo() {
                    println!("Nothing to undo.");
                    return Ok(Next::Prompt);
                }
                // Back to a position where a player is to move.
                while self.is_computer(self.game.current().side_to_move()) && self.game.undo() {}
            }
            "redo" => {
                if !self.game.redo() {
                    println!("Nothing to redo.");
                    return Ok(Next::Prompt);
                }
            }
            "mode" => match arg(1).parse::<u8>() {
                Ok(n @ 0..=3) => self.set_mode(n),
                _ => return usage("mode <0-3>"),
            },
            "go" => {
                if self.game.current().is_terminal() {
                    println!("The game is over.");
                    return Ok(Next::Prompt);
                }
                self.think()?;
            }
            "help" | "h" | "?" => {
                help();
                return Ok(Next::Prompt);
            }
            "quit" | "exit" | "q" => return Ok(Next::Quit),
            word => match word.parse::<Action>() {
                Ok(action) => {
                    if let Err(e) = self.game.play(action) {
                        println!("{word}: {e}");
                        return Ok(Next::Prompt);
                    }
                }
                Err(_) => {
                    println!("Unknown command `{word}`; type `help`.");
                    return Ok(Next::Prompt);
                }
            },
        }
        Ok(Next::Show)
    }

    /// Plays the engine's move for the side to move.
    fn think(&mut self) -> Result<()> {
        let position = self.game.current();
        let side = display::color(position.side_to_move());
        if self.book_on
            && let Some(choice) = self.book.as_mut().and_then(|b| b.choose(position))
        {
            println!(
                "{side} plays {} from the book ({})",
                choice.action,
                book_value(&choice)
            );
            self.game.play(choice.action)?;
            return Ok(());
        }
        println!(
            "{side} is thinking ({}); Ctrl-C plays the best move so far.",
            goal(self.goal, self.time)
        );
        let mut request = SearchRequest::new(self.goal);
        if let Some(limit) = self.time {
            request = request.time_limit(limit);
        }
        let _armed = stop::arm(request.stop_handle());
        let result = self
            .engine
            .search_with(position, &request, |p| println!("{}", display::progress(p)))?;
        let action = result
            .best_move
            .ok_or("no move before the end of the game")?;
        println!(
            "{side} plays {action}  score {}  {}  {} nodes  {:.2}s{}",
            display::score(result.score),
            display::kind(result.kind),
            display::nodes(result.stats.nodes),
            result.elapsed.as_secs_f64(),
            display::stop_note(&result),
        );
        println!("pv {}", display::pv(&result.pv));
        self.game.play(action)?;
        Ok(())
    }

    fn show(&self) {
        let position = self.game.current();
        println!();
        print!("{}", display::board(position));
        println!(
            "X {:>2}  O {:>2}   move {}   empties {}   {} to move{}",
            position.disc_count(Color::Black),
            position.disc_count(Color::White),
            self.game.placements() + 1,
            position.empty_count(),
            display::color(position.side_to_move()),
            if self.book_on { "   book on" } else { "" },
        );
        let record = self.game.record();
        if !record.is_empty() {
            for line in record.as_bytes().chunks(40) {
                println!("{}", std::str::from_utf8(line).unwrap());
            }
        }
    }

    fn show_result(&self) {
        let score = self.game.current().final_score(Color::Black);
        let winner = match score {
            0 => "Draw".into(),
            s if s > 0 => format!("Black wins by {s}"),
            s => format!("White wins by {}", -s),
        };
        println!("Game over: {winner}. `new` starts another game, `undo` takes back.");
    }
}

enum Next {
    /// Show the board and continue the game.
    Show,
    /// Ask for another command.
    Prompt,
    Quit,
}

fn usage(text: &str) -> Result<Next> {
    println!("usage: {text}");
    Ok(Next::Prompt)
}

fn goal(goal: Goal, time: Option<Duration>) -> String {
    let goal = match goal {
        Goal::Level(n) => format!("level {n}"),
        Goal::Depth(n) => format!("depth {n}"),
        Goal::Endgame => "exact".into(),
    };
    match time {
        Some(t) => format!("{goal}, {:.1}s", t.as_secs_f64()),
        None => goal,
    }
}

fn book_value(choice: &BookChoice) -> String {
    let v = choice.value;
    let quality = if v.quality.solved {
        "solved".into()
    } else if v.quality.selectivity == Selectivity::Full {
        format!("d{}", v.quality.depth)
    } else {
        format!("d{}@{}%", v.quality.depth, v.quality.selectivity.percent())
    };
    format!("{:+}, {quality}", v.score)
}

fn help() {
    println!(
        "\
  f5                  play a move (forced passes are played for you)
  play f5d6c3         play a record
  undo | redo         take back or replay a move
  go                  let the engine move now
  new                 start a new game
  mode <0-3>          0: you black, 1: you white, 2: engine vs engine, 3: two players
  level <1-60>        search by level
  depth <0-60>        search to a fixed depth
  exact               read to the end of the game
  time <ms>           time limit per move, 0 for none
  book on|off         use the opening book
  help | quit"
    );
}
