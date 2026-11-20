//! What one side remembers between its turns of a game. Budgets are still
//! recomputed from the clock on every turn; this only carries what the
//! clock does not show.
use super::parts::Transition;
use deft_reversi_engine::{Action, Position, ResultKind};
use std::time::Duration;

/// The side's previous searched turn.
#[derive(Clone, Debug)]
pub(crate) struct TurnSummary {
    pub position: Position,
    pub used: Duration,
    /// The last completed iteration, if any.
    pub reached: Option<ResultKind>,
    /// The principal variation, starting with the move played.
    pub pv: Vec<Action>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct GameContext {
    /// Game number in this process, from 0.
    pub game: u64,
    pub previous: Option<TurnSummary>,
    /// Whether the position of this turn is the one the previous turn's pv
    /// expected (the opponent played the predicted reply); None without a
    /// previous pv of two moves.
    pub reply_matched: Option<bool>,
    /// ln(actual / predicted) of iterations finished this game, oldest first.
    pub misses: Vec<(Transition, f64)>,
    /// A turn of this game ran in extension mode (see `extension`), so the
    /// following turns do not play as a real game would.
    pub extended: bool,
    started: bool,
}

/// Misses kept per game; older ones are dropped.
const MISSES: usize = 256;

impl GameContext {
    /// Forget the game (a new one starts).
    pub fn new_game(&mut self) {
        if self.started {
            self.game += 1;
        }
        self.previous = None;
        self.reply_matched = None;
        self.misses.clear();
        self.extended = false;
    }

    /// At the start of each of this side's turns. A position with no fewer
    /// empties than the previous turn's starts a new game.
    pub fn begin_turn(&mut self, position: Position) {
        if let Some(previous) = &self.previous
            && position.empty_count() >= previous.position.empty_count()
        {
            self.new_game();
        }
        self.started = true;
        self.reply_matched = self
            .previous
            .as_ref()
            .and_then(expected)
            .map(|p| p == position);
    }

    /// At the end of each of this side's turns, searched or not.
    pub fn end_turn(&mut self, summary: TurnSummary) {
        self.previous = Some(summary);
    }

    pub fn record_miss(&mut self, transition: Transition, log_ratio: f64) {
        if self.misses.len() == MISSES {
            self.misses.remove(0);
        }
        self.misses.push((transition, log_ratio));
    }
}

/// The position after the previous turn's move and the expected reply.
fn expected(previous: &TurnSummary) -> Option<Position> {
    let [mine, reply, ..] = previous.pv.as_slice() else {
        return None;
    };
    previous.position.play(*mine).ok()?.play(*reply).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(position: Position, pv: &[&str]) -> TurnSummary {
        TurnSummary {
            position,
            used: Duration::ZERO,
            reached: None,
            pv: pv.iter().map(|m| m.parse().unwrap()).collect(),
        }
    }

    #[test]
    fn the_expected_reply_is_recognised() {
        let start = Position::initial();
        let mut ctx = GameContext::default();
        ctx.begin_turn(start);
        assert_eq!(ctx.reply_matched, None);
        ctx.end_turn(summary(start, &["f5", "d6"]));
        let after = |reply: &str| {
            start
                .play("f5".parse::<Action>().unwrap())
                .unwrap()
                .play(reply.parse::<Action>().unwrap())
                .unwrap()
        };
        ctx.begin_turn(after("d6"));
        assert_eq!(ctx.reply_matched, Some(true));
        ctx.end_turn(summary(start, &["f5", "d6"]));
        ctx.begin_turn(after("f6"));
        assert_eq!(ctx.reply_matched, Some(false));
    }

    #[test]
    fn more_empties_start_a_new_game() {
        let start = Position::initial();
        let later = start.play("f5".parse::<Action>().unwrap()).unwrap();
        let mut ctx = GameContext::default();
        ctx.begin_turn(later);
        ctx.extended = true;
        ctx.record_miss(Transition::Mid, 0.1);
        ctx.end_turn(summary(later, &[]));
        assert_eq!(ctx.game, 0);
        ctx.begin_turn(start);
        assert_eq!(ctx.game, 1);
        assert!(ctx.previous.is_none() && ctx.misses.is_empty() && !ctx.extended);
    }
}
