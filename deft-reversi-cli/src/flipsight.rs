//! FlipSight ボットコネクタ向けの JSON Lines 対局プロトコル実装モジュール。
//!
//! 標準入力から 1 行 1 手番の JSON を受け取り、Book または時間制御付き探索で
//! 最善手を選んで標準出力へ JSON として返す。

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

/// 応答の書き出しとフラッシュに備えて残す安全余裕時間（ミリ秒）。
const REPLY_MARGIN: Duration = Duration::from_millis(50);
// コネクタ側の 750 ms 打ち切りの 1.5 倍と、それまでの各手番で見込まれる 300 ms の通信遅延分を確保する。
const FINAL_TURN_RESERVE: Duration = Duration::from_millis(1_125);
/// 最終手より前の各手番で通信遅延として差し引く予約時間。
const COMMUNICATION_PER_TURN: Duration = Duration::from_millis(450);

/// 残り空きマス数・残り時間・コネクタ上限から、この手番の探索予算 [`Budget`] を算出する。
fn search_budget(empties: u8, remaining: Duration, cap: Duration) -> Budget {
    let turns_before_final = u32::from(empties.div_ceil(2).saturating_sub(1));
    // 最終手番の時計と、それ以前の各手番の通信コストを差し引いて持ち時間を見積もる。
    // 最終手番と見積もられる手では、コネクタ側の cap で探索時間が制限される。
    let search_clock = if turns_before_final == 0 {
        remaining
    } else {
        remaining.saturating_sub(FINAL_TURN_RESERVE + COMMUNICATION_PER_TURN * turns_before_final)
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
    Loading(Receiver<std::result::Result<ReadyEngine, String>>),
    // Box 化する理由: Engine はチャネル型に比べてサイズが非常に大きいため。
    /// 読み込みが完了し探索可能な状態。
    Ready(Box<ReadyEngine>),
}

/// 初期化済みのエンジンと省略可能な Book プレイヤー。
struct ReadyEngine {
    engine: Engine,
    book: Option<BookPlayer>,
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
        Ok(Self { engine, book })
    }
}

impl SearchEngine {
    /// バックグラウンドスレッドでエンジンと Book の読み込みを開始する。
    fn start(args: FlipsightArgs) -> Result<Self> {
        let (send, receive) = mpsc::channel();
        thread::Builder::new()
            .name("flipsight-model".into())
            .spawn(move || {
                let _ = send.send(ReadyEngine::load(args).map_err(|error| error.to_string()));
            })?;
        Ok(Self::Loading(receive))
    }

    /// 最大 `timeout` まで初期化完了を待ち、間に合えば [`ReadyEngine`] を返す。
    ///
    /// タイムアウトした場合は `Ok(None)` を返し、呼び出し側でフォールバック手を返せるようにする。
    fn wait(&mut self, timeout: Duration) -> Result<Option<&mut ReadyEngine>> {
        if let Self::Loading(receive) = self {
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
            Self::Loading(_) => Ok(None),
        }
    }
}

/// 標準入力から手番リクエストを逐次読み取り、決定した手を標準出力へ書き出すメインループ。
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
        let (square, eval) = match moves.next() {
            None => (-1, None),
            // 合法手が 1 手しかない場合はエンジン初期化を待たずに即座に返す。
            Some(only) if moves.is_empty() => (i16::from(only.index()), None),
            Some(first) => {
                let engine = match &mut engine {
                    Some(engine) => engine,
                    None => engine.insert(SearchEngine::start(args.clone())?),
                };
                let (square, eval) = choose(engine, &turn, position, first, received)?;
                (i16::from(square.index()), eval)
            }
        };
        serde_json::to_writer(&mut out, &Reply { square, eval })?;
        writeln!(out)?;
        out.flush()?;
    }
    Ok(())
}

/// 複数の合法手がある局面で、Book 参照または時間予算付き探索により着手と評価値を決定する。
fn choose(
    engine: &mut SearchEngine,
    turn: &Turn,
    position: Position,
    first: Square,
    received: Instant,
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
    let budget = search_budget(position.empty_count(), remaining, time_left());
    let result = if budget.hard.is_zero() {
        game_clock::reflex(&mut engine.engine, position)?
    } else {
        // 締切時のフォールバック結果がこの探索自身から得られるよう、最初のステージから始める。
        budget.search(&mut engine.engine, position, false, |_| {})?
    };
    match (result.stop_reason, result.best_move) {
        (StopReason::WorkerFailed, _) => Err("search worker failed".into()),
        (_, Some(Action::Place(square))) => {
            // ルートの評価値はすでに手番側から見た値になっている。
            let eval = result
                .score
                .filter(|score| !result.score_is_upper_bound && (-64..=64).contains(score));
            Ok((square, eval))
        }
        _ => Err("search returned no placement for a playable position".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for (empties, reserve_ms) in [(3, 1_575), (6, 2_025), (10, 2_925), (60, 14_175)] {
            let reserve = Duration::from_millis(reserve_ms);
            let cap = Duration::from_secs(60);
            assert!(search_budget(empties, reserve, cap).hard.is_zero());
            let remaining = reserve + Duration::from_secs(1);
            let hard = search_budget(empties, remaining, cap).hard;
            assert!(hard > Duration::ZERO && remaining - hard >= reserve);
        }
    }

    #[test]
    fn estimated_final_turn_can_use_the_connector_cap() {
        let cap = Duration::from_millis(350);
        for empties in [1, 2] {
            let budget = search_budget(empties, FINAL_TURN_RESERVE, cap);
            assert_eq!(budget.hard, cap);
        }
    }
}
