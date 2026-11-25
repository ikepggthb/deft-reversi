//! ターミナル上でエンジンと対局・検討を行う対話型 `play` コマンド。
//!
//! 対局モード（先手／後手／AI 同士／二人打ち）の切り替え、待った・やり直し、
//! Book のオン・オフ、全候補手の解析、棋譜検討などのコマンドを受け付ける。

use crate::{EngineArgs, GoalArgs, Result, book_view, display, game::Game, settings, stop};
use clap::Args;
use deft_reversi_book::{BookDecision, BookMiss, BookPlayer, MovePolicy};
use deft_reversi_engine::{Action, Color, Engine, Goal, SearchRequest, Selectivity};
use sha2::{Digest, Sha256};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    time::Duration,
};

/// `play` サブコマンドの引数。
#[derive(Args, Debug)]
pub struct PlayArgs {
    #[command(flatten)]
    engine: EngineArgs,
    /// 探索レベル・深さ・完全読みの指定（省略時はレベル 10）。
    #[command(flatten)]
    pub(super) goal: GoalArgs,
    /// 探索の前に参照する Book ファイル（`.dbook`）。
    #[arg(short, long)]
    book: Option<PathBuf>,
    /// 最善手だけでなく、最善との差がこの石数以内の Book 手からランダムに選ぶ。
    #[arg(long, requires = "book", value_parser = clap::value_parser!(u8).range(0..=128))]
    book_loss: Option<u8>,
    /// 対局モード（0: 黒番人間, 1: 白番人間, 2: AI 対 AI, 3: 人間同士）。
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
    mode: u8,
}

/// 対話セッションの状態。
struct Session {
    game: Game,
    engine: Engine,
    book: Option<BookPlayer>,
    book_on: bool,
    goal: Goal,
    time: Option<Duration>,
    /// エンジンが担当する手番（添字 0: 黒番、添字 1: 白番）。
    computer: [bool; 2],
    details: Option<ModelDetails>,
}

/// `settings` コマンドで表示するために保持しておくモデルと Book の詳細情報。
struct ModelDetails {
    path: PathBuf,
    sha256: String,
    calibrated: bool,
    book: Option<PathBuf>,
    book_loss: Option<u8>,
    book_positions: Option<usize>,
    book_matches: Option<bool>,
}

