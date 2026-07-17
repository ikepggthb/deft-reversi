//! 終盤の Null-Window Search (NWS) 完全読み。
//!
//! ## 関数階層
//!
//! ```text
//! nws_final               (TT + ETC + MPC + 浅い eval ordering)
//!   └─ nws_final_simple   (MPC + FFS ordering のみ)
//!        └─ negaalpha_final  (parity ordering のみ)
//!             └─ solve_score_2_empties
//! ```
//!
//! ## 定数
//!
//! - `SWITCH_EMPTIES_NEGA_ALPHA` 空きマス以下では `negaalpha_final` に切り替える
//! - `SWITCH_EMPTIES_SIMPLE_NWS` 空きマス以下では `nws_final_simple` に切り替える
//! - `FINAL_LV` 置換表に登録する探索レベル(60 = 完全読み)

use crate::{
    board::{board::Board, constant::NO_COORD},
    eval::evaluator_const::SCORE_MAX,
    search::{
        final_search::{negaalpha::negaalpha_final, solve_score::solve_score},
        move_list::*,
        mpc::{final_search_mpc, ProbCutResult},
        search::SearchContext,
        stability_cut::stability_cut_nws,
        thread_pool::{Job, TaskHandle, TaskResult},
        tt_cut::*,
    },
    t_table::{TTProbe, TTSlot, TTValue},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const TT_MOVE0_SCORE: i32 = 1 << 8;
const TT_MOVE1_SCORE: i32 = 1 << 7;

const FINAL_LV: i32 = 60;

/// 空きマスがこれ以下のとき `negaalpha_final` に切り替える。
const SWITCH_EMPTIES_NEGA_ALPHA: i32 = 5;

/// 空きマスがこれ以下のとき `nws_final_simple` に切り替える。
const SWITCH_EMPTIES_SIMPLE_NWS: i32 = 10;

const YBWC_END_SPLIT_MIN_EMPTIES: i32 = 16;

// ── nws_final_simple ──────────────────────────────────────────────────────────

/// TT なしの簡易 NWS 完全読み。FFS 手順 + MPC のみ。
///
/// 空きマスが少ないとき `negaalpha_final` へ降格する。
pub fn nws_final_simple(board: &Board, alpha: i32, search: &mut SearchContext) -> i32 {
    let beta = alpha + 1;
    let n_empties = (board.player | board.opponent).count_zeros() as i32;

    if n_empties <= SWITCH_EMPTIES_NEGA_ALPHA {
        return negaalpha_final(board, alpha, beta, search);
    }

    search.stats.final_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    let moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -nws_final_simple(&passed, -beta, search);
    }

    if let Some(score) = stability_cut_nws(board, alpha, n_empties, search) {
        return score;
    }

    match final_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    let mut move_list = make_move_list(board, moves_bit);

    if move_list.len() >= 2 {
        assign_ffs_scores(board, &mut move_list);
        sort_move_list(&mut move_list);
    }

    let mut best_score = -SCORE_MAX;
    for mb in move_list.iter() {
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        let score = -nws_final_simple(&child_board, -beta, search);
        if search.is_aborted() {
            return alpha;
        }
        if score >= beta {
            return score;
        }
        if score > best_score {
            best_score = score;
        }
    }

    best_score
}

// ── nws_final ─────────────────────────────────────────────────────────────────

