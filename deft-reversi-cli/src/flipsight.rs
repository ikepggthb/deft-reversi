//! FlipSight ボットコネクタ向けの JSON Lines 対局プロトコル実装モジュール。
//!
//! 標準入力から 1 行 1 手番の JSON を受け取り、Book または時間制御付き探索で
//! 最善手を選んで標準出力へ JSON として返す。

use crate::{
    EngineArgs, Result, book_view,
    game_clock::{self, Budget},
};
use clap::Args;
use deft_reversi_book::{BookPlayer, MovePolicy};
use deft_reversi_engine::{
    Action, Color, EngineWorker, Moves, Position, SearchHandle, SearchRequest, Square, StopReason,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

/// 応答の書き出しとフラッシュに備えて残す安全余裕時間（ミリ秒）。
const REPLY_MARGIN: Duration = Duration::from_millis(50);
// コネクタ側の 750 ms 打ち切りの 1.5 倍と、それまでの各手番で見込まれる 300 ms の通信遅延分を確保する。
const FINAL_TURN_RESERVE: Duration = Duration::from_millis(1_125);
/// 最終手より前の各手番で通信遅延として差し引く既定の予約時間（ミリ秒）。
const DEFAULT_COMMUNICATION_PER_TURN_MS: u64 = 450;
/// 先読み探索に与える時間上限（次の手番到着時に `StopHandle` で即座に停止する）。
const PONDER_LIMIT: Duration = Duration::from_secs(3_600);

/// 残り空きマス数・残り時間・コネクタ上限・1 手あたり通信予約時間から、この手番の探索予算 [`Budget`] を算出する。
fn search_budget(
    empties: u8,
    remaining: Duration,
    cap: Duration,
    comm_per_turn: Duration,
) -> Budget {
    let turns_before_final = u32::from(empties.div_ceil(2).saturating_sub(1));
    // 最終手番の時計と、それ以前の各手番の通信コストを差し引いて持ち時間を見積もる。
    // 最終手番と見積もられる手では、コネクタ側の cap で探索時間が制限される。
    let search_clock = if turns_before_final == 0 {
        remaining
    } else {
        remaining.saturating_sub(FINAL_TURN_RESERVE + comm_per_turn * turns_before_final)
    };
    Budget::new(empties, search_clock, cap)
}

/// `flipsight` サブコマンドの引数。
#[derive(Args, Clone, Debug)]
pub struct FlipsightArgs {
    #[command(flatten)]
    engine: EngineArgs,
    /// 探索の前に参照する Book ファイル（`.dbook`）。
    #[arg(long)]
    book: Option<PathBuf>,
    /// 相手の手番中もバックグラウンドで先読み探索を行う。
    #[arg(long)]
    ponder: bool,
    /// 最終手より前の 1 手番あたりに通信遅延として差し引く予約時間（ミリ秒）。
    #[arg(long, default_value_t = DEFAULT_COMMUNICATION_PER_TURN_MS)]
    comm_ms: u64,
}

/// コネクタから 1 行ごとに送られる手番リクエスト。
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
    /// 受信した盤面・手番・合法手を検証し、エンジンの [`Position`] と合法手集合 [`Moves`] に変換する。
    fn position(&self) -> Result<(Position, Moves)> {
        if self.game_mode.as_deref().unwrap_or("std") != "std"
            || self.board_size.unwrap_or(8) != 8
            || !self.blocked_squares.is_empty()
        {
            return Err("FlipSight engine supports the standard 8x8 board only".into());
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
        // 両プロトコルとも A1..H8 を 0 始まりの行優先順で番号付けしている。
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

/// コネクタへ 1 行で返す応答 JSON。
#[derive(Serialize)]
struct Reply {
    /// 着手マス番号（0〜63）。パス時は `-1`。
    #[serde(rename = "move")]
    square: i16,
    /// 手番側から見た評価値（石差）。Book 利用時や未確定時は省略する。
    #[serde(skip_serializing_if = "Option::is_none")]
    eval: Option<i32>,
}

/// エンジンおよび Book の初期化状態。
enum SearchEngine {
    /// 別スレッドでモデルと Book を読み込み中。
    Loading {
        receive: Receiver<std::result::Result<ReadyEngine, String>>,
        /// 読み込み完了後にすぐ先読みを開始する保留局面。
        pending_ponder: Option<Position>,
    },
    // Box 化する理由: Engine はチャネル型に比べてサイズが非常に大きいため。
    /// 読み込みが完了し探索可能な状態。
    Ready(Box<ReadyEngine>),
}

/// 実行中の先読み探索の対象局面とハンドル。
struct PonderState {
    position: Position,
    handle: SearchHandle,
}

/// 先読み探索の停止結果。
#[derive(Clone, Copy, Debug)]
struct PonderOutcome {
    /// 先読みで予測した相手最善応手（またはパス後局面）と実際の手番局面が一致したか。
    hit: bool,
    /// 先読み探索に費やした実時間。
    elapsed: Duration,
}

/// 初期化済みのエンジンと省略可能な Book プレイヤー。
struct ReadyEngine {
    worker: EngineWorker,
    book: Option<BookPlayer>,
    ponder: Option<PonderState>,
}

impl ReadyEngine {
    /// モデルファイルと指定された Book を読み込む。
    fn load(args: FlipsightArgs) -> Result<Self> {
        let engine = args.engine.engine()?;
        let book = args
            .book
            .map(|path| book_view::load(&engine, &path, MovePolicy::Best, 0))
            .transpose()?
            .map(|(player, _, _)| player);
        Ok(Self {
            worker: EngineWorker::new(engine),
            book,
            ponder: None,
        })
    }

    /// 相手手番（または強制パス後の局面）に対してバックグラウンドの先読み探索を開始する。
    fn start_ponder(&mut self, position: Position) {
        if position.is_terminal() {
            return;
        }
        let request = SearchRequest::timed_levels(60, PONDER_LIMIT).skip_pv_recovery();
        if let Ok(handle) = self.worker.start(position, request) {
            self.ponder = Some(PonderState { position, handle });
        }
    }

    /// 実行中の先読み探索があれば停止し、ワーカースレッドの終了を待って先読み結果を返す。
    fn stop_ponder(&mut self, next_position: Option<Position>) -> Result<Option<PonderOutcome>> {
        let Some(state) = self.ponder.take() else {
            return Ok(None);
        };
        state.handle.stop();
        let result = state.handle.wait()?;
        if result.stop_reason == StopReason::WorkerFailed {
            return Err("ponder worker failed".into());
        }
        let hit = is_ponder_hit(state.position, next_position, result.kind, result.best_move);
        Ok(Some(PonderOutcome {
            hit,
            elapsed: result.elapsed,
        }))
    }
}

/// 先読みが少なくとも 1 反復完了しており、予測した相手最善応手（またはパス後局面）が
/// 実際の手番局面と一致した場合に `true` を返す。
fn is_ponder_hit(
    ponder_position: Position,
    next_position: Option<Position>,
    kind: Option<deft_reversi_engine::ResultKind>,
    best_move: Option<Action>,
) -> bool {
    match (next_position, kind, best_move) {
        (Some(actual), Some(_), _) if actual == ponder_position => true,
        (Some(actual), Some(_), Some(predicted)) => {
            ponder_position.play(predicted).ok() == Some(actual)
        }
        _ => false,
    }
}

impl SearchEngine {
    /// バックグラウンドスレッドでエンジンと Book の読み込みを開始し、完了時に `notify` へ通知する。
    fn start(args: FlipsightArgs, notify: mpsc::Sender<LoopEvent>) -> Result<Self> {
        let (send, receive) = mpsc::channel();
        thread::Builder::new()
            .name("flipsight-model".into())
            .spawn(move || {
                let loaded = ReadyEngine::load(args).map_err(|error| error.to_string());
                let _ = send.send(loaded);
                let _ = notify.send(LoopEvent::ModelReady);
            })?;
        Ok(Self::Loading {
            receive,
            pending_ponder: None,
        })
    }

    /// 最大 `timeout` まで初期化完了を待ち、間に合えば [`ReadyEngine`] を返す。
    ///
    /// タイムアウトした場合は `Ok(None)` を返し、呼び出し側でフォールバック手を返せるようにする。
    fn wait(&mut self, timeout: Duration) -> Result<Option<&mut ReadyEngine>> {
        if let Self::Loading { receive, .. } = self {
            match receive.recv_timeout(timeout) {
                Ok(engine) => *self = Self::Ready(Box::new(engine?)),
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("engine initialization failed".into());
                }
            }
        }
        match self {
            Self::Ready(engine) => Ok(Some(engine)),
            Self::Loading { .. } => Ok(None),
        }
    }

    /// モデル読み込み完了通知を受け取り、保留中の先読み局面があれば即座に開始する。
    fn on_model_ready(&mut self) -> Result<()> {
        let Self::Loading {
            receive,
            pending_ponder,
        } = self
        else {
            return Ok(());
        };
        let pending = *pending_ponder;
        match receive.try_recv() {
            Ok(engine) => {
                let mut ready = Box::new(engine?);
                if let Some(position) = pending {
                    ready.start_ponder(position);
                }
                *self = Self::Ready(ready);
                Ok(())
            }
            Err(mpsc::TryRecvError::Empty) => Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => Err("engine initialization failed".into()),
        }
    }

    /// エンジンが初期化済みであれば実行中の先読み探索を停止し、結果を返す。
    /// 読み込み中の場合は保留していた先読み局面を取り消す。
    fn stop_ponder(&mut self, next_position: Option<Position>) -> Result<Option<PonderOutcome>> {
        match self {
            Self::Ready(engine) => engine.stop_ponder(next_position),
            Self::Loading { pending_ponder, .. } => {
                *pending_ponder = None;
                Ok(None)
            }
        }
    }

    /// 指定局面での先読み探索を開始する。モデル読み込み中の場合は完了時に開始できるよう保留する。
    fn start_ponder(&mut self, position: Position) -> Result<()> {
        if position.is_terminal() {
            return Ok(());
        }
        if let Self::Loading {
            receive,
            pending_ponder,
        } = self
        {
            match receive.try_recv() {
                Ok(engine) => *self = Self::Ready(Box::new(engine?)),
                Err(mpsc::TryRecvError::Empty) => {
                    *pending_ponder = Some(position);
                    return Ok(());
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("engine initialization failed".into());
                }
            }
        }
        if let Self::Ready(engine) = self {
            engine.start_ponder(position);
        }
        Ok(())
    }
}

/// メインループで処理するイベント。
enum LoopEvent {
    Line(String, Instant),
    StdinError(io::Error),
    StdinClosed,
    ModelReady,
}

/// 標準入力から手番リクエストを逐次読み取り、決定した手を標準出力へ書き出すメインループ。
pub fn run(args: FlipsightArgs) -> Result<()> {
    let comm_per_turn = Duration::from_millis(args.comm_ms);
    let ponder = args.ponder;
    let (event_tx, event_rx) = mpsc::channel();
    // 初手 JSON 到着前からモデルと Book の読み込みをバックグラウンドで先行開始する。
    let mut engine = SearchEngine::start(args, event_tx.clone())?;
    thread::Builder::new()
        .name("flipsight-stdin".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                let event = match line {
                    Ok(line) => LoopEvent::Line(line, Instant::now()),
                    Err(err) => LoopEvent::StdinError(err),
                };
                if event_tx.send(event).is_err() {
                    return;
                }
            }
            let _ = event_tx.send(LoopEvent::StdinClosed);
        })?;

    let mut out = io::stdout().lock();
    let result = (|| -> Result<()> {
        while let Ok(event) = event_rx.recv() {
            let (line, received) = match event {
                LoopEvent::Line(line, received) => (line, received),
                LoopEvent::StdinError(err) => return Err(err.into()),
                LoopEvent::StdinClosed => break,
                LoopEvent::ModelReady => {
                    engine.on_model_ready()?;
                    continue;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let turn: Turn = serde_json::from_str(&line)?;
            let (position, mut moves) = turn.position()?;
            let ponder_outcome = engine.stop_ponder(Some(position))?;
            let (square, eval) = match moves.next() {
                None => (-1, None),
                // 合法手が 1 手しかない場合はエンジン初期化を待たずに即座に返す。
                Some(only) if moves.is_empty() => (i16::from(only.index()), None),
                Some(first) => {
                    let (square, eval) = choose(
                        &mut engine,
                        &turn,
                        position,
                        first,
                        received,
                        comm_per_turn,
                        ponder_outcome,
                    )?;
                    (i16::from(square.index()), eval)
                }
            };
            serde_json::to_writer(&mut out, &Reply { square, eval })?;
            writeln!(out)?;
            out.flush()?;
            if ponder {
                let action = if square < 0 {
                    Action::Pass
                } else {
                    Action::Place(Square::new(square as u8).ok_or("invalid move square")?)
                };
                if let Ok(next_position) = position.play(action) {
                    engine.start_ponder(next_position)?;
                }
            }
        }
        Ok(())
    })();
    let _ = engine.stop_ponder(None);
    result
}

/// 複数の合法手がある局面で、Book 参照または時間予算付き探索により着手と評価値を決定する。
fn choose(
    engine: &mut SearchEngine,
    turn: &Turn,
    position: Position,
    first: Square,
    received: Instant,
    comm_per_turn: Duration,
    ponder_outcome: Option<PonderOutcome>,
) -> Result<(Square, Option<i32>)> {
    let hard_limit = Duration::from_millis(turn.budget_ms.min(turn.remaining_ms));
    let time_left = || hard_limit.saturating_sub(received.elapsed() + REPLY_MARGIN);
    // コネクタは初手時にプロセスを起動するため、モデル読込もこの手番の締切内に収める必要がある。
    let Some(engine) = engine.wait(time_left())? else {
        return Ok((first, None));
    };
    if let Some(choice) = engine.book.as_mut().and_then(|book| book.choose(position))
        && let Action::Place(square) = choice.action
    {
        return Ok((square, None));
    }
    let remaining =
        Duration::from_millis(turn.remaining_ms).saturating_sub(received.elapsed() + REPLY_MARGIN);
    let mut budget = search_budget(position.empty_count(), remaining, time_left(), comm_per_turn);
    if let Some(outcome) = ponder_outcome
        && outcome.hit
    {
        budget = budget.with_ponder_hit(outcome.elapsed);
    }
    // 先読みが直前に走っていた場合は置換表の世代を引き継ぎ、ウォームスタートも有効化する。
    let reuse_pv = ponder_outcome.is_some();
    let keep_generation = ponder_outcome.is_some();
    let result = engine.worker.with(|eng| {
        if budget.hard.is_zero() {
            if reuse_pv {
                let cached = SearchRequest::timed_levels(60, Duration::ZERO)
                    .skip_pv_recovery()
                    .reuse_pv()
                    .keep_generation();
                let res = eng.search(position, &cached)?;
                if res.warm_score.is_some() {
                    return Ok(res);
                }
            }
            game_clock::reflex(eng, position)
        } else {
            budget.search_with_options(eng, position, reuse_pv, keep_generation, |_| {})
        }
    })?;
    match (result.stop_reason, result.best_move) {
        (StopReason::WorkerFailed, _) => Err("search worker failed".into()),
        (_, Some(Action::Place(square))) => {
            // ルートの評価値はすでに手番側から見た値になっている。
            // ウォームスタート後の初段途中で時間切れになった場合は置換表の暫定評価値を使う。
            let eval = result
                .score
                .filter(|_| !result.score_is_upper_bound)
                .or(result.warm_score)
                .filter(|score| (-64..=64).contains(score));
            Ok((square, eval))
        }
        _ => Err("search returned no placement for a playable position".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Engine, EngineOptions, Model, PatternModel};

    #[test]
    fn move_reply_includes_only_available_evaluation() {
        let scored = Reply {
            square: 19,
            eval: Some(-2),
        };
        let unscored = Reply {
            square: 19,
            eval: None,
        };
        assert_eq!(
            serde_json::to_value(scored).unwrap(),
            serde_json::json!({"move": 19, "eval": -2})
        );
        assert_eq!(
            serde_json::to_value(unscored).unwrap(),
            serde_json::json!({"move": 19})
        );
    }

    #[test]
    fn search_protects_the_final_turn_and_communication() {
        let comm = Duration::from_millis(DEFAULT_COMMUNICATION_PER_TURN_MS);
        for (empties, reserve_ms) in [(3, 1_575), (6, 2_025), (10, 2_925), (60, 14_175)] {
            let reserve = Duration::from_millis(reserve_ms);
            let cap = Duration::from_secs(60);
            assert!(search_budget(empties, reserve, cap, comm).hard.is_zero());
            let remaining = reserve + Duration::from_secs(1);
            let hard = search_budget(empties, remaining, cap, comm).hard;
            assert!(hard > Duration::ZERO && remaining - hard >= reserve);
        }
    }

    #[test]
    fn estimated_final_turn_can_use_the_connector_cap() {
        let comm = Duration::from_millis(DEFAULT_COMMUNICATION_PER_TURN_MS);
        let cap = Duration::from_millis(350);
        for empties in [1, 2] {
            let budget = search_budget(empties, FINAL_TURN_RESERVE, cap, comm);
            assert_eq!(budget.hard, cap);
        }
    }

    #[test]
    fn ponder_starts_and_stops_cleanly() {
        let engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().threads(1).tt_mib(1),
        )
        .unwrap();
        let mut ready = ReadyEngine {
            worker: EngineWorker::new(engine),
            book: None,
            ponder: None,
        };
        let pos = Position::initial()
            .play(Action::Place(Square::new(19).unwrap()))
            .unwrap();
        ready.start_ponder(pos);
        assert!(ready.ponder.is_some());
        thread::sleep(Duration::from_millis(30));
        let outcome = ready.stop_ponder(Some(pos)).unwrap().unwrap();
        assert!(outcome.hit);
        assert!(ready.ponder.is_none());
        assert!(!ready.worker.is_busy());
    }

    #[test]
    fn ponder_hit_requires_completed_iteration() {
        let pos = Position::initial()
            .play(Action::Place(Square::new(19).unwrap()))
            .unwrap();
        let first_reply = Action::Place(pos.legal_moves().next().unwrap());
        let after_reply = pos.play(first_reply).unwrap();
        assert!(!is_ponder_hit(
            pos,
            Some(after_reply),
            None,
            Some(first_reply)
        ));
        let completed = Some(deft_reversi_engine::ResultKind::Depth {
            depth: 6,
            selectivity: deft_reversi_engine::Selectivity::Full,
        });
        assert!(is_ponder_hit(
            pos,
            Some(after_reply),
            completed,
            Some(first_reply)
        ));
    }

    #[test]
    fn pending_ponder_starts_when_model_finishes_loading() {
        let (send, receive) = mpsc::channel();
        let mut search_engine = SearchEngine::Loading {
            receive,
            pending_ponder: None,
        };
        let pos = Position::initial()
            .play(Action::Place(Square::new(19).unwrap()))
            .unwrap();
        search_engine.start_ponder(pos).unwrap();
        let engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().threads(1).tt_mib(1),
        )
        .unwrap();
        send.send(Ok(ReadyEngine {
            worker: EngineWorker::new(engine),
            book: None,
            ponder: None,
        }))
        .unwrap();
        search_engine.on_model_ready().unwrap();
        let SearchEngine::Ready(ready) = &search_engine else {
            panic!("engine should be ready");
        };
        assert!(ready.ponder.is_some());
        let _ = search_engine.stop_ponder(None).unwrap();
    }
}
