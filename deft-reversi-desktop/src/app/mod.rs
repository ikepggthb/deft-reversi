//! アプリケーションの状態遷移を担う純粋コントローラ層。
//!
//! ユーザー操作（[`Command`]）やエンジンからの通知（[`EngineEvent`]）を受け取り、
//! 棋譜・対局・解析・検討モード・AI ログの状態を更新する。
//! 探索の開始や効果音再生などの外部への働きかけは直接実行せず、[`Effect`] のリストとして返すため、
//! GPUI のウィンドウやバックグラウンドスレッドに依存せず単体テストができる。
use crate::{
    domain::{GameInfo, Grouping, History, Player, Strength, editor},
    persistence::record::LoadedGame,
    services::engine::{EngineEvent, Report, Resources, SearchId, SearchJob, SearchSpec},
};
use deft_reversi_engine::{Action, Color, Position, Square};
mod analysis;
pub use analysis::{AnalysisSettings, Analyzer};

/// 新規対局の開始パラメータ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewGame {
    /// 黒番（0）・白番（1）のプレイヤー設定。
    pub players: [Player; 2],
    /// 開始局面。
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

/// UI やユーザー操作からコントローラへ送られるコマンド。
#[derive(Clone, Debug)]
pub enum Command {
    /// 新しい対局を開始する。
    NewGame(NewGame),
    /// 指定マスに着手する。
    Play(Square),
    /// 直前の Undo 単位を取り消す。
    Undo,
    /// 取り消した手順をやり直す。
    Redo,
    /// 棋譜カーソルを前（負）または後（正）へ移動する。
    Step(isize),
    /// 棋譜の先頭へ移動する。
    First,
    /// 棋譜の末尾へ移動する。
    Last,
    /// 指定手数目の局面へ移動する。
    GoTo(usize),
    /// 対局を再開する（強制パス位置にいる場合はパスを進めてから AI 思考を開始する）。
    Resume,
    /// 対局を一時停止し、実行中の AI 思考をキャンセルする。
    Pause,
    /// 一時停止中に AI へ 1 手だけ打たせる。
    StepAi,
    /// 思考中の AI に現時点の最善手で即座に着手させる。
    MoveNow,
    /// エンジンのスレッド数・置換表サイズを変更する。
    SetResources(Resources),
    /// 検証済みの棋譜を読み込んで現在の対局を置き換える（一時停止状態で開く）。
    Load(Box<LoadedGame>),
    /// 解析モードを切り替える（オン時は表示中局面の解析をバックグラウンドで開始する）。
    SetAnalysis(bool),
    /// 検討モードを切り替える。
    ///
    /// 検討中は本譜を退避し、盤上で自由に別変化を試せる（解除すると本譜に戻る）。
    SetStudy(bool),
    /// 表示中局面の完全解析（選択率 100% 終局読み）を実行する。
    AnalyzeExact,
    /// 棋譜全体の未解析局面を末尾から遡って一括解析する。`redo` が `true` なら完全解析済み以外を再解析する。
    AnalyzeRecord {
        /// 完全解析済み以外の局面も再解析する場合は `true`。
        redo: bool,
    },
    /// 実行中の局面解析・一括解析を途中までの結果を保持したまま停止する。
    StopAnalysis,
    /// 表示中局面のキャッシュを破棄し、最低レベルから再解析する。
    Reanalyze,
    /// 解析レベル・悪手閾値の設定変更を反映する。
    SetAnalysisSettings(AnalysisSettings),
    /// AI の対局で Book を使用するかを切り替える。
    SetBook(bool),
    /// Book 生成のためにエンジンを占有状態にする（[`EngineEvent::BookDone`] で解除される）。
    GenerateBook,
}

/// コントローラが再生を要求する効果音の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// 着手音。
    Place,
    /// パス音。
    Pass,
    /// 終局音。
    GameOver,
}

/// コントローラがサービス層へ実行を依頼する副作用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// 探索ジョブを開始する。
    StartSearch {
        /// 開始する探索ジョブ。
        job: SearchJob,
    },
    /// 実行中の探索をキャンセルする。
    CancelSearch,
    /// 実行中の探索を打ち切って現時点の最善手を返させる。
    MoveNow,
    /// 置換表をクリアする。
    ClearTt,
    /// エンジンリソースを再構成する。
    Reconfigure(Resources),
    /// 効果音を再生する。
    Sound(Sound),
}

/// 対局の進行状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// 対局中（AI の手番になれば自動で探索を開始する）。
    Playing,
    /// 一時停止中（ユーザーの操作または再開指示まで AI は動かない）。
    Paused,
}

/// バックグラウンドエンジンの動作状態。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineStatus {
    /// モデル読み込み・再構成中。
    Loading,
    /// 探索可能。
    Ready,
    /// Book 生成でエンジンを占有中（通常探索・解析は待機する）。
    Generating,
    /// モデル不在や初期化失敗により利用不可。
    Unavailable(String),
}

