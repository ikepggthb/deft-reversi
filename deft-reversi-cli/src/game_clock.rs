//! 1 局全体の持ち時間配分と、反復深化の次段階コスト予測による探索打ち切り制御モジュール。
//!
//! 通信遅延や応答マージンなどのプロトコル固有の余裕時間は呼び出し側（`gtp` や `flipsight`）が
//! 差し引いたうえで [`Budget::new`] に渡す。

use crate::stop;
use deft_reversi_engine::{
    Action, Engine, EngineError, Goal, Position, Progress, ResultKind, SearchRequest, SearchResult,
    Selectivity, StopReason,
};
use std::time::Duration;

/// 1 手分の探索時間予算。
#[derive(Clone, Copy)]
pub(crate) struct Budget {
    /// 平常時の目標時間。最善手が揺らいでいるときは `hard` を上限に延長を許す。
    soft: Duration,
    /// いかなる場合も超えてはならない絶対上限時間。
    pub hard: Duration,
}

impl Budget {
    /// 空きマス数 `empties`、残り持ち時間 `remaining`、1 手上限 `cap` から探索時間予算を決める。
    pub fn new(empties: u8, remaining: Duration, cap: Duration) -> Self {
        // レベル表の計測では、完全読みは 17 空きで約 6 ms かかり、空きマスが 1 増えるごとに約 2 倍になる。
        // ここでは将来の手番の必要時間だけをこの事前見積もりで確保する（現在の局面は平均よりずっと重い場合があるため）。
        let turns = u32::from(empties.div_ceil(2)).max(1);
        let mut pool = remaining;
        let mut turns_left = turns;
        for distance in (1..turns).rev() {
            let future = i32::from(empties) - 2 * distance as i32;
            // 低速な環境、冷えた置換表、局面の難易度のばらつきを見込む。
            // また、どれほど軽い終盤手番でも整数ミリ秒時計上で往復分の余裕が必要になる。
            let solve = Duration::from_millis(2).saturating_add(Duration::from_secs_f64(
                3.0 * 0.006 * 2_f64.powi(future - 17),
            ));
            if solve >= pool / turns_left {
                break;
            }
            pool -= solve;
            turns_left -= 1;
        }
        let share = pool / turns_left;
        // 将来の予算の半分以上を残す範囲で、最大 1 手番分まで前借りして `hard` 上限とする。
        // すでに差し引いた終盤完全読み用の確保時間はそのまま維持される。
        let hard = share.saturating_add(share.min((pool - share) / 2)).min(cap);
        Self {
            soft: share.min(hard),
            hard,
        }
    }

    /// 予算 `self` に従って反復深化探索を実行し、次段階が予算を超えると予測した時点で停止する。
    pub fn search(
        self,
        engine: &mut Engine,
        pos: Position,
        reuse_pv: bool,
        mut report: impl FnMut(&Progress),
    ) -> Result<SearchResult, EngineError> {
        let request = SearchRequest::timed_levels(60, self.hard).skip_pv_recovery();
        let request = if reuse_pv {
            request.reuse_pv()
        } else {
            request
        };
        let handle = request.stop_handle();
        let _armed = stop::arm(handle.clone());
        let mut forecast = Forecast {
            empties: pos.empty_count(),
            ..Forecast::default()
        };
        let mut forecast_stopped = false;
        let mut result = engine.search_with(pos, &request, |p| {
            report(p);
            if forecast.stop(p.elapsed, p.kind, p.next, p.best_move, self) {
                forecast_stopped = true;
                handle.stop();
            }
        })?;
        // 予測停止も Ctrl-C と同じ StopHandle で止めるため、ログで区別できるよう TimeLimit へ変換する。
        if forecast_stopped && result.stop_reason == StopReason::Requested {
            result.stop_reason = StopReason::TimeLimit;
        }
        Ok(result)
    }
}

/// 予算を組めないほど残り時間が短いときに使う 2 手読み探索。
///
/// 数マイクロ秒程度で終わり、合法手の最小マスを機械的に選ぶことによる大悪手を防ぐ。
pub(crate) fn reflex(engine: &mut Engine, pos: Position) -> Result<SearchResult, EngineError> {
    engine.search(pos, &SearchRequest::new(Goal::Depth(2)).skip_pv_recovery())
}

