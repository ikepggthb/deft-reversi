//! 終盤の Principal Variation Search (PVS) 完全読み。
//!
//! ## アルゴリズム
//!
//! 最初の手は全幅 `[alpha, beta]` で探索し、以降の手は NWS `[alpha, alpha+1]` で試みる。
//! NWS で改善が見込まれる場合(score > alpha かつ score < beta)は全幅で再探索する。
//!
//! ## 関数階層
//!
//! ```text
//! pvs_final               (TT + ETC + MPC + 浅い eval ordering + PVS ループ)
//!   ├─ pvs_final          (再帰)
//!   └─ nws_final          (非 PV 手の NWS サブサーチ)
//! ```

use crate::{
    board::{board::Board, constant::NO_COORD},
    eval::evaluator_const::SCORE_MAX,
    search::tt_cut::*,
    search::{
        final_search::{
            negaalpha::negaalpha_final,
            nws::{collect_ybwc_tasks, make_ybwc_job, nws_final},
            solve_score::solve_score,
        },
        move_list::*,
        mpc::{final_search_mpc, ProbCutResult},
        search::{SearchContext, NO_MPC_SELECTIVITY_LV},
        stability_cut::stability_cut_pvs,
    },
    t_table::{TTProbe, TTSlot, TTValue},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const TT_MOVE0_SCORE: i32 = 1 << 20;
const TT_MOVE1_SCORE: i32 = 1 << 19;

const FINAL_LV: i32 = 60;
const PVS_YBWC_MIN_EMPTIES: i32 = 20;
const SELECTIVE_PVS_YBWC_MIN_EMPTIES: i32 = 16;

/// 空きマスがこれ以下のとき `negaalpha_final` に切り替える。
///
/// PVS は全幅探索なので、基底は NWS ではなく厳密探索に委ねる。
const SWITCH_EMPTIES_NEGAALPHA: i32 = 12;

#[inline(always)]
fn pvs_ybwc_min_empties(selectivity_lv: i32) -> i32 {
    if selectivity_lv < NO_MPC_SELECTIVITY_LV {
        SELECTIVE_PVS_YBWC_MIN_EMPTIES
    } else {
        PVS_YBWC_MIN_EMPTIES
    }
}

#[inline(always)]
fn choose_pvs_tt_value(
    pv_value: Option<TTValue>,
    main_value: Option<TTValue>,
    selectivity_lv: i32,
) -> Option<TTValue> {
    let matches_search = |value: TTValue| {
        value.lv as i32 == FINAL_LV && value.selectivity_lv as i32 == selectivity_lv
    };
    let pv_match = pv_value.filter(|value| matches_search(*value));
    let main_match = main_value.filter(|value| matches_search(*value));
    match (pv_match, main_match) {
        (Some(pv), Some(main)) => {
            let pv_width = pv.upper as i32 - pv.lower as i32;
            let main_width = main.upper as i32 - main.lower as i32;
            Some(if pv_width <= main_width { pv } else { main })
        }
        (Some(pv), None) => Some(pv),
        (None, Some(main)) => Some(main),
        (None, None) => pv_value.or(main_value),
    }
}

#[inline(always)]
fn store_pvs_result(
    search: &SearchContext,
    main_slot: TTSlot,
    pv_slot: Option<TTSlot>,
    board: &Board,
    lower: i32,
    upper: i32,
    best_move: u8,
) {
    search.tt.store(
        main_slot,
        board,
        lower,
        upper,
        FINAL_LV,
        search.selectivity_lv,
        best_move,
    );
    if let (Some(pv_tt), Some(slot)) = (search.pv_tt.as_ref(), pv_slot) {
        pv_tt.store(
            slot,
            board,
            lower,
            upper,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    }
}

fn pvs_final_ybwc(
    board: &Board,
    alpha: i32,
    mut alpha_cur: i32,
    beta_cur: i32,
    probe: TTProbe,
    pv_probe: Option<TTProbe>,
    move_list: &[MoveBoard],
    search: &mut SearchContext,
) -> i32 {
    let thread_pool = search.thread_pool.clone().expect("YBWC requires a pool");
    let Some((first_index, first_move)) = move_list.iter().enumerate().find(|(_, mv)| !mv.is_skip)
    else {
        return -SCORE_MAX;
    };

    let first_child = board.make_move_from_flip_bit(1 << first_move.move_num, first_move.flip_bit);
    let mut best_score = -pvs_final(&first_child, -beta_cur, -alpha_cur, search);
    if search.is_aborted() {
        return alpha;
    }
    let mut best_move = first_move.move_num;
    if best_score >= beta_cur {
        store_pvs_result(
            search,
            probe.slot(),
            pv_probe.map(|probe| probe.slot()),
            board,
            best_score,
            SCORE_MAX,
            best_move,
        );
        return best_score;
    }
    alpha_cur = alpha_cur.max(best_score);

    let split_searching = Arc::new(AtomicBool::new(true));
    let mut handles = Vec::new();
    let mut results = Vec::new();
    for (move_index, mv) in move_list.iter().enumerate().skip(first_index + 1) {
        if mv.is_skip || !split_searching.load(Ordering::Relaxed) {
            continue;
        }
        let child = board.make_move_from_flip_bit(1 << mv.move_num, mv.flip_bit);
        let job = make_ybwc_job(
            child,
            -(alpha_cur + 1),
            beta_cur,
            move_index,
            split_searching.clone(),
            search,
        );
        match thread_pool.try_push(job) {
            Ok(handle) => {
                search.stats.ybwc_splits += 1;
                handles.push(handle);
            }
            Err(job) => {
                let result = job();
                search.stats.add_assign(result.stats);
                if result.aborted {
                    search.stats.ybwc_split_aborts += 1;
                }
                let cutoff = !result.aborted && result.score >= beta_cur;
                results.push(result);
                if cutoff {
                    break;
                }
            }
        }
    }
    results.extend(collect_ybwc_tasks(handles, search));
    if search.check_abort_now() {
        split_searching.store(false, Ordering::Relaxed);
        return alpha;
    }

    if let Some(result) = results
        .iter()
        .find(|result| !result.aborted && result.score >= beta_cur)
    {
        let best_move = move_list[result.move_index].move_num;
        store_pvs_result(
            search,
            probe.slot(),
            pv_probe.map(|probe| probe.slot()),
            board,
            result.score,
            SCORE_MAX,
            best_move,
        );
        return result.score;
    }

    results.sort_unstable_by_key(|result| result.move_index);
    for result in results {
        if result.aborted {
            continue;
        }
        let mv = &move_list[result.move_index];
        let mut score = result.score;
        if score > alpha_cur {
            let child = board.make_move_from_flip_bit(1 << mv.move_num, mv.flip_bit);
            score = -pvs_final(&child, -beta_cur, -alpha_cur, search);
            if search.is_aborted() {
                return alpha;
            }
        }
        if score > best_score {
            best_score = score;
            best_move = mv.move_num;
        }
        if score >= beta_cur {
            store_pvs_result(
                search,
                probe.slot(),
                pv_probe.map(|probe| probe.slot()),
                board,
                score,
                SCORE_MAX,
                best_move,
            );
            return score;
        }
        alpha_cur = alpha_cur.max(score);
    }

    if best_score > alpha {
        store_pvs_result(
            search,
            probe.slot(),
            pv_probe.map(|probe| probe.slot()),
            board,
            best_score,
            best_score,
            best_move,
        );
    } else {
        store_pvs_result(
            search,
            probe.slot(),
            pv_probe.map(|probe| probe.slot()),
            board,
            -SCORE_MAX,
            best_score,
            best_move,
        );
    }
    best_score
}

/// TT + ETC + MPC + PVS ループによる完全読みを行い、現プレイヤー視点のスコアを返す。
pub fn pvs_final(board: &Board, alpha: i32, beta: i32, search: &mut SearchContext) -> i32 {
    debug_assert!(alpha < beta);
    debug_assert!(-SCORE_MAX <= alpha && beta <= SCORE_MAX);

    let empty_count = (board.player | board.opponent).count_zeros() as i32;

    if empty_count <= SWITCH_EMPTIES_NEGAALPHA {
        return negaalpha_final(board, alpha, beta, search);
    }

    search.stats.final_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    let moves_bit = board.moves();

    // 着手する場所がない
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            // 双方打てる場所がないので、終端ノード
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }

        // パス
        return -pvs_final(&passed, -beta, -alpha, search);
    }

    // ── 通常手リストの生成 ──────────────────────────────────────────────────────
    let mut move_list = make_move_list(board, moves_bit);

    // 全消しがある場合は、即時return
    for move_board in move_list.iter() {
        if board.opponent ^ move_board.flip_bit == 0 {
            return SCORE_MAX;
        }
    }

    let mut alpha_cur = alpha;
    let mut beta_cur = beta;
    if let Some(score) = stability_cut_pvs(board, alpha_cur, &mut beta_cur, empty_count, search) {
        return score;
    }

    // ── 置換表 取得 ────────────────────────────────────────────────────────
    let probe: TTProbe = search.tt.probe(board);
    let pv_probe = (empty_count >= search.pv_tt_min_empties)
        .then(|| search.pv_tt.as_ref().map(|pv_tt| pv_tt.probe(board)))
        .flatten();
    let tt_value = choose_pvs_tt_value(
        pv_probe.and_then(|probe| probe.value()),
        probe.value(),
        search.selectivity_lv,
    );

    // ── TT CUT OFF ─────────────────────────────────────────────────────────────
    if let Some(v) = tt_value {
        if let Some(score) = tt_cut(
            v,
            FINAL_LV,
            search.selectivity_lv,
            &mut alpha_cur,
            &mut beta_cur,
        ) {
            return score;
        }
    }

    // ── MPC ──────────────────────────────────────────────────────────────────
    match final_search_mpc(board, alpha_cur, beta_cur, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    // ── ETC ───────────────────────────────────────────────────────────────────
    match e_tt_cut(
        &board,
        alpha,
        beta_cur,
        &mut move_list,
        FINAL_LV,
        search.selectivity_lv,
        search,
    ) {
        ETCResult::BetaCut(beta) => return beta,
        ETCResult::NarrowAlpha(na) => alpha_cur = na,
        ETCResult::AllMovesSkipped(upper) => return upper,
    };

    // A selective endgame result is a strong score predictor even though its
    // bounds cannot be used for an exact TT cutoff. Probe a two-point window
    // first, then search only the side on which the prediction failed.
    if beta_cur - alpha_cur >= 4 {
        if let Some(value) = tt_value.filter(|value| {
            value.lower == value.upper && value.selectivity_lv < search.selectivity_lv as u8
        }) {
            let raw_prediction = value.lower as i32;
            let predicted = if raw_prediction & 1 != 0 {
                raw_prediction - raw_prediction.signum()
            } else {
                raw_prediction
            };
            if alpha_cur < predicted && predicted < beta_cur {
                let predicted_alpha = predicted - 1;
                let predicted_beta = predicted + 1;
                let score = pvs_final(board, predicted_alpha, predicted_beta, search);
                if search.is_aborted() {
                    return alpha;
                }
                if predicted_alpha < score && score < predicted_beta {
                    return score;
                }
                if score <= predicted_alpha {
                    if score <= alpha_cur {
                        return score;
                    }
                    return pvs_final(board, alpha_cur, score, search);
                }
                if score >= beta_cur {
                    return score;
                }
                return pvs_final(board, score, beta_cur, search);
            }
        }
    }

    // ── TT 手リスト ───────────────────────────────────────────────────────────
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
    let eval_depth = match empty_count {
        // 13..17 => 1,
        // 17..21 => 2,
        _ => (empty_count >> 3).max(1),
    };

    assign_ordering_scores_weighted_window(
        board,
        &mut move_list,
        eval_depth,
        (-beta_cur - 8).max(-SCORE_MAX),
        (-alpha_cur + 12).min(SCORE_MAX),
        269 + 94 * eval_depth,
        35,
        1 << 15,
        search,
    );
    sort_move_list(&mut move_list);

    let pvs_split_min_empties = pvs_ybwc_min_empties(search.selectivity_lv);
    if empty_count >= pvs_split_min_empties
        && search.thread_pool.is_some()
        && move_list.iter().filter(|mv| !mv.is_skip).take(2).count() >= 2
    {
        return pvs_final_ybwc(
            board, alpha, alpha_cur, beta_cur, probe, pv_probe, &move_list, search,
        );
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
            -pvs_final(&child_board, -beta_cur, -alpha_cur, search)
        } else {
            let s = -nws_final(&child_board, -(alpha_cur + 1), search);
            if s > alpha_cur && s < beta_cur {
                -pvs_final(&child_board, -beta_cur, -alpha_cur, search)
            } else {
                s
            }
        };
        if search.is_aborted() {
            return alpha;
        }

        if score >= beta_cur {
            store_pvs_result(
                search,
                probe.slot(),
                pv_probe.map(|probe| probe.slot()),
                board,
                score,
                SCORE_MAX,
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

    debug_assert_ne!(best_move, NO_COORD);

    if best_score > alpha {
        store_pvs_result(
            search,
            probe.slot(),
            pv_probe.map(|probe| probe.slot()),
            board,
            best_score,
            best_score,
            best_move,
        );
    } else {
        store_pvs_result(
            search,
            probe.slot(),
            pv_probe.map(|probe| probe.slot()),
            board,
            -SCORE_MAX,
            best_score,
            best_move,
        );
    }

    best_score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::final_search::solve_score::solve_score;
    use crate::search::mpc::MpcConfig;
    use crate::search::search::SearchStats;
    use crate::t_table::TranspositionTable;
    use std::sync::Arc;

    #[test]
    fn selective_pvs_splits_four_plies_earlier() {
        assert_eq!(pvs_ybwc_min_empties(NO_MPC_SELECTIVITY_LV - 1), 16);
        assert_eq!(pvs_ybwc_min_empties(NO_MPC_SELECTIVITY_LV), 20);
    }

    fn shared_resources() -> (Arc<Evaluator>, Arc<MpcConfig>, Arc<TranspositionTable>) {
        (
            Arc::new(Evaluator::default()),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
        )
    }

    #[test]
    fn pvs_tt_prefers_tighter_matching_bound() {
        let value = |lower, upper| TTValue {
            lower,
            upper,
            lv: FINAL_LV as u8,
            selectivity_lv: crate::search::mpc::SELECTIVITY_LV_MAX as u8,
            move0: 1,
            move1: NO_COORD,
            generation: 1,
            flags: 1,
        };
        let pv = value(-8, 64);
        let main = value(-10, -10);

        assert_eq!(
            choose_pvs_tt_value(Some(pv), Some(main), crate::search::mpc::SELECTIVITY_LV_MAX,),
            Some(main)
        );
    }

    fn brute_force(board: &Board) -> i32 {
        let moves = board.moves();
        if moves == 0 {
            if board.opponent_moves() == 0 {
                return solve_score(board);
            }
            return -brute_force(&board.passed());
        }
        let mut best = -SCORE_MAX;
        let mut bits = moves;
        while bits != 0 {
            let m = bits & bits.wrapping_neg();
            bits ^= m;
            best = best.max(-brute_force(&board.make_move(m)));
        }
        best
    }

    fn next_pseudo_random(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
    }

    fn make_board_with_empties(rng: &mut u64, empty_count: u32) -> Board {
        let mut empties = 0u64;
        while empties.count_ones() < empty_count {
            let pos = (next_pseudo_random(rng) % 64) as u32;
            empties |= 1u64 << pos;
        }
        let player = next_pseudo_random(rng) & !empties;
        Board {
            player,
            opponent: !player & !empties,
        }
    }

    /// pvs_final が brute_force と一致するか確認する(6〜9 マス空き)。
    #[test]
    fn pvs_final_matches_brute_force() {
        let (ev, mpc, tt) = shared_resources();
        let mut rng = 0x1357_9bdf_2468_ace0_u64;

        for _ in 0..200 {
            let extra = (next_pseudo_random(&mut rng) % 4) as u32;
            let board = make_board_with_empties(&mut rng, 6 + extra);
            let true_score = brute_force(&board);

            let mut stats = SearchStats::default();
            let mut search = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
            let result = pvs_final(&board, -SCORE_MAX, SCORE_MAX, &mut search);

            assert_eq!(
                result, true_score,
                "pvs_final mismatch (player={:#018x}, opponent={:#018x})",
                board.player, board.opponent,
            );
        }
    }

    /// fail-soft の境界が正しいことを確認する(6〜9 マス空き)。
    #[test]
    fn pvs_final_alpha_beta_bounds() {
        let (ev, mpc, tt) = shared_resources();
        let mut rng = 0xfeed_beef_dead_cafe_u64;

        for _ in 0..100 {
            let extra = (next_pseudo_random(&mut rng) % 4) as u32;
            let board = make_board_with_empties(&mut rng, 6 + extra);
            let true_score = brute_force(&board);

            for &(alpha, beta) in &[(-10, 10), (0, 2), (-5, 0), (3, 10), (-SCORE_MAX, SCORE_MAX)] {
                if alpha >= beta {
                    continue;
                }
                let mut stats = SearchStats::default();
                let mut search =
                    SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
                let result = pvs_final(&board, alpha, beta, &mut search);

                if true_score >= beta {
                    assert!(result >= beta,
                        "fail-high broken: alpha={alpha} beta={beta} result={result} true={true_score}");
                } else if true_score <= alpha {
                    assert!(result <= alpha,
                        "fail-low broken: alpha={alpha} beta={beta} result={result} true={true_score}");
                } else {
                    assert_eq!(
                        result, true_score,
                        "exact mismatch: alpha={alpha} beta={beta} true={true_score}"
                    );
                }
            }
        }
    }
}