/// 現在進行中の AI 思考情報。
#[derive(Clone, Debug)]
pub struct Thinking {
    id: SearchId,
    revision: u64,
    strength: Strength,
    /// `StepAi` による 1 手実行時は着手後に `Paused` へ戻す。
    single_step: bool,
    /// 思考中の手番色。
    pub color: Color,
    /// 最新の途中経過レポート。
    pub report: Option<Report>,
}

/// AI が実際に行った 1 手と、その裏付けとなる探索結果ログ。
#[derive(Clone, Debug)]
pub struct AiMove {
    /// 棋譜上の手数番号。
    pub number: usize,
    /// 着手した色。
    pub color: Color,
    /// その手番での AI 強さ設定。
    pub strength: Strength,
    /// 実際に打たれた手。
    pub action: Action,
    /// 探索結果レポート。
    pub report: Report,
}

/// 対局・解析・検討モードの状態を一元管理するコントローラ。
pub struct Controller {
    game: GameInfo,
    history: History,
    players: [Player; 2],
    status: Status,
    engine: EngineStatus,
    /// 棋譜やプレイヤー設定が変わるたびに 1 増えるリビジョン番号。
    ///
    /// AI の思考は別スレッドで非同期に進むため、思考中に Undo や新規対局が行われると
    /// 古い局面に対する結果が遅れて届くことがある。開始時の `revision` と照合して
    /// 食い違いがあれば古い結果として破棄するほか、自動保存の変更検知にも使われる。
    revision: u64,
    next_search: u64,
    thinking: Option<Thinking>,
    analyzing: bool,
    analyzer: Analyzer,
    notice: Option<String>,
    /// 現在の対局で AI が打った全手のログ（古い順。Undo しても保持される）。
    ai_log: Vec<AiMove>,
    /// Book を AI が使用するか（[`Command::SetBook`] で更新）。
    book: bool,
    /// 検討モード中に本譜を退避しておくスロット（`history` 側で別変化を操作する）。
    main: Option<History>,
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
            main: None,
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
    /// 現在画面に表示中の棋譜履歴（検討モード中は検討手順）を返す。
    pub fn history(&self) -> &History {
        &self.history
    }

    /// 対局本来の本譜を返す（検討モード中は退避中の本譜、通常時は `history`）。
    ///
    /// 自動保存・エクスポートには常にこちらを使用する。
    pub fn game_record(&self) -> &History {
        self.main.as_ref().unwrap_or(&self.history)
    }

    /// 検討モード中に退避されている本譜を返す。
    pub fn main_line(&self) -> Option<&History> {
        self.main.as_ref()
    }

    /// 現在検討モード中であるかを返す。
    pub fn studying(&self) -> bool {
        self.main.is_some()
    }

    /// 現在の対局識別情報を返す。
    pub fn game(&self) -> &GameInfo {
        &self.game
    }

    /// 保存対象の状態変更ごとにインクリメントされるリビジョン番号を返す。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 黒番・白番のプレイヤー設定を返す。
    pub fn players(&self) -> [Player; 2] {
        self.players
    }

    /// 対局の進行状態（対局中 / 一時停止）を返す。
    pub fn status(&self) -> Status {
        self.status
    }

    /// バックグラウンドエンジンの状態を返す。
    pub fn engine(&self) -> &EngineStatus {
        &self.engine
    }

    /// 現在 AI が思考中であればその情報を返す。
    pub fn thinking(&self) -> Option<&Thinking> {
        self.thinking.as_ref()
    }

    /// エンジンイベントが現在思考中の AI の着手完了通知であるかを判定する。
    pub fn is_ai_move(&self, event: &EngineEvent) -> bool {
        matches!(event, EngineEvent::Finished { id, .. }
            if self.thinking.as_ref().is_some_and(|t| t.id == *id))
    }

    /// 解析モードが有効かを返す。
    pub fn analyzing(&self) -> bool {
        self.analyzing
    }

    /// 局面解析器への参照を返す。
    pub fn analyzer(&self) -> &Analyzer {
        &self.analyzer
    }

    /// AI の着手ログ一覧を返す。
    pub fn ai_log(&self) -> &[AiMove] {
        &self.ai_log
    }

    /// 直近の探索エラーメッセージ（次のコマンド実行まで保持）を返す。
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// 少なくとも一方のプレイヤーが AI であるかを返す。
    pub fn has_ai(&self) -> bool {
        self.players.iter().any(|p| p.is_ai())
    }