/// 反復深化の各段階の所要時間と最善手の変化を追跡し、次段階に進むべきかを判定する状態。
#[derive(Default)]
struct Forecast {
    empties: u8,
    elapsed: Duration,
    last_cost: Duration,
    last_depth: Option<u8>,
    best: Option<Action>,
    instability: f64,
}

impl Forecast {
    /// 反復が 1 段階完了したときに、次の段階 `next` が予算を超える見込みなら `true` を返す。
    fn stop(
        &mut self,
        elapsed: Duration,
        kind: ResultKind,
        next: Option<ResultKind>,
        best: Option<Action>,
        budget: Budget,
    ) -> bool {
        let cost = elapsed.saturating_sub(self.elapsed);
        let growth = next.map(|next| self.growth(kind, next, cost));
        let changed = self.best.is_some() && self.best != best;
        // 2 手前に最善手が入れ替わった場合も、直後の反復で確認されるまでは不安定さとみなす。
        // 同じ手が確認されるたびに半減させ、再び入れ替わったら 1.0 に戻す。
        self.instability = if changed { 1.0 } else { self.instability * 0.5 };
        self.best = best;
        self.elapsed = elapsed;
        self.last_cost = cost;
        self.last_depth = match kind {
            ResultKind::Depth { depth, .. } => Some(depth),
            ResultKind::Endgame { .. } => None,
        };
        // 最終段階が完了した場合は、もう予測すべき次段階がない。
        let Some(growth) = growth else {
            return false;
        };
        let target = budget.soft.as_secs_f64() * (1.0 + self.instability);
        let predicted = elapsed.as_secs_f64() + cost.as_secs_f64() * growth;
        predicted > target.min(budget.hard.as_secs_f64())
    }

    /// 直前に `cost` を要した段階 `kind` から次の段階 `next` へ進む際の推定コスト倍率を返す。
    fn growth(&self, kind: ResultKind, next: ResultKind, cost: Duration) -> f64 {
        let empties = f64::from(self.empties);
        match (kind, next) {
            (ResultKind::Depth { depth, .. }, ResultKind::Depth { depth: deeper, .. }) => {
                // 直近 2 回の反復から 2 手あたりの探索木成長率を計算する。
                // 置換表ヒットや浅い反復による極端な比率はクランプして抑える。
                let per_two_plies = match self.last_depth {
                    Some(previous) if !self.last_cost.is_zero() && depth > previous => {
                        let ratio = cost.as_secs_f64() / self.last_cost.as_secs_f64();
                        ratio
                            .powf(2.0 / f64::from(depth - previous))
                            .clamp(1.5, 6.0)
                    }
                    _ => 3.0,
                };
                per_two_plies.powf(f64::from(deeper.saturating_sub(depth)) / 2.0)
            }
            // 最初の終盤ステージは終局まで読み切るため、直前の中盤探索より大幅に重くなる。
            (
                ResultKind::Depth { .. },
                ResultKind::Endgame {
                    selectivity: Selectivity::Full,
                },
            ) => ENTRY.at(empties) * EXACT.at(empties),
            (ResultKind::Depth { .. }, ResultKind::Endgame { .. }) => ENTRY.at(empties),
            (
                ResultKind::Endgame { .. },
                ResultKind::Endgame {
                    selectivity: Selectivity::Full,
                },
            ) => EXACT.at(empties),
            // 同じ終盤深さをより高い MPC 信頼度で再探索する段階。
            (ResultKind::Endgame { .. }, ResultKind::Endgame { .. }) => STAGE,
            // 計画上、終盤から中盤へ戻ることはないが、万一戻った場合に備えて安全な倍率を返す。
            (ResultKind::Endgame { .. }, ResultKind::Depth { .. }) => ENTRY.at(empties),
        }
    }
}

/// 空きマス数に応じて幾何級数的に増加し、下限 `floor` を下回らないコスト倍率モデル。
struct Ratio {
    floor: f64,
    base: f64,
    pivot: f64,
    per_empty: f64,
}

impl Ratio {
    /// 空きマス数 `empties` における推定コスト倍率を返す。
    fn at(&self, empties: f64) -> f64 {
        (self.base * self.per_empty.powf(empties - self.pivot)).max(self.floor)
    }
}

