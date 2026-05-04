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
use crate::search::eval_search::cut_off::{e_tt_cut, tt_cut};
use crate::search::eval_search::leaf::negaalpha_eval_leaf;
use crate::search::eval_search::nws::nws_eval;
use crate::search::final_search::cut_off::assign_ordering_scores;
use crate::search::final_search::move_list::{
    build_tt_move_list, set_move_list, sort_move_list, uninit_move_array, MoveBoard,
};
use crate::search::final_search::pvs::pvs_final;
use crate::search::final_search::solve_score::solve_score;
use crate::search::mpc::{eval_search_mpc, ProbCutResult};
use crate::search::search::SearchContext;
use crate::t_table::TT_MOVES_CAPACITY;

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

    let n_empties = (board.player | board.opponent).count_zeros() as i32;

    // 終盤に十分近づいたら完全読みへ
    if n_empties <= search.final_search_empties {
        return pvs_final(board, alpha, beta, search);
    }

    // 浅い depth は leaf 探索へ
    if depth <= SWITCH_DEPTH_LEAF {
        return negaalpha_eval_leaf(board, alpha, beta, depth, search);
    }

    search.stats.eval_search_nodes += 1;

    let mut moves_bit = board.moves();
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

    let tt_moves = if let Some(v) = tt_value {
        let moves = v.moves();
        if moves[0] != NO_COORD {
            moves_bit &= !(1u64 << moves[0]);
        }
        if moves[1] != NO_COORD {
            moves_bit &= !(1u64 << moves[1]);
        }
        Some(moves)
    } else {
        None
    };

    if let Some(v) = tt_value {
        if let Some(score) = tt_cut(v, depth, search.selectivity_lv, &mut alpha_cur, &mut beta_cur) {
            return score;
        }
    }

    // ── MPC ──────────────────────────────────────────────────────────────────
    match eval_search_mpc(board, alpha_cur, beta_cur, depth, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    // ── TT 手リスト ───────────────────────────────────────────────────────────
    let mut tt_move_list: [MoveBoard; TT_MOVES_CAPACITY] = [MoveBoard::SENTINEL; TT_MOVES_CAPACITY];
    let tt_move_count = build_tt_move_list(board, &tt_moves, &mut tt_move_list);
    let tt_move_list = &mut tt_move_list[..tt_move_count];

    // ── 通常手リスト ──────────────────────────────────────────────────────────
    let move_count = moves_bit.count_ones() as usize;
    let mut move_list = uninit_move_array();
    let move_list = &mut move_list[..move_count];
    set_move_list(board, moves_bit, move_list);

    // ── ETC ───────────────────────────────────────────────────────────────────
    let mut n_skip = 0i32;
    if depth >= SWITCH_DEPTH_ETC {
        let sl = search.selectivity_lv;
        if let Some(score) = e_tt_cut(&mut alpha_cur, &mut beta_cur, tt_move_list, depth, sl, &mut 0, search) {
            return score;
        }
        if let Some(score) = e_tt_cut(&mut alpha_cur, &mut beta_cur, move_list, depth, sl, &mut n_skip, search) {
            return score;
        }
    }

    // ── move ordering ─────────────────────────────────────────────────────────
    if move_count - n_skip as usize >= 2 {
        let eval_depth = match depth {
            ..=4 => 0,
            5..=7 => 1,
            8..=11 => 2,
            _ => 3,
        };
        assign_ordering_scores(move_list, alpha_cur, eval_depth, 2, search);
        sort_move_list(move_list);
    }

    // ── PVS ループ ────────────────────────────────────────────────────────────
    let mut best_score = -SCORE_MAX;
    let mut best_move = NO_COORD;
    let mut is_first = true;

    for mb in tt_move_list.iter() {
        let score = if is_first {
            is_first = false;
            -pvs_eval(&mb.board, -beta_cur, -alpha_cur, depth - 1, search)
        } else {
            let s = -nws_eval(&mb.board, -(alpha_cur + 1), depth - 1, search);
            if s > alpha_cur && s < beta_cur {
                -pvs_eval(&mb.board, -beta_cur, -alpha_cur, depth - 1, search)
            } else {
                s
            }
        };

        if score >= beta_cur {
            search.tt.store(
                probe.slot(), board, score, SCORE_MAX, depth,
                search.selectivity_lv, mb.put_place,
            );
            return score;
        }
        if score > alpha_cur {
            alpha_cur = score;
        }
        if score > best_score {
            best_score = score;
            best_move = mb.put_place;
        }
    }

    for mb in move_list.iter() {
        if mb.skip {
            continue;
        }
        let score = if is_first {
            is_first = false;
            -pvs_eval(&mb.board, -beta_cur, -alpha_cur, depth - 1, search)
        } else {
            let s = -nws_eval(&mb.board, -(alpha_cur + 1), depth - 1, search);
            if s > alpha_cur && s < beta_cur {
                -pvs_eval(&mb.board, -beta_cur, -alpha_cur, depth - 1, search)
            } else {
                s
            }
        };

        if score >= beta_cur {
            search.tt.store(
                probe.slot(), board, score, SCORE_MAX, depth,
                search.selectivity_lv, mb.put_place,
            );
            return score;
        }
        if score > alpha_cur {
            alpha_cur = score;
        }
        if score > best_score {
            best_score = score;
            best_move = mb.put_place;
        }
    }

    if best_move == NO_COORD {
        return -SCORE_MAX;
    }

    if best_score > alpha {
        search.tt.store(
            probe.slot(), board, best_score, best_score, depth,
            search.selectivity_lv, best_move,
        );
    } else {
        search.tt.store(
            probe.slot(), board, -SCORE_MAX, best_score, depth,
            search.selectivity_lv, best_move,
        );
    }

    best_score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::mpc::MpcConfig;
    use crate::search::search::SearchStats;
    use crate::t_table::TranspositionTable;
    use std::sync::Arc;

    fn shared_resources() -> (Arc<Evaluator>, Arc<MpcConfig>, Arc<TranspositionTable>) {
        (
            Arc::new(Evaluator::default()),
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
        Board { player, opponent: !player & !empties }
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
                ev.clone(), mpc.clone(), Arc::new(TranspositionTable::new()), &mut stats,
            );
            let leaf_r = negaalpha_eval_leaf(&board, -SCORE_MAX, SCORE_MAX, depth, &mut search);

            let mut stats2 = SearchStats::default();
            let mut search2 = SearchContext::new(
                ev.clone(), mpc.clone(), Arc::new(TranspositionTable::new()), &mut stats2,
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
                ev.clone(), mpc.clone(), Arc::new(TranspositionTable::new()), &mut stats,
            );
            let true_score = negaalpha_eval_leaf(&board, -SCORE_MAX, SCORE_MAX, depth, &mut search);

            for &(alpha, beta) in &[(true_score - 5, true_score + 5), (true_score, true_score + 1), (true_score - 1, true_score)] {
                if alpha >= beta {
                    continue;
                }
                let mut stats2 = SearchStats::default();
                let mut search2 = SearchContext::new(
                    ev.clone(), mpc.clone(), Arc::new(TranspositionTable::new()), &mut stats2,
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