    /// 表示中の局面で AI が着手可能であればその強さ設定を返す。
    pub fn ai_to_move(&self) -> Option<Strength> {
        let position = self.history.current();
        if position.legal_moves().is_empty() || !editor::searchable(position) {
            return None;
        }
        match self.players[index(position.side_to_move())] {
            Player::Ai(strength) => Some(strength),
            Player::Human => None,
        }
    }

    /// ユーザー操作等のコマンドを適用し、発生した副作用のリストを返す。
    pub fn apply(&mut self, command: Command) -> Vec<Effect> {
        self.notice = None;
        let mut effects = Vec::new();
        match command {
            Command::NewGame(new_game) => {
                self.cancel(&mut effects);
                self.main = None;
                // 解析モードのオン/オフ設定は新しい対局を始めても維持する。
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
            Command::Resume => {
                if self.history.advance_pass() {
                    self.revision += 1;
                }
                self.status = Status::Playing;
            }
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
                self.analyzer.cancel(&mut effects);
                self.main = None;
                let LoadedGame {
                    game,
                    players,
                    history,
                } = *loaded;
                self.game = game;
                self.players = players;
                self.history = history;
                self.ai_log.clear();
                // ファイルから開いた対局は勝手に AI が打ち始めないよう一時停止状態で開く。
                self.status = Status::Paused;
                self.revision += 1;
                effects.push(Effect::ClearTt);
            }
            // 解析モード中も対局は止めない。AI の手番では AI 思考がエンジンを使い、
            // 人間の手番でエンジンが空いている間に局面解析を進める。
            Command::SetStudy(on) => self.set_study(on, &mut effects),
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
            Command::Reanalyze => {
                if self.analyzing {
                    let position = self.history.current();
                    self.analyzer.reanalyze(position, &mut effects);
                }
            }
            Command::SetAnalysisSettings(settings) => self.analyzer.settings_changed(settings),
            Command::SetBook(book) => self.book = book,
            // Book 生成中はエンジンを占有するため、AI の着手も局面解析も完了まで待機させる。
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

    /// エンジンイベントを処理して状態を更新し、後続の副作用（次の AI 探索や解析など）を返す。
    pub fn on_engine(&mut self, event: EngineEvent) -> Vec<Effect> {
        let mut effects = Vec::new();
        match event {
            EngineEvent::Ready { .. } => self.engine = EngineStatus::Ready,
            // モデル情報・Book 状態・Book 生成の途中経過は画面表示用なので、コントローラ側では何もしない。
            EngineEvent::Model(_) | EngineEvent::Book(_) | EngineEvent::BookProgress(_) => {}
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

    /// 指定 `id` が現在思考中の探索と一致し、かつ開始時点から盤面・プレイヤーが変化していない場合のみ返す。
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
        // 一時停止中は、検討や実験のために AI 側の手番でも人間が代わりに石を置けるようにする。
        if !human_turn && self.status != Status::Paused {
            return;
        }
        if !self.history.current().legal_moves().contains(square) {
            return;
        }
        self.cancel(effects);
        self.record(Action::Place(square), Grouping::NewGroup, effects);
        // 検討モードでは黒番・白番のどちらも人間が手動で並べて変化を調べる。
        if human_turn && !self.studying() {
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
        // 編集局面から始めた対局でも手数が破綻しないよう、盤上の石数から手数を求める。
        let discs = position.disc_count(Color::Black) + position.disc_count(Color::White);
        self.ai_log.push(AiMove {
            number: discs.saturating_sub(3) as usize,
            color: thinking.color,
            strength: thinking.strength,
            action: best_move.expect("checked legal"),
            report,
        });
        // 人間対 AI 戦では、1 回の Undo で人間の直前の手までまとめて戻せるよう前の手に結合する。
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

    /// 検討モードの切り替え。開始時は表示中の局面で一時停止し、終了時は退避した本譜を復元する。
    fn set_study(&mut self, on: bool, effects: &mut Vec<Effect>) {
        if on == self.studying() {
            return;
        }
        self.cancel(effects);
        if on {
            self.main = Some(self.history.clone());
            self.status = Status::Paused;
        } else if let Some(main) = self.main.take() {
            self.history = main;
            self.revision += 1;
            if self.has_ai() {
                self.status = Status::Paused;
            }
        }
    }

    /// 棋譜カーソル移動時の共通処理。移動に成功したら AI 思考を停止し、AI 戦なら一時停止状態にする。
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

    /// エンジンが空きかつ解析モード中であれば、解析器に次の探索を起動させる（解析探索の唯一のエントリポイント）。
    fn pump_analysis(&mut self, effects: &mut Vec<Effect>) {
        if self.analyzing && self.thinking.is_none() && self.engine == EngineStatus::Ready {
            let shown = self.history.current();
            self.analyzer.pump(shown, &mut self.next_search, effects);
        }
    }

    /// 対局中かつ AI の手番であれば自動で AI 思考を開始する（自動着手の唯一のエントリポイント）。
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
        // 対局の着手を最優先し、進行中の解析は中断する（AI の着手後に自動で再開される）。
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
                    ..SearchSpec::from(strength)
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
    fn study_mode_plays_other_lines_and_keeps_the_record() {
        let mut c = ready([Player::Human, Player::Human]);
        for name in ["f5", "d6", "c3"] {
            play(&mut c, name);
        }
        c.apply(Command::SetStudy(true));
        assert!(c.studying());
        c.apply(Command::GoTo(1));
        play(&mut c, "f6");
        play(&mut c, "e6");
        // The line studied is shown; the record is what is saved.
        assert_eq!(c.history().actions().len(), 3);
        assert_eq!(
            c.history().actions()[1],
            Action::Place("f6".parse().unwrap())
        );
        assert_eq!(
            c.game_record().actions()[1],
            Action::Place("d6".parse().unwrap())
        );
        // Leaving brings the record back where it was shown.
        c.apply(Command::SetStudy(false));
        assert!(!c.studying());
        assert_eq!(
            c.history().actions()[1],
            Action::Place("d6".parse().unwrap())
        );
        assert_eq!(c.history().cursor(), 3);
    }

    #[test]
    fn a_study_line_waits_for_the_user_on_both_sides() {
        let mut c = ready([Player::Human, Player::Ai(Strength::Level(8))]);
        let id = started(&play(&mut c, "f5")).expect("the AI thinks");
        finish(&mut c, id);
        c.apply(Command::SetStudy(true));
        c.apply(Command::GoTo(0));
        // Neither side searches by itself in the study line.
        assert!(started(&play(&mut c, "d3")).is_none());
        assert_eq!(c.status(), Status::Paused);
        assert!(started(&play(&mut c, "c3")).is_none());
    }

    #[test]
    fn the_user_can_resume_or_step_the_ai_in_a_study_line() {
        for (command, status) in [
            (Command::Resume, Status::Playing),
            (Command::StepAi, Status::Paused),
        ] {
            let mut c = ready([Player::Human, AI]);
            c.apply(Command::SetStudy(true));
            assert!(started(&play(&mut c, "d3")).is_none());
            let id = started(&c.apply(command)).expect("the AI thinks on request");
            finish(&mut c, id);
            assert_eq!(c.status(), status);
            assert_eq!(c.history().actions().len(), 2);
            assert!(c.game_record().actions().is_empty());
        }
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
    fn opening_another_game_cancels_the_old_record_search() {
        let mut c = ready([Player::Human; 2]);
        play(&mut c, "f5");
        c.apply(Command::SetAnalysis(true));
        c.apply(Command::AnalyzeRecord { redo: false });
        let old = started(&c.apply(Command::First)).unwrap();
        assert_eq!(c.analyzer().batch_progress(), Some((1, 2)));

        let history = History::new(Position::initial());
        let effects = c.apply(Command::Load(Box::new(LoadedGame {
            game: GameInfo::new(),
            players: [Player::Human; 2],
            history,
        })));
        let [
            Effect::CancelSearch,
            Effect::ClearTt,
            Effect::StartSearch { job },
        ] = &effects[..]
        else {
            panic!("expected a new search after clearing the old one: {effects:?}");
        };
        assert_eq!(job.position, c.history().current());
        assert!(job.spec.root_moves.is_some());
        assert_eq!(c.analyzer().batch_progress(), None);
        assert!(finish(&mut c, old).is_empty());
        assert!(c.analyzer().is_searching(job.position));
    }

    #[test]
    fn resume_follows_the_recorded_pass_before_starting_the_next_turn() {
        let bit = |name: &str| name.parse::<Square>().unwrap().bit();
        let start =
            Position::from_colors(bit("a1") | bit("d1"), bit("b1") | bit("a2"), Color::White)
                .unwrap();
        for players in [[AI, Player::Human], [Player::Human, AI]] {
            let mut history = History::new(start);
            history
                .play(Action::Place("c1".parse().unwrap()), Grouping::NewGroup)
                .unwrap();
            let actions = history.actions().to_vec();
            let mut c = ready(players);
            c.apply(Command::Load(Box::new(LoadedGame {
                game: GameInfo::new(),
                players,
                history,
            })));
            c.apply(Command::First);
            let revision = c.revision();
            let effects = c.apply(Command::Resume);
            assert_eq!(c.history().cursor(), 1);
            assert_eq!(c.history().actions(), actions);
            assert_eq!(c.revision(), revision + 1);
            assert_eq!(c.history().current().side_to_move(), Color::Black);
            assert_eq!(started(&effects).is_some(), players[0].is_ai());
            if !players[0].is_ai() {
                play(&mut c, "c1");
                assert_eq!(c.history().cursor(), 3);
            }
        }
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