// 冷えた置換表（probe、1/4 スレッド）および実戦（`trace` 機能付き GTP）で計測した中央値に基づく倍率。
// `soft` 目標は、上振れ時に `hard` 上限まで許容できるよう余裕を持たせてある。
// スレッド数を変えても中央値はほとんど変化しない。
/// 最終中盤深さ → 最初の終盤ステージのコスト倍率（20〜31 空きで約 5 倍、空きが少なく置換表が温まっていればより小さい）。
const ENTRY: Ratio = Ratio {
    floor: 2.0,
    base: 5.0,
    pivot: 24.0,
    per_empty: 1.06,
};
/// 最後の選択的終盤ステージ → 完全読みのコスト倍率（20〜23 空きで 2.5〜3.4 倍、24〜27 空きで 4〜5 倍、28 空き以上では 6 倍以上）。
const EXACT: Ratio = Ratio {
    floor: 1.0,
    base: 3.0,
    pivot: 22.0,
    per_empty: 1.15,
};
/// 選択的終盤ステージ → 次の選択的終盤ステージのコスト倍率（中央値 1.0〜1.25）。
const STAGE: f64 = 1.25;

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Color, EngineOptions, Model, PatternModel};

    /// Final disc difference for `me` under best play from both sides.
    fn exact(pos: Position, me: Color) -> i32 {
        if pos.is_terminal() {
            return pos.final_score(me);
        }
        if pos.must_pass() {
            return exact(pos.play(Action::Pass).unwrap(), me);
        }
        let values = pos
            .legal_moves()
            .map(|sq| exact(pos.play(Action::Place(sq)).unwrap(), me));
        if pos.side_to_move() == me {
            values.max().unwrap()
        } else {
            values.min().unwrap()
        }
    }

    #[test]
    fn the_reflex_move_is_not_the_lowest_square_when_that_loses() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let mut checked = 0;
        for game in 0..400_usize {
            let mut pos = Position::initial();
            let mut ply = 0;
            while pos.empty_count() > 2 && !pos.is_terminal() {
                if pos.must_pass() {
                    pos = pos.play(Action::Pass).unwrap();
                    continue;
                }
                let moves: Vec<_> = pos.legal_moves().collect();
                ply += 1;
                pos = pos
                    .play(Action::Place(moves[(game * 7 + ply * ply) % moves.len()]))
                    .unwrap();
            }
            if pos.is_terminal() || pos.must_pass() || pos.legal_moves().count() < 2 {
                continue;
            }
            let me = pos.side_to_move();
            let value = |sq| exact(pos.play(Action::Place(sq)).unwrap(), me);
            let best = pos.legal_moves().map(value).max().unwrap();
            let lowest = pos.legal_moves().next().unwrap();
            if value(lowest) == best {
                continue;
            }
            let Some(Action::Place(chosen)) = reflex(&mut engine, pos).unwrap().best_move else {
                panic!("no move");
            };
            // Two plies from two empties reach the end, so even the zero
            // model finds the best move.
            assert_eq!(value(chosen), best, "{pos:?}");
            checked += 1;
        }
        assert!(checked > 5, "only {checked} positions");
    }

    #[test]
    fn a_long_think_preserves_round_trips_for_cheap_future_solves() {
        let left = Duration::from_millis(80);
        let budget = Budget::new(12, left, left);
        assert!(left - budget.hard >= Duration::from_millis(10));
    }

    #[test]
    fn move_change_can_extend_soft_target_only_within_the_cap() {
        let ms = Duration::from_millis;
        let decision = |mv: &str, hard| {
            let mut forecast = Forecast {
                elapsed: ms(10),
                last_cost: ms(5),
                last_depth: Some(8),
                best: Some("d3".parse().unwrap()),
                ..Forecast::default()
            };
            // Last step took 10 ms and doubled in cost. The projected
            // completion at 40 ms lies beyond the normal 32 ms share.
            forecast.stop(
                ms(20),
                ResultKind::Depth {
                    depth: 10,
                    selectivity: Selectivity::Percent83,
                },
                Some(ResultKind::Depth {
                    depth: 12,
                    selectivity: Selectivity::Percent83,
                }),
                Some(mv.parse().unwrap()),
                Budget {
                    soft: ms(32),
                    hard: ms(hard),
                },
            )
        };
        assert!(decision("d3", 60));
        assert!(!decision("c4", 60));
        assert!(decision("c4", 35));
    }

    #[test]
    fn a_recent_reversal_keeps_some_time_for_confirmation() {
        let ms = Duration::from_millis;
        let budget = Budget {
            soft: ms(32),
            hard: ms(64),
        };
        let depth = |depth| ResultKind::Depth {
            depth,
            selectivity: Selectivity::Percent83,
        };
        let mut forecast = Forecast::default();
        let first = Some("d3".parse().unwrap());
        let second = Some("c4".parse().unwrap());
        let next = |d: u8| Some(depth(d + 2));
        assert!(!forecast.stop(ms(4), depth(6), next(6), first, budget));
        assert!(!forecast.stop(ms(10), depth(8), next(8), second, budget));
        // 20 + 10 * (10 / 6) exceeds the ordinary share, but fits
        // after a single confirmation of the new best move.
        assert!(!forecast.stop(ms(20), depth(10), next(10), second, budget));
        assert!(forecast.stop(ms(35), depth(12), next(12), second, budget));
    }

    const fn stage(selectivity: Selectivity) -> ResultKind {
        ResultKind::Endgame { selectivity }
    }

    /// Whether a forecast that saw the midgame grow 1.5x per two plies
    /// (4 ms, then 6 ms ending at 12 ms) stops before `next`.
    fn after_midgame(empties: u8, next: Option<ResultKind>) -> bool {
        let ms = Duration::from_millis;
        let mut forecast = Forecast {
            empties,
            elapsed: ms(6),
            last_cost: ms(4),
            last_depth: Some(14),
            ..Forecast::default()
        };
        let depth = ResultKind::Depth {
            depth: 16,
            selectivity: Selectivity::Percent83,
        };
        let budget = Budget {
            soft: ms(32),
            hard: ms(64),
        };
        forecast.stop(ms(12), depth, next, None, budget)
    }

    #[test]
    fn the_first_endgame_stage_is_not_priced_like_a_midgame_step() {
        let deeper = ResultKind::Depth {
            depth: 18,
            selectivity: Selectivity::Percent83,
        };
        // 12 + 6 * 1.5 fits in the 32 ms share, 12 + 6 * 4.4 does not.
        assert!(!after_midgame(22, Some(deeper)));
        assert!(after_midgame(22, Some(stage(Selectivity::Percent85))));
        assert!(after_midgame(22, Some(stage(Selectivity::Full))));
    }

    #[test]
    fn the_last_iteration_has_nothing_to_forecast() {
        let ms = Duration::from_millis;
        let mut forecast = Forecast::default();
        let budget = Budget {
            soft: ms(1),
            hard: ms(2),
        };
        assert!(!forecast.stop(ms(50), stage(Selectivity::Full), None, None, budget));
        assert!(!forecast.stop(ms(90), stage(Selectivity::Percent99), None, None, budget));
    }

    #[test]
    fn an_exact_solve_costs_more_than_another_selective_stage() {
        let ms = Duration::from_millis;
        let budget = Budget {
            soft: ms(32),
            hard: ms(64),
        };
        let decide = |next| {
            let mut forecast = Forecast {
                empties: 22,
                elapsed: ms(12),
                ..Forecast::default()
            };
            // The 99% stage took 8 ms, ending at 20 ms.
            forecast.stop(
                ms(20),
                stage(Selectivity::Percent99),
                Some(next),
                None,
                budget,
            )
        };
        assert!(!decide(stage(Selectivity::Percent99)));
        assert!(decide(stage(Selectivity::Full)));
    }

    #[test]
    fn transition_ratios_grow_with_the_empties_above_their_floor() {
        for ratio in [&ENTRY, &EXACT] {
            assert!(ratio.at(10.0) >= ratio.floor);
            assert!(ratio.at(30.0) > ratio.at(24.0));
        }
        assert!(EXACT.at(22.0) > STAGE);
    }

    #[test]
    fn future_exact_solves_release_time_as_the_clock_grows() {
        let share = |sec| {
            let left = Duration::from_secs(sec);
            Budget::new(40, left, left).soft.as_secs_f64() / sec as f64
        };
        assert!(share(1) > 1.0 / 20.0);
        assert!(share(60) > share(1));
    }

    #[test]
    fn protocol_cap_and_future_turns_bound_borrowing() {
        let left = Duration::from_secs(1);
        let cap = Duration::from_millis(5);
        for empty in 1..=60 {
            let b = Budget::new(empty, left, cap);
            assert!(b.soft <= b.hard && b.hard <= cap);
            assert_eq!(Budget::new(empty, Duration::ZERO, cap).hard, Duration::ZERO);
        }
        assert!(Budget::new(22, left, left).hard < left);
        assert_eq!(Budget::new(1, left, left).soft, left);
    }
}
