//! The controller: commands in, state changes and effects out. It does not
//! touch GPUI or threads, so it is tested without a window or an engine.
use crate::{
    domain::{GameInfo, Grouping, History, Player, Strength, editor},
    persistence::record::LoadedGame,
    services::engine::{EngineEvent, Report, Resources, SearchId, SearchJob, SearchSpec},
};
use deft_reversi_engine::{Action, Color, Position, Square};
mod analysis;
pub use analysis::{AnalysisSettings, Analyzer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewGame {
    /// Black, then White.
    pub players: [Player; 2],
    pub start: Position,
}

impl Default for NewGame {
    fn default() -> Self {
        Self {
            players: [Player::Human, Player::Ai(Strength::default())],
            start: Position::initial(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    NewGame(NewGame),
    Play(Square),
    Undo,
    Redo,
    /// Moves back (negative) or forward through the record.
    Step(isize),
    First,
    Last,
    /// Shows the position after that many actions.
    GoTo(usize),
    Resume,
    Pause,
    /// One AI move while paused.
    StepAi,
    /// Plays the AI's best move so far.
    MoveNow,
    SetResources(Resources),
    /// A checked record replaces the game; it opens paused.
    Load(Box<LoadedGame>),
    /// Analysis mode pauses the game and searches the shown position.
    SetAnalysis(bool),
    AnalyzeExact,
    /// Searches the positions of the record not yet read at the level,
    /// from the end; with `redo`, every one that is not solved exactly.
    AnalyzeRecord {
        redo: bool,
    },
    /// Stops the analysis search, keeping what it found, and the batch.
    StopAnalysis,
    SetAnalysisSettings(AnalysisSettings),
    /// Whether AI moves may come from the opening book.
    SetBook(bool),
    /// The engine grows a book; it is free again on `EngineEvent::BookDone`.
    GenerateBook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Place,
    Pass,
    GameOver,
}

/// Work for the services; sounds are played from P6.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    StartSearch { job: SearchJob },
    CancelSearch,
    MoveNow,
    ClearTt,
    Reconfigure(Resources),
    Sound(Sound),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// AI players move by themselves.
    Playing,
    /// Nothing moves until the user resumes or plays.
    Paused,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineStatus {
    Loading,
    Ready,
    /// Growing a book; nothing else searches.
    Generating,
    Unavailable(String),
}

#[derive(Clone, Debug)]
pub struct Thinking {
    id: SearchId,
    revision: u64,
    strength: Strength,
    /// Stays paused after the move.
    single_step: bool,
    pub color: Color,
    pub report: Option<Report>,
}

/// One move the AI played, with the search behind it.
#[derive(Clone, Debug)]
pub struct AiMove {
    /// The move number, as in the record.
    pub number: usize,
    pub color: Color,
    pub strength: Strength,
    pub action: Action,
    pub report: Report,
}

pub struct Controller {
    game: GameInfo,
    history: History,
    players: [Player; 2],
    status: Status,
    engine: EngineStatus,
    /// Grows with every change of the record or the players, so a search
    /// result can be checked against the state it was asked for.
    revision: u64,
    next_search: u64,
    thinking: Option<Thinking>,
    analyzing: bool,
    analyzer: Analyzer,
    notice: Option<String>,
    /// Every AI move of this game, oldest first; Undo keeps them.
    ai_log: Vec<AiMove>,
    /// See `Command::SetBook`.
    book: bool,
}

impl Default for Controller {
    fn default() -> Self {
        let new_game = NewGame::default();
        Self {
            game: GameInfo::new(),
            history: History::new(new_game.start),
            players: new_game.players,
            status: Status::Playing,
            engine: EngineStatus::Loading,
            revision: 0,
            next_search: 1,
            thinking: None,
            analyzing: false,
            analyzer: Analyzer::default(),
            notice: None,
            ai_log: Vec::new(),
            book: false,
        }
    }
}

fn index(color: Color) -> usize {
    match color {
        Color::Black => 0,
        Color::White => 1,
    }
}

impl Controller {
    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn game(&self) -> &GameInfo {
        &self.game
    }

    /// Changes with every change worth saving.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn players(&self) -> [Player; 2] {
        self.players
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn engine(&self) -> &EngineStatus {
        &self.engine
    }

    pub fn thinking(&self) -> Option<&Thinking> {
        self.thinking.as_ref()
    }

    /// Whether the event brings the AI's move, which plays it.
    pub fn is_ai_move(&self, event: &EngineEvent) -> bool {
        matches!(event, EngineEvent::Finished { id, .. }
            if self.thinking.as_ref().is_some_and(|t| t.id == *id))
    }

    pub fn analyzing(&self) -> bool {
        self.analyzing
    }

    pub fn analyzer(&self) -> &Analyzer {
        &self.analyzer
    }

    pub fn ai_log(&self) -> &[AiMove] {
        &self.ai_log
    }

    /// The last search error, until the next command.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn has_ai(&self) -> bool {
        self.players.iter().any(|p| p.is_ai())
    }

    /// The AI's strength when an AI is to move in the shown position. An
    /// edited position with too few discs has no AI move (§9.1).
    pub fn ai_to_move(&self) -> Option<Strength> {
        let position = self.history.current();
        if position.is_terminal() || !editor::searchable(position) {
            return None;
        }
        match self.players[index(position.side_to_move())] {
            Player::Ai(strength) => Some(strength),
            Player::Human => None,
        }
    }

    pub fn apply(&mut self, command: Command) -> Vec<Effect> {
        self.notice = None;
        let mut effects = Vec::new();
        match command {
            Command::NewGame(new_game) => {
                self.cancel(&mut effects);
                // Analysis mode stays on for the new game.
                self.analyzer.cancel(&mut effects);
                self.ai_log.clear();
                self.game = GameInfo::new();
                self.history = History::new(new_game.start);
                self.players = new_game.players;
                self.status = Status::Playing;
                self.revision += 1;
                effects.push(Effect::ClearTt);
            }
            Command::Play(square) => self.play_human(square, &mut effects),
            Command::Undo => self.navigate(History::undo, &mut effects),
            Command::Redo => self.navigate(History::redo, &mut effects),
            Command::Step(steps) => self.navigate(|h| h.step(steps), &mut effects),
            Command::First => self.navigate(|h| h.go_to(0), &mut effects),
            Command::Last => self.navigate(|h| h.go_to(usize::MAX), &mut effects),
            Command::GoTo(index) => self.navigate(|h| h.go_to(index), &mut effects),
            Command::Resume => self.status = Status::Playing,
            Command::Pause => {
                self.cancel(&mut effects);
                self.status = Status::Paused;
            }
            Command::StepAi => {
                if self.status == Status::Paused
                    && self.thinking.is_none()
                    && self.engine == EngineStatus::Ready
                    && let Some(strength) = self.ai_to_move()
                {
                    self.start_search(strength, true, &mut effects);
                }
            }
            Command::MoveNow => {
                if self.thinking.is_some() {
                    effects.push(Effect::MoveNow);
                }
            }
            Command::SetResources(resources) => {
                self.cancel(&mut effects);
                self.analyzer.cancel(&mut effects);
                self.engine = EngineStatus::Loading;
                effects.push(Effect::Reconfigure(resources));
            }
            Command::Load(loaded) => {
                self.cancel(&mut effects);
                let LoadedGame {
                    game,
                    players,
                    history,
                } = *loaded;
                self.game = game;
                self.players = players;
                self.history = history;
                self.ai_log.clear();
                // A loaded game never plays on by itself.
                self.status = Status::Paused;
                self.revision += 1;
                effects.push(Effect::ClearTt);
            }
            // The game goes on in analysis mode: an AI move takes the engine,
            // and the analysis searches while a person is to move.
            Command::SetAnalysis(on) => {
                if !on {
                    self.analyzer.cancel(&mut effects);
                }
                self.analyzing = on;
            }
            Command::AnalyzeExact => {
                if self.analyzing && self.engine == EngineStatus::Ready {
                    let position = self.history.current();
                    self.analyzer
                        .exact(position, &mut self.next_search, &mut effects);
                }
            }
            Command::AnalyzeRecord { redo } => {
                if self.analyzing {
                    self.analyzer.start_batch(&self.history, redo);
                }
            }
            Command::StopAnalysis => self.analyzer.stop(&mut effects),
            Command::SetAnalysisSettings(settings) => self.analyzer.settings_changed(settings),
            Command::SetBook(book) => self.book = book,
            // A book being grown takes the engine: the AI and the analysis wait.
            Command::GenerateBook => {
                if self.engine == EngineStatus::Ready {
                    self.cancel(&mut effects);
                    self.analyzer.cancel(&mut effects);
                    self.status = Status::Paused;
                    self.engine = EngineStatus::Generating;
                }
            }
        }
        self.autoplay(&mut effects);
        self.pump_analysis(&mut effects);
        effects
    }

    pub fn on_engine(&mut self, event: EngineEvent) -> Vec<Effect> {
        let mut effects = Vec::new();
        match event {
            EngineEvent::Ready { .. } => self.engine = EngineStatus::Ready,
            // The settings screen shows it.
            EngineEvent::Book(_) | EngineEvent::BookProgress(_) => {}
            EngineEvent::BookDone(_) => {
                if self.engine == EngineStatus::Generating {
                    self.engine = EngineStatus::Ready;
                }
            }
            EngineEvent::LoadFailed(error) => {
                self.thinking = None;
                self.analyzer.cancel(&mut Vec::new());
                self.engine = EngineStatus::Unavailable(error);
            }
            EngineEvent::ReconfigureFailed(error) => {
                self.engine = EngineStatus::Ready;
                self.notice = Some(error);
            }
            EngineEvent::Progress { id, report } => {
                if let Some(thinking) = self.thinking.as_mut().filter(|t| t.id == id) {
                    thinking.report = Some(report);
                } else {
                    self.analyzer.on_progress(id, &report);
                }
            }
            EngineEvent::Finished { id, report } => {
                if self.thinking.as_ref().is_some_and(|t| t.id == id) {
                    if let Some(thinking) = self.take_thinking(id) {
                        self.play_ai(thinking, report, &mut effects);
                    }
                } else {
                    self.analyzer.on_finished(id, report);
                }
            }
            EngineEvent::Failed { id, error } => {
                if self.take_thinking(id).is_some() {
                    self.status = Status::Paused;
                    self.notice = Some(error);
                } else if self.analyzer.on_failed(id) {
                    self.notice = Some(error);
                }
            }
        }
        self.autoplay(&mut effects);
        self.pump_analysis(&mut effects);
        effects
    }

    /// The running search if `id` is it and nothing changed since it began.
    fn take_thinking(&mut self, id: SearchId) -> Option<Thinking> {
        let current = self.thinking.as_ref()?;
        if current.id != id {
            return None;
        }
        let thinking = self.thinking.take()?;
        (thinking.revision == self.revision).then_some(thinking)
    }

    fn play_human(&mut self, square: Square, effects: &mut Vec<Effect>) {
        let human_turn = !self.history.current().is_terminal() && self.ai_to_move().is_none();
        // While paused, the user may also play the AI's side to explore.
        if !human_turn && self.status != Status::Paused {
            return;
        }
        if !self.history.current().legal_moves().contains(square) {
            return;
        }
        self.cancel(effects);
        self.record(Action::Place(square), Grouping::NewGroup, effects);
        if human_turn {
            self.status = Status::Playing;
        }
    }

    fn play_ai(&mut self, thinking: Thinking, report: Report, effects: &mut Vec<Effect>) {
        let best_move = report.best_move;
        let position = self.history.current();
        let legal = match best_move {
            Some(Action::Place(square)) => position.legal_moves().contains(square),
            _ => false,
        };
        if !legal || position.side_to_move() != thinking.color {
            self.status = Status::Paused;
            return;
        }
        // Discs on the board count the moves, also from an edited start.
        let discs = position.disc_count(Color::Black) + position.disc_count(Color::White);
        self.ai_log.push(AiMove {
            number: discs as usize - 3,
            color: thinking.color,
            strength: thinking.strength,
            action: best_move.expect("checked legal"),
            report,
        });
        // An AI reply undoes together with the human move before it.
        let grouping = if self.players.iter().all(|p| p.is_ai()) {
            Grouping::NewGroup
        } else {
            Grouping::Join
        };
        self.record(best_move.expect("checked legal"), grouping, effects);
        if thinking.single_step {
            self.status = Status::Paused;
        }
    }

    fn record(&mut self, action: Action, grouping: Grouping, effects: &mut Vec<Effect>) {
        self.history
            .play(action, grouping)
            .expect("the caller checked the move");
        self.revision += 1;
        effects.push(Effect::Sound(if self.history.current().is_terminal() {
            Sound::GameOver
        } else if self.history.actions().last() == Some(&Action::Pass) {
            Sound::Pass
        } else {
            Sound::Place
        }));
    }

    /// Moving through the record stops the AI; play resumes on request.
    fn navigate(&mut self, step: impl FnOnce(&mut History) -> bool, effects: &mut Vec<Effect>) {
        if !step(&mut self.history) {
            return;
        }
        self.cancel(effects);
        self.revision += 1;
        if self.has_ai() {
            self.status = Status::Paused;
        }
    }

    fn cancel(&mut self, effects: &mut Vec<Effect>) {
        if self.thinking.take().is_some() {
            effects.push(Effect::CancelSearch);
        }
    }

    /// The only place an analysis search starts.
    fn pump_analysis(&mut self, effects: &mut Vec<Effect>) {
        if self.analyzing && self.thinking.is_none() && self.engine == EngineStatus::Ready {
            let shown = self.history.current();
            self.analyzer.pump(shown, &mut self.next_search, effects);
        }
    }

    /// The only place a move search starts by itself.
    fn autoplay(&mut self, effects: &mut Vec<Effect>) {
        if self.status != Status::Playing
            || self.thinking.is_some()
            || self.engine != EngineStatus::Ready
        {
            return;
        }
        if let Some(strength) = self.ai_to_move() {
            self.start_search(strength, false, effects);
        }
    }

    fn start_search(&mut self, strength: Strength, single_step: bool, effects: &mut Vec<Effect>) {
        // The move comes first; the analysis picks up after it.
        self.analyzer.interrupt(effects);
        let id = SearchId(self.next_search);
        self.next_search += 1;
        let position = self.history.current();
        self.thinking = Some(Thinking {
            id,
            revision: self.revision,
            strength,
            single_step,
            color: position.side_to_move(),
            report: None,
        });
        effects.push(Effect::StartSearch {
            job: SearchJob {
                id,
                position,
                spec: SearchSpec {
                    book: self.book,
                    ..strength.spec()
                },
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Preset;
    use std::time::Duration;

    const AI: Player = Player::Ai(Strength::Preset(Preset::Beginner));

    fn ready(players: [Player; 2]) -> Controller {
        let mut c = Controller::default();
        c.on_engine(EngineEvent::Ready {
            resources: Resources {
                threads: 1,
                tt_mib: 1,
            },
            load_time: Duration::ZERO,
        });
        c.apply(Command::NewGame(NewGame {
            players,
            start: Position::initial(),
        }));
        c
    }

    fn play(c: &mut Controller, name: &str) -> Vec<Effect> {
        c.apply(Command::Play(name.parse().unwrap()))
    }

    fn started(effects: &[Effect]) -> Option<SearchId> {
        effects.iter().find_map(|e| match e {
            Effect::StartSearch { job, .. } => Some(job.id),
            _ => None,
        })
    }

    /// The engine answers `id` with the first legal move.
    fn finish(c: &mut Controller, id: SearchId) -> Vec<Effect> {
        let best = c
            .history()
            .current()
            .legal_moves()
            .next()
            .map(Action::Place);
        c.on_engine(EngineEvent::Finished {
            id,
            report: Report {
                best_move: best,
                score: Some(0),
                reading: None,
                pv: Vec::new(),
                moves: Vec::new(),
                nodes: 0,
                elapsed: Duration::ZERO,
            },
        })
    }

    #[test]
    fn only_the_answer_to_the_ai_search_is_its_move() {
        let mut c = ready([Player::Human, Player::Ai(Strength::Level(8))]);
        let id = started(&play(&mut c, "f5")).expect("the AI thinks");
        let answer = |id| EngineEvent::Finished {
            id,
            report: Report {
                best_move: None,
                score: None,
                reading: None,
                pv: Vec::new(),
                moves: Vec::new(),
                nodes: 0,
                elapsed: Duration::ZERO,
            },
        };
        assert!(c.is_ai_move(&answer(id)));
        // Once taken back, a late answer plays nothing.
        c.apply(Command::Undo);
        assert!(!c.is_ai_move(&answer(id)));
    }

    #[test]
    fn growing_a_book_holds_the_engine_until_done() {
        let mut c = ready([Player::Human, Player::Ai(Strength::Level(8))]);
        c.apply(Command::SetAnalysis(true));
        c.apply(Command::GenerateBook);
        assert_eq!(*c.engine(), EngineStatus::Generating);
        assert_eq!(c.status(), Status::Paused);
        // Neither a move nor the analysis may search meanwhile.
        let effects = play(&mut c, "f5");
        assert!(started(&effects).is_none());
        assert!(started(&c.apply(Command::StepAi)).is_none());
        // The person played, so the AI moves as soon as the engine is free.
        let effects = c.on_engine(EngineEvent::BookDone(Err("stopped".into())));
        assert_eq!(*c.engine(), EngineStatus::Ready);
        assert!(started(&effects).is_some());
    }

    #[test]
    fn every_ai_asks_the_book_when_there_is_one() {
        let strengths = [
            Strength::Preset(Preset::Beginner),
            Strength::Preset(Preset::Strongest),
            Strength::Level(8),
        ];
        for strength in strengths {
            let mut c = Controller::default();
            c.apply(Command::SetBook(true));
            c.on_engine(EngineEvent::Ready {
                resources: Resources {
                    threads: 1,
                    tt_mib: 1,
                },
                load_time: Duration::ZERO,
            });
            let effects = c.apply(Command::NewGame(NewGame {
                players: [Player::Ai(strength), Player::Human],
                start: Position::initial(),
            }));
            let spec = effects.iter().find_map(|e| match e {
                Effect::StartSearch { job } => Some(job.spec),
                _ => None,
            });
            assert_eq!(spec.map(|s| s.book), Some(true), "{strength:?}");
        }
        // Without a book, no AI asks it.
        let mut c = ready([Player::Human, Player::Ai(Strength::Level(8))]);
        let effects = play(&mut c, "f5");
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::StartSearch { job } if !job.spec.book
        )));
    }

    #[test]
    fn local_game_with_undo_and_navigation() {
        let mut c = ready([Player::Human, Player::Human]);
        assert_eq!(play(&mut c, "f5"), vec![Effect::Sound(Sound::Place)]);
        assert!(play(&mut c, "a1").is_empty());
        play(&mut c, "d6");
        play(&mut c, "c3");
        c.apply(Command::Undo);
        assert_eq!(c.history().cursor(), 2);
        c.apply(Command::First);
        c.apply(Command::Step(2));
        assert_eq!(c.history().cursor(), 2);
        c.apply(Command::Last);
        assert_eq!(c.history().cursor(), 3);
        c.apply(Command::GoTo(1));
        play(&mut c, "f4");
        assert_eq!(c.history().actions().len(), 2);
        assert_eq!(c.status(), Status::Playing);
    }

    #[test]
    fn the_ai_answers_and_undo_takes_back_both_moves() {
        let mut c = ready([Player::Human, AI]);
        let id = started(&play(&mut c, "f5")).expect("the AI starts");
        // The human cannot move for the AI.
        assert!(play(&mut c, "f6").is_empty());
        finish(&mut c, id);
        assert_eq!(c.history().actions().len(), 2);
        assert!(c.thinking().is_none());
        // The AI move is logged with its number and colour.
        assert_eq!(c.ai_log().len(), 1);
        assert_eq!(
            (c.ai_log()[0].number, c.ai_log()[0].color),
            (2, Color::White)
        );
        c.apply(Command::Undo);
        assert_eq!(c.history().cursor(), 0);
        assert_eq!(c.status(), Status::Paused);
        // A new human move resumes the game.
        assert!(started(&play(&mut c, "d3")).is_some());
        assert_eq!(c.status(), Status::Playing);
    }

    #[test]
    fn stale_results_are_dropped() {
        let mut c = ready([Player::Human, AI]);
        let id = started(&play(&mut c, "f5")).unwrap();
        assert!(c.apply(Command::Undo).contains(&Effect::CancelSearch));
        finish(&mut c, id);
        assert_eq!(c.history().actions().len(), 1);
        assert_eq!(c.history().cursor(), 0);

        let mut c = ready([Player::Human, AI]);
        let id = started(&play(&mut c, "f5")).unwrap();
        let effects = c.apply(Command::NewGame(NewGame::default()));
        assert_eq!(effects[..2], [Effect::CancelSearch, Effect::ClearTt]);
        finish(&mut c, id);
        assert!(c.history().actions().is_empty());
    }

    #[test]
    fn ai_against_ai_plays_on_and_steps() {
        let mut c = ready([AI, AI]);
        let effects = c.apply(Command::Resume);
        assert!(effects.is_empty(), "the search started with the game");
        let id = c.thinking().map(|t| t.id).unwrap();
        let effects = finish(&mut c, id);
        let next = started(&effects).expect("the other side starts");
        assert_eq!(c.apply(Command::Pause), vec![Effect::CancelSearch]);
        finish(&mut c, next);
        assert_eq!(c.history().actions().len(), 1);

        let step = started(&c.apply(Command::StepAi)).unwrap();
        assert!(c.apply(Command::StepAi).is_empty(), "one search at a time");
        assert!(started(&finish(&mut c, step)).is_none(), "stays paused");
        assert_eq!(c.history().actions().len(), 2);
        assert_eq!(c.status(), Status::Paused);
        assert!(started(&c.apply(Command::Resume)).is_some());
    }

    #[test]
    fn analysis_mode_keeps_the_game_going() {
        let mut c = ready([Player::Human, AI]);
        // A person is to move: the analysis searches.
        let analysis = started(&c.apply(Command::SetAnalysis(true))).expect("analysed");
        // The move stops it, and the AI takes the engine.
        let effects = play(&mut c, "f5");
        assert!(effects.contains(&Effect::CancelSearch));
        let reply = started(&effects).expect("the AI moves");
        assert_ne!(reply, analysis);
        assert_eq!(c.thinking().map(|t| t.id), Some(reply));
        // After the reply, the new position is analysed.
        let effects = finish(&mut c, reply);
        assert_eq!(c.history().actions().len(), 2);
        assert!(c.thinking().is_none());
        assert!(started(&effects).is_some());
        // Moving back pauses the game and analyses there.
        let effects = c.apply(Command::First);
        assert!(started(&effects).is_some());
        assert_eq!(c.status(), Status::Paused);
        // A new game keeps analysis mode on and clears the AI log.
        c.apply(Command::NewGame(NewGame {
            players: [Player::Human, Player::Human],
            start: Position::initial(),
        }));
        assert!(c.analyzing());
        assert!(c.ai_log().is_empty());
        assert!(
            c.apply(Command::SetAnalysis(false))
                .contains(&Effect::CancelSearch)
        );
    }

    #[test]
    fn too_few_discs_keep_the_ai_and_analysis_off() {
        let bits = |n: &str| n.parse::<Square>().unwrap().bit();
        // Three discs; Black to move has A3.
        let start =
            Position::from_colors(bits("a1"), bits("a2") | bits("b2"), Color::Black).unwrap();
        let mut c = ready([AI, AI]);
        let effects = c.apply(Command::NewGame(NewGame {
            players: [AI, AI],
            start,
        }));
        assert!(started(&effects).is_none());
        assert!(c.ai_to_move().is_none());
        assert!(started(&c.apply(Command::SetAnalysis(true))).is_none());
    }

    #[test]
    fn a_loaded_game_opens_paused() {
        let mut c = ready([Player::Human, AI]);
        let id = started(&play(&mut c, "f5")).unwrap();
        let mut history = History::new(Position::initial());
        history
            .play(Action::Place("f5".parse().unwrap()), Grouping::NewGroup)
            .unwrap();
        let game = GameInfo::new();
        let effects = c.apply(Command::Load(Box::new(LoadedGame {
            game: game.clone(),
            players: [Player::Human, AI],
            history,
        })));
        assert_eq!(effects, [Effect::CancelSearch, Effect::ClearTt]);
        assert_eq!(c.status(), Status::Paused);
        assert_eq!(c.game(), &game);
        // The search from before the load is not applied.
        finish(&mut c, id);
        assert_eq!(c.history().actions().len(), 1);
        assert!(started(&c.apply(Command::Resume)).is_some());
    }

    #[test]
    fn nothing_starts_before_the_engine_and_failures_pause() {
        let mut c = Controller::default();
        c.apply(Command::NewGame(NewGame {
            players: [AI, Player::Human],
            start: Position::initial(),
        }));
        assert!(c.thinking().is_none());
        let effects = c.on_engine(EngineEvent::Ready {
            resources: Resources {
                threads: 1,
                tt_mib: 1,
            },
            load_time: Duration::ZERO,
        });
        let id = started(&effects).unwrap();
        c.on_engine(EngineEvent::Failed {
            id,
            error: "boom".into(),
        });
        assert_eq!(c.status(), Status::Paused);
        assert_eq!(c.notice(), Some("boom"));
        assert!(c.apply(Command::MoveNow).is_empty());
    }
}
