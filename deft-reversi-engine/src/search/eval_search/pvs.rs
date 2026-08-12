//! 中盤の Principal Variation Search (PVS) 評価探索。
//!
//! ## 関数階層
//!
//! ```text
//! pvs_eval                (TT + ETC + MPC + ordering + PVS ループ)
//!   ├─ pvs_final          (空きマス少のとき委譲)
//!   ├─ negaalpha_eval_leaf (depth が浅いとき委譲)
//!   ├─ pvs_eval           (再帰: 第一手 / 再探索)
//!   └─ nws_eval           (非 PV 手の NWS サブサーチ)
//! ```

use crate::board::board::Board;
use crate::board::constant::NO_COORD;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::eval_search::leaf::negaalpha_eval_leaf;
use crate::search::eval_search::nws::nws_eval;
use crate::search::final_search::solve_score::solve_score;
use crate::search::move_list::*;
use crate::search::mpc::{eval_search_mpc, ProbCutResult};
use crate::search::search::SearchContext;
use crate::search::tt_cut::*;

const TT_MOVE0_SCORE: i32 = 1 << 8;
const TT_MOVE1_SCORE: i32 = 1 << 7;

/// depth がこれ以下のとき `negaalpha_eval_leaf` に委譲する。
const SWITCH_DEPTH_LEAF: i32 = 2;

/// depth がこれ以上のとき ETC を実行する。
const SWITCH_DEPTH_ETC: i32 = 4;