/// モデルと Book を読み込み、対話型対局ループを開始する。
pub fn run(args: PlayArgs) -> Result<()> {
    let bytes = std::fs::read(&args.engine.model)
        .map_err(|e| format!("{}: {e}", args.engine.model.display()))?;
    let model = deft_reversi_engine::Model::from_bytes(&bytes)?;
    let mut details = ModelDetails {
        path: args.engine.model.clone(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        calibrated: model.has_calibration(),
        book: args.book.clone(),
        book_loss: args.book_loss,
        book_positions: None,
        book_matches: None,
    };
    drop(bytes);
    let engine = args.engine.with_model(model)?;
    let book = match &args.book {
        None => None,
        Some(path) => {
            let moves = match args.book_loss {
                None => MovePolicy::Best,
                Some(max_loss) => MovePolicy::RandomWithinLoss { max_loss },
            };
            let (player, positions, matches) = book_view::load(&engine, path, moves, seed())?;
            details.book_positions = Some(positions);
            details.book_matches = Some(matches);
            Some(player)
        }
    };
    let mut session = Session {
        game: Game::new(),
        engine,
        book_on: book.is_some(),
        book,
        goal: args.goal.goal(Goal::Level(10)),
        time: args.goal.time_limit(),
        computer: [false; 2],
        details: Some(details),
    };
    session.set_mode(args.mode);
    println!("Type `help` for commands.");
    session.run()
}

/// 現在時刻のナノ秒値から Book ランダム選択用の簡易シードを生成する。
fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

impl Session {
    /// 盤面表示、AI 思考、標準入力からのコマンド受付を繰り返すメインループ。
    fn run(&mut self) -> Result<()> {
        let mut input = io::stdin().lock();
        loop {
            self.show();
            let position = self.game.current();
            if position.is_terminal() {
                self.show_result();
            } else if position.must_pass() {
                println!("{} passes.", display::color(position.side_to_move()));
                self.game.play(Action::Pass)?;
                continue;
            } else if self.is_computer(position.side_to_move()) {
                match self.think() {
                    Ok(()) => continue,
                    Err(e) => {
                        println!("Search error: {e}. Switched to mode 3; board preserved.");
                        self.set_mode(3);
                    }
                }
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

    /// 指定された手番をエンジンが担当するかどうかを返す。
    fn is_computer(&self, color: Color) -> bool {
        self.computer[color as usize]
    }

    /// 対局モード（0〜3）に応じて黒番・白番のエンジン担当フラグを設定する。
    fn set_mode(&mut self, mode: u8) {
        self.computer = match mode {
            0 => [false, true],
            1 => [true, false],
            2 => [true, true],
            _ => [false, false],
        };
    }

    /// 1 行分のユーザー入力を解釈して実行し、次の画面遷移先を返す。
    fn command(&mut self, line: &str) -> Result<Next> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let arg = |i: usize| words.get(i).copied().unwrap_or("");
        let expected = match arg(0) {
            "new" | "init" | "undo" | "u" | "redo" | "go" | "hint" | "exact" | "settings"
            | "help" | "h" | "?" | "quit" | "exit" | "q" => Some(1),
            "level" | "l" | "depth" | "time" | "mode" | "play" | "book" => Some(2),
            _ => None,
        };
        if let Some(n) = expected
            && words.len() != n
        {
            return usage("wrong number of arguments; type help");
        }
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
                ("show", Some(book)) => {
                    book_view::show(book, self.game.current());
                    return Ok(Next::Prompt);
                }
                _ => return usage("book on|off|show"),
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
                // 次の人間手番まで戻る。
                while self.is_computer(self.game.current().side_to_move()) && self.game.undo() {}
            }
            "redo" => {
                if !self.game.redo() {
                    println!("Nothing to redo.");
                    return Ok(Next::Prompt);
                }
                while self.is_computer(self.game.current().side_to_move()) && self.game.redo() {}
            }
            "mode" => match arg(1).parse::<u8>() {
                Ok(n @ 0..=3) => self.set_mode(n),
                _ => return usage("mode <0-3>"),
            },
            "search" => {
                if words.len() == 1 {
                    self.show_search();
                } else {
                    match <crate::SearchCommand as clap::Parser>::try_parse_from(
                        words.iter().copied(),
                    ) {
                        Ok(args) => {
                            self.goal = args.goal.goal(self.goal);
                            if args.goal.time_ms.is_some() {
                                self.time = args.goal.time_limit();
                            }
                            self.show_search();
                        }
                        Err(e) => println!("{e}"),
                    }
                }
                return Ok(Next::Prompt);
            }
            "review" => {
                let (start, record) = match words.len() {
                    1 => (self.game.start(), self.game.record()),
                    2 => (deft_reversi_engine::Position::initial(), arg(1).to_string()),
                    _ => return usage("review [RECORD]"),
                };
                let book = self.book.as_ref().filter(|_| self.book_on);
                if let Err(e) = crate::review::review(
                    &mut self.engine,
                    start,
                    &record,
                    self.goal,
                    self.time,
                    book,
                ) {
                    println!("Review error: {e}");
                }
                return Ok(Next::Prompt);
            }
            "settings" => {
                self.show_settings();
                return Ok(Next::Prompt);
            }
            "setboard" => {
                match imported_position(line.strip_prefix("setboard").unwrap().trim()) {
                    Ok(position) => {
                        self.engine.clear_tt()?;
                        self.game = Game::from_position(position);
                        // AI 対 AI モードのまま読み込むと即座に手が進行するため、二人打ちへ切り替えて盤面を保持する。
                        self.set_mode(3);
                        println!("Position loaded; mode 3 (two players). History starts here.");
                    }
                    Err(e) => {
                        println!("{e}");
                        return Ok(Next::Prompt);
                    }
                }
            }
            "hint" | "analyze" => {
                let all = arg(0) == "analyze";
                let full_pvs = all && words.len() == 2 && arg(1) == "--pv";
                if words.len() != 1 && !full_pvs {
                    return usage("analyze [--pv]");
                }
                if let Err(e) = self.analyze(all, full_pvs) {
                    println!("Analysis error: {e}");
                }
                return Ok(Next::Prompt);
            }
            "eval" => {
                let compact = settings::eval_spec(arg(1));
                let parsed = compact.as_ref().ok().map(|e| e.depth);
                let plan = settings::plan(self.goal);
                let stage = if arg(2).is_empty() {
                    Ok(compact
                        .as_ref()
                        .ok()
                        .and_then(|e| e.mpc)
                        .unwrap_or(plan.midgame_selectivity()))
                } else {
                    settings::selectivity(arg(2))
                };
                match (parsed, stage) {
                    (Some(depth), Ok(stage))
                        if words.len() <= 3 && !(arg(1).contains('@') && words.len() == 3) =>
                    {
                        self.goal = Goal::Plan(plan.with_midgame(depth, stage))
                    }
                    _ => return usage("eval <0-60> [83|85|95|98|99|100]"),
                }
                print!("{}", settings::describe(self.goal));
            }
            "endgame" => {
                let mut plan = settings::plan(self.goal);
                if arg(1) == "clear" && words.len() == 2 {
                    for stage in Selectivity::ALL {
                        plan = plan.endgame(stage, 0);
                    }
                } else {
                    let stage = settings::endgame_stage(&format!("{}:{}", arg(1), arg(2)));
                    match stage {
                        Ok(stage) if words.len() == 3 => {
                            plan = plan.endgame(stage.selectivity, stage.empties)
                        }
                        _ => {
                            return usage(
                                "endgame <83|85|95|98|99|100> <empties 0-60> | endgame clear",
                            );
                        }
                    }
                }
                self.goal = Goal::Plan(plan);
                print!("{}", settings::describe(self.goal));
            }
            "go" => {
                if self.game.current().is_terminal() {
                    println!("The game is over.");
                    return Ok(Next::Prompt);
                }
                if let Err(e) = self.think() {
                    println!("Search error: {e}");
                    return Ok(Next::Prompt);
                }
            }
            "help" | "h" | "?" => {
                help();
                return Ok(Next::Prompt);
            }
            "quit" | "exit" | "q" => return Ok(Next::Quit),
            word if words.len() > 1 => return usage(&format!("unexpected arguments after {word}")),
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

    /// 現在の手番でエンジンの手（Book または探索）を 1 手進める。
    fn think(&mut self) -> Result<()> {
        let position = self.game.current();
        let side = display::color(position.side_to_move());
        if self.book_on
            && let Some(book) = self.book.as_mut()
        {
            match book.decide(position) {
                BookDecision::Chosen(choice) => {
                    println!(
                        "{side} plays {} from the book ({})",
                        choice.action,
                        book_value(&choice)
                    );
                    self.game.play(choice.action)?;
                    return Ok(());
                }
                BookDecision::Fallback(reason) => {
                    let message = match reason {
                        BookMiss::ModelMismatch => "model mismatch",
                        BookMiss::MissingPosition => "position not in book",
                        BookMiss::NoRegisteredMove => "no eligible registered move",
                        BookMiss::InvalidPosition => "position cannot be played from book",
                        BookMiss::NoBook => "no book loaded",
                        _ => "no eligible registered move",
                    };
                    println!("Book fallback: {message}");
                }
            }
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
        if result.stop_reason == deft_reversi_engine::StopReason::WorkerFailed {
            return Err("engine search worker failed".into());
        }
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

    /// `hint`（最善手のみ）または `analyze`（全候補手）を実行し、盤面を変えずに結果を表示する。
    fn analyze(&mut self, all: bool, full_pvs: bool) -> Result<()> {
        let position = self.game.current();
        if position.is_terminal() {
            println!("The game is over.");
            return Ok(());
        }
        let book = self.book.as_ref().filter(|_| self.book_on);
        if !all
            && !matches!(self.goal, Goal::Endgame)
            && let Some(book) = book
            && let BookDecision::Chosen(choice) = book.recommend(position)
        {
            let v = choice.value;
            println!(
                "Hint: {} from the book  {}",
                choice.action,
                book_view::value(v.result(), v.conditions(), v.source())
            );
            let mut line = vec![choice.action];
            line.extend(book.continuation(position.play(choice.action)?));
            println!("Book line: {}", display::pv(&line));
            println!("Board unchanged. Scores are from the side to move's perspective.");
            return Ok(());
        }
        let entry = book.and_then(|b| b.probe(position));
        let mut request = SearchRequest::new(self.goal);
        if full_pvs {
            request = request.all_moves_with_pv();
        } else if all {
            request = request.all_moves();
        }
        if let Some(limit) = self.time {
            request = request.time_limit(limit);
        }
        let _armed = stop::arm(request.stop_handle());
        let result = self.engine.search_with(position, &request, |p| {
            if !all {
                println!("{}", display::progress(p));
            }
        })?;
        if result.stop_reason == deft_reversi_engine::StopReason::WorkerFailed {
            return Err("engine search worker failed".into());
        }
        if all {
            println!(
                "Analysis: {}  {} nodes  {:.2}s{}",
                display::kind(result.kind),
                display::nodes(result.stats.nodes),
                result.elapsed.as_secs_f64(),
                display::stop_note(&result)
            );
            if result.moves.is_empty() {
                println!("No complete candidate scores.");
                if book.is_some() {
                    println!("Use 'book show' to inspect the Book.");
                }
            } else {
                let book_entry = book.zip(entry.as_ref());
                if book_entry.is_some() {
                    println!("Move  Search  Book link  Book evaluation");
                } else {
                    if book.is_some() {
                        println!("Book: position absent");
                    }
                    println!("Move  Search");
                }
                for m in &result.moves {
                    let score = format!("{:+}", m.score);
                    if let Some((book, entry)) = book_entry {
                        let candidate =
                            entry.moves().find(|candidate| candidate.action == m.action);
                        let (link, evaluation) = match candidate {
                            Some(c) => (
                                if c.registered { "linked" } else { "unlinked" },
                                book_view::analysis_value(book, c),
                            ),
                            None => ("-", "-".into()),
                        };
                        println!(
                            "{:<4}  {:>6}  {:<9}  {evaluation}",
                            m.action.to_string(),
                            score,
                            link
                        );
                    } else {
                        println!("{:<4}  {:>6}", m.action.to_string(), score);
                    }
                }
            }
        } else {
            println!(
                "Hint: {}  score {}  {}{}",
                display::action(result.best_move),
                display::score(result.score),
                display::kind(result.kind),
                display::stop_note(&result)
            );
        }
        if full_pvs && !result.moves.is_empty() {
            println!("Candidate PVs");
            for (m, pv) in result.moves.iter().zip(&result.move_pvs) {
                println!("  {:<4} {}", m.action.to_string(), display::pv(pv));
            }
        } else if !result.pv.is_empty() {
            let pv = if all {
                display::pv_prefix(&result.pv, 8)
            } else {
                display::pv(&result.pv)
            };
            println!("PV (best): {pv}");
        }
        println!("Board unchanged. Scores are from the side to move's perspective.");
        Ok(())
    }

    /// 現在の探索設定と、現局面の空きマス数で有効になる探索段階を表示する。
    fn show_search(&self) {
        println!("Search: {}", settings::summary(self.goal, self.time));
        println!(
            "Now ({} empties): {}",
            self.game.current().empty_count(),
            settings::active(self.goal, self.game.current().empty_count())
        );
    }

    /// エンジン構成、モデル、Book、探索目標、ビルド情報をまとめて表示する。
    fn show_settings(&self) {
        let options = self.engine.options();
        let actual = 1usize << options.tt_mib.ilog2();
        println!("Threads: {}", options.threads);
        println!(
            "TT: requested {} MiB; allocated main table {} MiB (auxiliary tables excluded)",
            options.tt_mib, actual
        );
        if let Some(d) = &self.details {
            println!(
                "Model: {}\nModel SHA-256: {}\nMPC calibration: {}",
                d.path.display(),
                d.sha256,
                d.calibrated
            );
            println!(
                "Book: {} ({}, policy {})",
                d.book
                    .as_ref()
                    .map_or_else(|| "none".into(), |p| p.display().to_string()),
                if self.book_on { "on" } else { "off" },
                d.book_loss
                    .map_or_else(|| "best move".into(), |n| format!("within {n} discs"))
            );
            if let (Some(positions), Some(matches)) = (d.book_positions, d.book_matches) {
                println!("Book positions: {positions}\nBook model match: {matches}");
            }
        }
        println!("Goal: {}", goal(self.goal, self.time));
        print!("{}", settings::describe(self.goal));
        println!(
            "Time limit: {}",
            self.time
                .map_or_else(|| "none".into(), |t| format!("{} ms", t.as_millis()))
        );
        println!(
            "Players: black {}, white {}",
            if self.computer[0] { "AI" } else { "human" },
            if self.computer[1] { "AI" } else { "human" }
        );
        println!("{}", settings::versions());
        println!(
            "Build target: {}\nProfile: {}, opt-level {}\nRust flags: {}",
            env!("DEFT_BUILD_TARGET"),
            env!("DEFT_BUILD_PROFILE"),
            env!("DEFT_BUILD_OPT_LEVEL"),
            env!("DEFT_BUILD_CARGO_ENCODED_RUSTFLAGS")
        );
        if let Ok(path) = std::env::current_exe()
            && let Ok(bytes) = std::fs::read(path)
        {
            println!("Binary SHA-256: {:x}", Sha256::digest(bytes));
        }
    }

    /// 現在の探索設定、盤面、石数、手数、棋譜を表示する。
    fn show(&self) {
        let position = self.game.current();
        println!();
        self.show_search();
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

    /// 終局時の石差と勝敗メッセージを表示する。
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

/// `setboard` コマンドの引数（OBF 文字列または `--file PATH [--line N]`）を局面へ変換する。
fn imported_position(text: &str) -> Result<deft_reversi_engine::Position> {
    if let Some(file) = text.strip_prefix("--file ") {
        let (file, index) = match file.rsplit_once(" --line ") {
            Some((file, index)) => (file, index.parse::<usize>()?),
            None => (file, 1),
        };
        let positions = crate::solve::read(&PathBuf::from(file.trim().trim_matches('"')))?;
        let p = index
            .checked_sub(1)
            .and_then(|i| positions.get(i))
            .ok_or("position index out of range (1-based)")?;
        Ok(p.position)
    } else {
        Ok(crate::solve::parse(text)?.position)
    }
}

/// 対話コマンド処理後のループ遷移先。
enum Next {
    /// 盤面を再描画して対局を続行する。
    Show,
    /// 盤面を再描画せず次のプロンプトを出す。
    Prompt,
    /// 対話セッションを終了する。
    Quit,
}

/// コマンドの使い方を表示し、盤面を再描画せず次の入力を待つ。
fn usage(text: &str) -> Result<Next> {
    println!("usage: {text}");
    Ok(Next::Prompt)
}

/// 現在の探索目標と制限時間を短い表示文字列に整形する。
fn goal(goal: Goal, time: Option<Duration>) -> String {
    let goal = match goal {
        Goal::Level(n) => format!("level {n}"),
        Goal::Depth(n) => format!("depth {n}"),
        Goal::Endgame => "exact".into(),
        Goal::Plan(_) => "custom plan".into(),
    };
    match time {
        Some(t) => format!("{goal}, {:.1}s", t.as_secs_f64()),
        None => goal,
    }
}

/// Book が選んだ手の評価値と探索品質を 1 行の補足文字列にする。
fn book_value(choice: &deft_reversi_book::BookChoice) -> String {
    let v = choice.value;
    let quality = if v.is_solved() {
        "solved".into()
    } else if v.selectivity() == Selectivity::Full {
        format!("d{}", v.depth())
    } else {
        format!("d{}@{}%", v.depth(), v.selectivity().percent())
    };
    format!("{:+}, {quality}", v.score())
}

/// 対話コマンド一覧を標準出力へ表示する。
fn help() {
    println!(
        "\
  f5                  play a move (forced passes are played for you)
  play f5d6c3         play a record
  undo | redo         take back or replay a move
  search              show current search settings and active stage
  search --eval 12@83 --endgame 95:24,99:22,100:18  replace search settings
  settings            show actual engine/model/search/build settings
  review [RECORD]     review current history or a record without playing
  setboard <OBF>      load a board and side to move; resets history, switches to mode 3
  setboard --file PATH [--line N]  load the Nth position (default 1)
  hint                suggest a Book move or search for a move without playing
  analyze [--pv]      score all candidates with Book values; --pv shows candidate lines
  eval DEPTH [PERCENT]  configure eval search, keeping endgame stages
  endgame PERCENT EMPTIES  switch at or below EMPTIES; 0 disables the stage
  endgame clear       disable all endgame stages
  go                  let the engine move now
  new                 start a new game
  mode <0-3>          0: you black, 1: you white, 2: engine vs engine, 3: two players
  level <1-60>        search by level
  depth <0-60>        search to a fixed depth
  exact               read to the end of the game
  time <ms>           time limit per move, 0 for none
  book on|off|show    toggle Book use or show current Book moves and lines
  help | quit"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{EngineOptions, Model, PatternModel};

    fn session() -> Session {
        Session {
            game: Game::new(),
            engine: Engine::new(
                Model::from_pattern(PatternModel::zero()),
                EngineOptions::default().tt_mib(1),
            )
            .unwrap(),
            book: None,
            book_on: false,
            goal: Goal::Depth(1),
            time: None,
            computer: [false, true],
            details: None,
        }
    }

    #[test]
    fn undo_redo_preserve_the_computer_reply() {
        let mut session = session();
        session.command("play f5d6").unwrap();
        let after = session.game.current();
        session.command("undo").unwrap();
        assert_eq!(session.game.placements(), 0);
        session.command("redo").unwrap();
        assert_eq!(session.game.current(), after);
        assert_eq!(session.game.record(), "f5d6");
    }
    #[test]
    fn inspection_does_not_play_and_loading_is_atomic() {
        let mut s = session();
        s.command("play f5d6").unwrap();
        s.command("depth 2").unwrap();
        let before = s.game.current();
        let record = s.game.record();
        for cmd in [
            "settings",
            "hint",
            "analyze",
            "setboard bad",
            "endgame 94 12",
            "eval 61",
        ] {
            s.command(cmd).unwrap();
            assert_eq!(s.game.current(), before);
            assert_eq!(s.game.record(), record);
        }
        s.command("review").unwrap();
        assert_eq!(s.game.current(), before);
        assert_eq!(s.game.record(), record);
        for cmd in ["f5 junk", "new junk", "go junk", "search --eval 61@83"] {
            s.command(cmd).unwrap();
            assert_eq!(s.game.current(), before);
        }
        s.command("search --eval 3@100 --endgame 95:8,100:4 --time-ms 0")
            .unwrap();
        assert_eq!(settings::plan(s.goal).midgame_depth(), 3);
        assert_eq!(s.time, None);
        s.command("endgame clear").unwrap();
        s.command("eval 3 100").unwrap();
        s.command("endgame 95 8").unwrap();
        let p = settings::plan(s.goal);
        assert_eq!(p.midgame_depth(), 3);
        assert_eq!(p.endgame_limit(Selectivity::Percent95), 8);
        s.command(&format!(
            "setboard {}",
            deft_reversi_engine::Position::initial()
        ))
        .unwrap();
        assert_eq!(s.game.current(), deft_reversi_engine::Position::initial());
        assert!(!s.game.undo());
        assert_eq!(s.computer, [false; 2]);
    }
}