/// TT + ETC + MPC + 浅い eval ordering を使った NWS 完全読み。
///
/// 空きマスが少ないとき `nws_final_simple` へ降格する。
pub fn nws_final(board: &Board, alpha: i32, search: &mut SearchContext) -> i32 {
    let beta = alpha + 1;

    let n_empties = (board.player | board.opponent).count_zeros() as i32;
    if n_empties <= SWITCH_EMPTIES_SIMPLE_NWS {
        return nws_final_simple(board, alpha, search);
    }

    search.stats.final_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    let moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -nws_final(&passed, -beta, search);
    }

    // ── 通常手リストの生成 ───────────────────────────────────────────────────
    let mut move_list = make_move_list(board, moves_bit);

    // 全消しがある場合は、即時return
    for move_board in move_list.iter() {
        if board.opponent ^ move_board.flip_bit == 0 {
            return SCORE_MAX;
        }
    }

    if let Some(score) = stability_cut_nws(board, alpha, n_empties, search) {
        return score;
    }

    // ── 置換表プローブ ───────────────────────────────────────────────────────
    let probe: TTProbe = search.tt.probe(board);
    let tt_value: Option<TTValue> = probe.value();
    let mut alpha_cur = alpha;
    let mut beta_cur = beta;

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
    if n_empties > 12 {
        match e_tt_cut(
            board,
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
    }

    // ── TT 手の ordering score 反映 ──────────────────────────────────────────
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
        let eval_depth = match n_empties {
            11..13 => 0,
            13..17 => 1,
            17..21 => 2,
            _ => ((n_empties / 3) - 1).max(0),
        };
        assign_ordering_scores(board, &mut move_list, eval_depth, alpha_cur, search);
        sort_move_list(&mut move_list);
    }

    if should_split_ybwc(n_empties, &move_list, search) {
        return nws_final_ybwc(
            board,
            alpha,
            beta_cur,
            alpha_cur,
            probe.slot(),
            &move_list,
            search,
        );
    }

    // ── 探索ループ ────────────────────────────────────────────────────────────
    let mut best_score = -SCORE_MAX;
    let mut best_move = NO_COORD;

    for mb in move_list.iter() {
        if mb.is_skip {
            continue;
        }
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        let score = -nws_final(&child_board, -beta_cur, search);
        if search.is_aborted() {
            return alpha;
        }
        if score >= beta_cur {
            search.tt.store(
                probe.slot(),
                board,
                score,
                SCORE_MAX,
                FINAL_LV,
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

    debug_assert_ne!(best_move, NO_COORD);

    if best_score > alpha {
        search.tt.store(
            probe.slot(),
            board,
            best_score,
            best_score,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    } else {
        search.tt.store(
            probe.slot(),
            board,
            -SCORE_MAX,
            best_score,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}

fn should_split_ybwc(n_empties: i32, move_list: &[MoveBoard], search: &SearchContext) -> bool {
    n_empties >= YBWC_END_SPLIT_MIN_EMPTIES
        && search.thread_pool.is_some()
        && move_list.iter().filter(|mb| !mb.is_skip).take(2).count() >= 2
}

fn nws_final_ybwc(
    board: &Board,
    alpha: i32,
    beta: i32,
    mut alpha_cur: i32,
    tt_slot: TTSlot,
    move_list: &[MoveBoard],
    search: &mut SearchContext,
) -> i32 {
    let Some(thread_pool) = search.thread_pool.clone() else {
        return alpha;
    };
    let split_searching = Arc::new(AtomicBool::new(true));
    let mut handles: Vec<TaskHandle> = Vec::new();
    let mut best_score = -SCORE_MAX;
    let mut best_move = NO_COORD;
    let mut searched = 0usize;

    for (move_index, mb) in move_list.iter().enumerate() {
        if mb.is_skip {
            continue;
        }
        // 分配済みの子が fail-high を確定させていたら、残りの手は探索不要
        if searched > 0 && !split_searching.load(Ordering::Relaxed) {
            break;
        }
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        if searched == 0 {
            let score = -nws_final(&child_board, -beta, search);
            searched += 1;
            if search.is_aborted() {
                split_searching.store(false, Ordering::Relaxed);
                join_ybwc_tasks(handles, search, true);
                return alpha;
            }
            if score >= beta {
                split_searching.store(false, Ordering::Relaxed);
                join_ybwc_tasks(handles, search, true);
                search.tt.store(
                    tt_slot,
                    board,
                    score,
                    SCORE_MAX,
                    FINAL_LV,
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
            continue;
        }

        let job = make_ybwc_job(
            child_board,
            -beta,
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
                    if !split_searching.load(Ordering::Relaxed) {
                        // 先に走っている兄弟がfail-highを確定した。勝者の結果を
                        // handlesから回収するため、残りの分配だけ打ち切る。
                        break;
                    }
                    split_searching.store(false, Ordering::Relaxed);
                    join_ybwc_tasks(handles, search, true);
                    search.check_abort_now();
                    return alpha;
                }
                if result.score >= beta {
                    split_searching.store(false, Ordering::Relaxed);
                    join_ybwc_tasks(handles, search, true);
                    let move_num = move_list[result.move_index].move_num;
                    search.tt.store(
                        tt_slot,
                        board,
                        result.score,
                        SCORE_MAX,
                        FINAL_LV,
                        search.selectivity_lv,
                        move_num,
                    );
                    return result.score;
                }
                if result.score > alpha_cur {
                    alpha_cur = result.score;
                }
                if result.score > best_score {
                    best_score = result.score;
                    best_move = move_list[result.move_index].move_num;
                }
            }
        }
    }

    let results = collect_ybwc_tasks(handles, search);
    if search.check_abort_now() {
        split_searching.store(false, Ordering::Relaxed);
        return alpha;
    }
    for result in results {
        if result.aborted {
            continue;
        }
        if result.score >= beta {
            split_searching.store(false, Ordering::Relaxed);
            let move_num = move_list[result.move_index].move_num;
            search.tt.store(
                tt_slot,
                board,
                result.score,
                SCORE_MAX,
                FINAL_LV,
                search.selectivity_lv,
                move_num,
            );
            return result.score;
        }
        if result.score > alpha_cur {
            alpha_cur = result.score;
        }
        if result.score > best_score {
            best_score = result.score;
            best_move = move_list[result.move_index].move_num;
        }
    }

    debug_assert_ne!(best_move, NO_COORD);

    if best_score > alpha {
        search.tt.store(
            tt_slot,
            board,
            best_score,
            best_score,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    } else {
        search.tt.store(
            tt_slot,
            board,
            -SCORE_MAX,
            best_score,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}

fn make_ybwc_job(
    child_board: Board,
    child_alpha: i32,
    move_index: usize,
    split_searching: Arc<AtomicBool>,
    parent: &SearchContext,
) -> Job {
    let evaluator = parent.evaluator.clone();
    let ordering_evaluator = parent.ordering_evaluator.clone();
    let mpc_config = parent.mpc_config.clone();
    let tt = parent.tt.clone();
    let stop = parent.stop.clone();
    let thread_pool = parent.thread_pool.clone();
    let selectivity_lv = parent.selectivity_lv;
    let mut searchings = parent.searchings.clone();
    searchings.push(split_searching.clone());

    Box::new(move || {
        let mut stats = crate::search::search::SearchStats::default();
        let mut search = SearchContext::new(evaluator, mpc_config, tt, &mut stats)
            .with_ordering_evaluator(ordering_evaluator)
            .with_stop(stop)
            .with_thread_pool(thread_pool)
            .with_searchings(searchings);
        search.selectivity_lv = selectivity_lv;
        let score = -nws_final(&child_board, child_alpha, &mut search);
        let aborted = search.is_aborted();
        drop(search);
        // fail-high (score >= beta = -child_alpha) が確定したら兄弟タスクを打ち切る
        if !aborted && score >= -child_alpha {
            split_searching.store(false, Ordering::Relaxed);
        }
        TaskResult {
            score,
            move_index,
            stats,
            aborted,
        }
    })
}

fn collect_ybwc_tasks(handles: Vec<TaskHandle>, search: &mut SearchContext) -> Vec<TaskResult> {
    let pool = search.thread_pool.clone();
    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        let result = match pool.as_deref() {
            Some(pool) => pool.join_helping(handle),
            None => handle.join(),
        };
        search.stats.add_assign(result.stats);
        if result.aborted {
            search.stats.ybwc_split_aborts += 1;
        }
        results.push(result);
    }
    results
}

fn join_ybwc_tasks(handles: Vec<TaskHandle>, search: &mut SearchContext, discard_scores: bool) {
    for result in collect_ybwc_tasks(handles, search) {
        if discard_scores && result.aborted {
            continue;
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::final_search::solve_score::solve_score;
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

    fn make_board_with_empties(rng: &mut u64, n_empties: u32) -> Board {
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

    /// NWS の正しさを検証するヘルパー。
    ///
    /// `brute_force` で真スコアを求めてから、`alpha = true_score - 1` と `alpha = true_score`
    /// で NWS を呼ぶ。
    /// - `alpha = T-1` → beta=T → true_score ≥ T=beta → fail-high → result ≥ beta=T
    /// - `alpha = T`   → beta=T+1 → true_score=T ≤ alpha=T → fail-low → result ≤ T
    fn check_nws<F>(
        board: &Board,
        ev: &Arc<Evaluator>,
        mpc: &Arc<MpcConfig>,
        tt: &Arc<TranspositionTable>,
        mut nws: F,
    ) where
        F: FnMut(&Board, i32, &mut SearchContext) -> i32,
    {
        let true_score = brute_force(board);

        // fail-high check
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
        let result = nws(board, true_score - 1, &mut search);
        assert!(
            result >= true_score,
            "fail-high broken: alpha={} result={result} true={true_score} (player={:#018x})",
            true_score - 1,
            board.player,
        );

        // fail-low check
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
        let result = nws(board, true_score, &mut search);
        assert!(
            result <= true_score,
            "fail-low broken: alpha={true_score} result={result} true={true_score} (player={:#018x})",
            board.player,
        );
    }

    /// nws_final_simple: ランダム 6〜9 マス空き盤面で NWS 単調性を確認する。
    #[test]
    fn nws_final_simple_nws_property() {
        let (ev, mpc, tt) = shared_resources();
        let mut rng = 0xabcd_ef01_2345_6789_u64;

        for _ in 0..200 {
            let extra = (next_pseudo_random(&mut rng) % 4) as u32;
            let board = make_board_with_empties(&mut rng, 6 + extra);
            check_nws(&board, &ev, &mpc, &tt, |b, alpha, s| {
                nws_final_simple(b, alpha, s)
            });
        }
    }

    /// nws_final: ランダム 6〜9 マス空き盤面で NWS 単調性を確認する(同じ盤面に TT を活用)。
    #[test]
    fn nws_final_nws_property() {
        let (ev, mpc, tt) = shared_resources();
        let mut rng = 0x9876_5432_10fe_dcba_u64;

        for _ in 0..200 {
            let extra = (next_pseudo_random(&mut rng) % 4) as u32;
            let board = make_board_with_empties(&mut rng, 6 + extra);
            check_nws(&board, &ev, &mpc, &tt, |b, alpha, s| nws_final(b, alpha, s));
        }
    }
}