/// TT + ETC + MPC + PVS ループによる中盤評価探索を行い、現プレイヤー視点のスコアを返す。
pub fn pvs_eval(
    board: &Board,
    alpha: i32,
    beta: i32,
    depth: i32,
    search: &mut SearchContext,
) -> i32 {
    debug_assert!(alpha < beta);
    debug_assert!(-SCORE_MAX <= alpha && beta <= SCORE_MAX);

    // 浅い depth は leaf 探索へ
    if depth <= SWITCH_DEPTH_LEAF {
        return negaalpha_eval_leaf(board, alpha, beta, depth, search);
    }

    search.stats.eval_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    let moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.eval_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -pvs_eval(&passed, -beta, -alpha, depth, search);
    }

    // ── 置換表プローブ ────────────────────────────────────────────────────────
    let probe = search.tt.probe(board);
    let tt_value = probe.value();
    let mut alpha_cur = alpha;
    let mut beta_cur = beta;

    if let Some(v) = tt_value {
        if let Some(score) = tt_cut(
            v,
            depth,
            search.selectivity_lv,
            &mut alpha_cur,
            &mut beta_cur,
        ) {
            return score;
        }
    }

    // ── MPC ──────────────────────────────────────────────────────────────────
    match eval_search_mpc(board, alpha_cur, beta_cur, depth, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    // ── 通常手リスト ──────────────────────────────────────────────────────────
    let mut move_list = make_move_list(board, moves_bit);

    // ── ETC ───────────────────────────────────────────────────────────────────
    if depth >= SWITCH_DEPTH_ETC {
        match e_tt_cut(
            board,
            alpha,
            beta_cur,
            &mut move_list,
            depth - 1,
            search.selectivity_lv,
            search,
        ) {
            ETCResult::BetaCut(beta) => return beta,
            ETCResult::NarrowAlpha(na) => alpha_cur = na,
            ETCResult::AllMovesSkipped(upper) => return upper,
        };
    }

    if let Some(value) = tt_value {
        for ml in move_list.iter_mut() {
            if ml.move_num == value.move0 {
                ml.score = TT_MOVE0_SCORE;
            } else if ml.move_num == value.move1 {
                ml.score = TT_MOVE1_SCORE;
            }
        }
    }

    // ── move ordering ─────────────────────────────────────────────────────────
    if move_list.iter().filter(|mb| !mb.is_skip).take(2).count() >= 2 {
        let eval_depth = match depth {
            ..=4 => 0,
            5..=7 => 1,
            8..=11 => 2,
            _ => 3,
        };
        assign_ordering_scores(board, &mut move_list, eval_depth, alpha_cur, search);
        sort_move_list(&mut move_list);
    }

    // ── PVS ループ ────────────────────────────────────────────────────────────
    let mut best_score = -SCORE_MAX;
    let mut best_move = NO_COORD;
    let mut is_first = true;

    for mb in move_list.iter() {
        if mb.is_skip {
            continue;
        }
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        let score = if is_first {
            is_first = false;
            -pvs_eval(&child_board, -beta_cur, -alpha_cur, depth - 1, search)
        } else {
            let s = -nws_eval(&child_board, -(alpha_cur + 1), depth - 1, search);
            if s > alpha_cur && s < beta_cur {
                -pvs_eval(&child_board, -beta_cur, -alpha_cur, depth - 1, search)
            } else {
                s
            }
        };
        if search.is_aborted() {
            return alpha;
        }

        if score >= beta_cur {
            search.tt.store(
                probe.slot(),
                board,
                score,
                SCORE_MAX,
                depth,
                search.selectivity_lv,
                mb.move_num,
            );
            return score;
        }
        if score > alpha_cur {
            alpha_cur = score;
        }
        if score > best_score {
            best_score = score;
            best_move = mb.move_num;
        }
    }

    if best_move == NO_COORD {
        return -SCORE_MAX;
    }

    if best_score > alpha {
        search.tt.store(
            probe.slot(),
            board,
            best_score,
            best_score,
            depth,
            search.selectivity_lv,
            best_move,
        );
    } else {
        search.tt.store(
            probe.slot(),
            board,
            -SCORE_MAX,
            best_score,
            depth,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{default_evaluator, Evaluator};
    use crate::search::mpc::MpcConfig;
    use crate::search::search::SearchStats;
    use crate::t_table::TranspositionTable;
    use std::sync::Arc;

    fn shared_resources() -> (Arc<dyn Evaluator>, Arc<MpcConfig>, Arc<TranspositionTable>) {
        (
            default_evaluator(),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
        )
    }

    fn next_pseudo_random(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
    }

    fn make_random_board(rng: &mut u64, n_empties: u32) -> Board {
        let mut empties = 0u64;
        while empties.count_ones() < n_empties {
            let pos = (next_pseudo_random(rng) % 64) as u32;
            empties |= 1u64 << pos;
        }
        let player = next_pseudo_random(rng) & !empties;
        Board {
            player,
            opponent: !player & !empties,
        }
    }

    /// `pvs_eval` が `negaalpha_eval_leaf`(全幅 reference)と一致することを確認する。
    ///
    /// MPC は両者で同じ設定なので、PVS ロジックが正しく実装されていれば
    /// 同じ exact 値を返す。
    #[test]
    fn pvs_eval_matches_negaalpha_leaf() {
        let (ev, mpc, _tt) = shared_resources();
        let mut rng = 0xa5a5_5a5a_3c3c_c3c3_u64;

        for _ in 0..20 {
            let extra = (next_pseudo_random(&mut rng) % 10) as u32;
            let board = make_random_board(&mut rng, 30 + extra);
            let depth = 4;

            let mut stats = SearchStats::default();
            let mut search = SearchContext::new(
                ev.clone(),
                mpc.clone(),
                Arc::new(TranspositionTable::new()),
                &mut stats,
            );
            let leaf_r = negaalpha_eval_leaf(&board, -SCORE_MAX, SCORE_MAX, depth, &mut search);

            let mut stats2 = SearchStats::default();
            let mut search2 = SearchContext::new(
                ev.clone(),
                mpc.clone(),
                Arc::new(TranspositionTable::new()),
                &mut stats2,
            );
            let pvs_r = pvs_eval(&board, -SCORE_MAX, SCORE_MAX, depth, &mut search2);

            assert_eq!(
                pvs_r, leaf_r,
                "pvs_eval vs negaalpha_eval_leaf mismatch (board player={:#018x})",
                board.player,
            );
        }
    }

    /// fail-soft の境界が正しいことを確認する。
    #[test]
    fn pvs_eval_alpha_beta_bounds() {
        let (ev, mpc, _tt) = shared_resources();
        let mut rng = 0xfeed_face_dead_beef_u64;

        for _ in 0..15 {
            let extra = (next_pseudo_random(&mut rng) % 10) as u32;
            let board = make_random_board(&mut rng, 30 + extra);
            let depth = 4;

            // 真値を leaf で取得
            let mut stats = SearchStats::default();
            let mut search = SearchContext::new(
                ev.clone(),
                mpc.clone(),
                Arc::new(TranspositionTable::new()),
                &mut stats,
            );
            let true_score = negaalpha_eval_leaf(&board, -SCORE_MAX, SCORE_MAX, depth, &mut search);

            for &(alpha, beta) in &[
                (true_score - 5, true_score + 5),
                (true_score, true_score + 1),
                (true_score - 1, true_score),
            ] {
                if alpha >= beta {
                    continue;
                }
                let mut stats2 = SearchStats::default();
                let mut search2 = SearchContext::new(
                    ev.clone(),
                    mpc.clone(),
                    Arc::new(TranspositionTable::new()),
                    &mut stats2,
                );
                let result = pvs_eval(&board, alpha, beta, depth, &mut search2);

                if true_score >= beta {
                    assert!(
                        result >= beta,
                        "fail-high broken: alpha={alpha} beta={beta} result={result} true={true_score}",
                    );
                } else if true_score <= alpha {
                    assert!(
                        result <= alpha,
                        "fail-low broken: alpha={alpha} beta={beta} result={result} true={true_score}",
                    );
                } else {
                    assert_eq!(
                        result, true_score,
                        "exact mismatch: alpha={alpha} beta={beta} true={true_score}",
                    );
                }
            }
        }
    }
}
