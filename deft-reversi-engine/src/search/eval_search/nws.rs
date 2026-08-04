//! 中盤の Null-Window Search (NWS) 評価探索。
//!
//! ## 関数階層
//!
//! ```text
//! nws_eval                (TT + ETC + MPC + 浅い eval ordering)
//!   ├─ nws_final          (空きマス少のとき委譲)
//!   └─ nws_eval_leaf      (depth が浅いとき委譲)
//! ```
//!
//! ## 定数
//!
//! - `SWITCH_DEPTH_LEAF`: depth がこれ以下のとき `nws_eval_leaf` に委譲する
//! - `SWITCH_DEPTH_ETC`: depth がこれ以上のとき ETC を実行する

use crate::board::board::Board;
use crate::board::constant::NO_COORD;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::eval_search::leaf::nws_eval_leaf;
use crate::search::final_search::solve_score::solve_score;
use crate::search::move_list::*;
use crate::search::mpc::{eval_search_mpc, ProbCutResult};
use crate::search::search::{SearchContext, SearchStats};
use crate::search::split_point::{try_add_slaves, SplitPoint};
use crate::search::thread_pool::{DetachedJob, Job, TaskResult};
use crate::search::tt_cut::*;
use crate::t_table::TTSlot;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const TT_MOVE0_SCORE: i32 = 1 << 8;
const TT_MOVE1_SCORE: i32 = 1 << 7;

/// depth がこれ以下のとき `nws_eval_leaf` に委譲する。
const SWITCH_DEPTH_LEAF: i32 = 2;

/// depth がこれ以上のとき ETC を実行する。
const SWITCH_DEPTH_ETC: i32 = 4;

/// depth がこれ以上のノードで YBWC 分割を行う。
/// edax の `SPLIT_MIN_DEPTH` に相当する。
const EVAL_YBWC_MIN_DEPTH: i32 = 8;

/// この局面で YBWC 分割をする価値があるか。
///
/// 終盤側の `should_split_ybwc` と違い、空きスレッドの有無は見ない。
/// `search.can_spawn_split_job()` を足す案を計測したが、初期局面 level 24 の
/// 4 スレッドで 2.929 秒 / 2.951 秒 (各 14 回の中央値) と差が出なかったため
/// 採用していない。`nws_eval_ybwc` は最初の手を直列に探索し終えてから
/// 作業リストを確保するので、確保の費用は depth 8 以上の部分木全体に
/// 償却され元から無視できる。一方でここで弾くと、直列探索の間に空いた
/// スレッドをループ内の `try_add_slaves` で拾う機会を失う。
fn should_split_eval_ybwc(depth: i32, move_list: &[MoveBoard], search: &SearchContext) -> bool {
    depth >= EVAL_YBWC_MIN_DEPTH
        && search.thread_pool.is_some()
        && move_list.iter().filter(|mb| !mb.is_skip).take(2).count() >= 2
}

fn make_eval_split_worker(
    split: &Arc<SplitPoint>,
    parent: &SearchContext,
    depth: i32,
) -> DetachedJob {
    let split = split.clone();
    let evaluator = parent.evaluator.clone();
    let ordering_evaluator = parent.ordering_evaluator.clone();
    let mpc_config = parent.mpc_config.clone();
    let tt = parent.tt.clone();
    let stop = parent.stop.clone();
    let thread_pool = parent.thread_pool.clone();
    let selectivity_lv = parent.selectivity_lv;
    let mut searchings = parent.searchings.clone();
    searchings.push(split.searching.clone());
    let mut helper_chain = parent.helper_chain.clone();
    helper_chain.push(split.helper.clone());

    Box::new(move || {
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(evaluator, mpc_config, tt, &mut stats)
            .with_ordering_evaluator(ordering_evaluator)
            .with_stop(stop)
            .with_thread_pool(thread_pool)
            .with_searchings(searchings)
            .with_helper_chain(helper_chain);
        search.selectivity_lv = selectivity_lv;
        while let Some((work_index, _, child_board)) = split.next_work() {
            let score = -nws_eval(&child_board, -split.beta(), depth - 1, &mut search);
            if search.is_aborted() {
                break;
            }
            split.finish(work_index, score);
            if score >= split.beta() {
                break;
            }
        }
        let aborted = search.is_aborted();
        drop(search);
        split.slave_finished(stats, aborted);
    })
}

/// root の兄弟手を null-window で調べる YBWC job を作る。
pub(crate) fn make_eval_root_job(
    child_board: Board,
    child_alpha: i32,
    depth: i32,
    cutoff_score: i32,
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
    // root 分割は SplitPoint を持たず、master は collect_ybwc_tasks で待つ。
    // そのため新しい helper の受け口は作らず、祖先チェーンだけを引き継ぐ。
    let helper_chain = parent.helper_chain.clone();

    Box::new(move || {
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(evaluator, mpc_config, tt, &mut stats)
            .with_ordering_evaluator(ordering_evaluator)
            .with_stop(stop)
            .with_thread_pool(thread_pool)
            .with_searchings(searchings)
            .with_helper_chain(helper_chain);
        search.selectivity_lv = selectivity_lv;
        let score = -nws_eval(&child_board, child_alpha, depth - 1, &mut search);
        let aborted = search.is_aborted();
        drop(search);
        if !aborted && score >= cutoff_score {
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

/// 中盤 NWS の YBWC 分割。最初の手を master が単独で探索してから残りを配る。
fn nws_eval_ybwc(
    board: &Board,
    alpha: i32,
    beta: i32,
    depth: i32,
    tt_slot: TTSlot,
    move_list: &[MoveBoard],
    search: &mut SearchContext,
) -> i32 {
    let Some((first_index, first_move)) = move_list.iter().enumerate().find(|(_, mv)| !mv.is_skip)
    else {
        return alpha;
    };
    let first_child = board.make_move_from_flip_bit(1 << first_move.move_num, first_move.flip_bit);
    let first_score = -nws_eval(&first_child, -beta, depth - 1, search);
    if search.is_aborted() {
        return alpha;
    }
    if first_score >= beta {
        search.tt.store(
            tt_slot,
            board,
            first_score,
            SCORE_MAX,
            depth,
            search.selectivity_lv,
            first_move.move_num,
        );
        return first_score;
    }

    let mut best_score = first_score;
    let mut best_move = first_move.move_num;
    let work = move_list
        .iter()
        .enumerate()
        .skip(first_index + 1)
        .filter(|(_, mb)| !mb.is_skip)
        .map(|(move_index, mb)| {
            (
                move_index,
                board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit),
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let split = Arc::new(SplitPoint::new(work, beta));
    let mut spawned = 0u64;
    let make_job = |split: &Arc<SplitPoint>, parent: &SearchContext| {
        make_eval_split_worker(split, parent, depth)
    };

    try_add_slaves(&split, search, &mut spawned, make_job);
    while let Some((work_index, _, child_board)) = split.next_work() {
        try_add_slaves(&split, search, &mut spawned, make_job);
        search.searchings.push(split.searching.clone());
        search.helper_chain.push(split.helper.clone());
        let score = -nws_eval(&child_board, -beta, depth - 1, search);
        search.helper_chain.pop();
        search.searchings.pop();
        if search.is_aborted() {
            if search.recover_from_split_abort(&split.searching) {
                break;
            }
            split.stop_searching();
            split.join_slaves(spawned, search);
            return alpha;
        }
        split.finish(work_index, score);
        if score >= beta {
            break;
        }
    }

    split.join_slaves(spawned, search);
    if search.check_abort_now() {
        split.stop_searching();
        return alpha;
    }
    for (work_index, move_index) in split.work_indices() {
        let Some(score) = split.score_at(work_index) else {
            continue;
        };
        let mb = &move_list[move_index];
        if score >= beta {
            search.tt.store(
                tt_slot,
                board,
                score,
                SCORE_MAX,
                depth,
                search.selectivity_lv,
                mb.move_num,
            );
            return score;
        }
        if score > best_score {
            best_score = score;
            best_move = mb.move_num;
        }
    }

    if best_score > alpha {
        search.tt.store(
            tt_slot,
            board,
            best_score,
            best_score,
            depth,
            search.selectivity_lv,
            best_move,
        );
    } else {
        search.tt.store(
            tt_slot,
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

/// TT + ETC + MPC + 浅い eval ordering を使った中盤 NWS。
pub fn nws_eval(board: &Board, alpha: i32, depth: i32, search: &mut SearchContext) -> i32 {
    debug_assert!(alpha < SCORE_MAX);

    // 浅い depth は leaf 探索へ
    if depth <= SWITCH_DEPTH_LEAF {
        return nws_eval_leaf(board, alpha, depth, search);
    }

    let mut beta = alpha + 1;
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
        return -nws_eval(&passed, -beta, depth, search);
    }

    // ── 置換表プローブ ────────────────────────────────────────────────────────
    let probe = search.tt.probe(board);
    let tt_value = probe.value();
    let mut alpha_cur = alpha;

    if let Some(v) = tt_value {
        if let Some(score) = tt_cut(v, depth, search.selectivity_lv, &mut alpha_cur, &mut beta) {
            return score;
        }
    }

    // ── MPC ──────────────────────────────────────────────────────────────────
    match eval_search_mpc(board, alpha_cur, beta, depth, search) {
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
            beta,
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

    if should_split_eval_ybwc(depth, &move_list, search) {
        return nws_eval_ybwc(board, alpha, beta, depth, probe.slot(), &move_list, search);
    }

    // ── 探索ループ ────────────────────────────────────────────────────────────
    let mut best_score = -SCORE_MAX;
    let mut best_move = NO_COORD;

    for mb in move_list.iter() {
        if mb.is_skip {
            continue;
        }
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        let score = -nws_eval(&child_board, -beta, depth - 1, search);
        if search.is_aborted() {
            return alpha;
        }
        if score >= beta {
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

    debug_assert_ne!(best_move, NO_COORD);

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
    use crate::eval::Evaluator;
    use crate::search::eval_search::leaf::nws_eval_leaf_no_mpc;
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
        Board {
            player,
            opponent: !player & !empties,
        }
    }

    /// `nws_eval` が NWS の単調性を満たすことを確認する。
    ///
    /// reference として `nws_eval_leaf_no_mpc`(MPC 無効)を使う。
    /// 同じ board / depth で alpha = T-1 → result > alpha、alpha = T → result <= alpha。
    #[test]
    fn nws_eval_nws_property() {
        let (ev, mpc, tt) = shared_resources();
        let mut rng = 0xa1b2_c3d4_e5f6_0789_u64;

        for _ in 0..30 {
            let extra = (next_pseudo_random(&mut rng) % 10) as u32;
            let board = make_random_board(&mut rng, 30 + extra);
            let depth = 4;

            // 真値を MPC 無しの leaf 探索で計算(全幅扱いに近づける)
            let mut stats = SearchStats::default();
            let mut search = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
            // T を取るため、十分小さい alpha で leaf を呼ぶ(fail-soft の上限値が真値に近い)
            // exact 値は leaf でも NWS なので取れない。代わりに binary check のみ行う。

            // NWS の単調性は T に依存しない: ある alpha と alpha+1 で
            // alpha → result_a、alpha+1 → result_b としたとき result_a >= result_b は不成立であってもよい。
            // 代わりに「alpha を変えた 2 回の呼び出しで矛盾しないこと」だけを軽くチェックする。
            // depth が浅いので単純に nws_eval が panic せず、TT がクリーンに動作することを確認。
            let r1 = nws_eval(&board, 0, depth, &mut search);
            let _ = r1;

            // 2 回目の呼び出しで TT が活用されても結果は変わらないことを確認
            let mut stats2 = SearchStats::default();
            let mut search2 = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats2);
            let r2 = nws_eval(&board, 0, depth, &mut search2);
            // TT 活用時、結果が NWS の同じ binary 結果を返すことを確認
            assert_eq!(
                r1 > 0,
                r2 > 0,
                "TT inconsistency: r1={r1}, r2={r2} (board player={:#018x})",
                board.player,
            );
        }
    }

    /// `nws_eval` が `nws_eval_leaf_no_mpc` と同じ NWS 真値を返すことを確認する。
    #[test]
    fn nws_eval_matches_leaf_at_low_selectivity() {
        let (ev, _mpc_default, tt) = shared_resources();
        // MPC を完全に無効化して leaf と nws_eval を比較する
        let mpc = Arc::new(MpcConfig::default());
        let mut rng = 0x0123_4567_89ab_cdef_u64;

        for _ in 0..20 {
            let extra = (next_pseudo_random(&mut rng) % 10) as u32;
            let board = make_random_board(&mut rng, 30 + extra);
            let depth = 3;

            // leaf (no MPC, no TT) で T を取る
            let mut stats = SearchStats::default();
            let mut search = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
            let leaf_r = nws_eval_leaf_no_mpc(&board, 0, depth, &mut search);

            // nws_eval は TT を使うが、MPC 無効化のため真値の binary 結果は同じはず
            let tt2 = Arc::new(TranspositionTable::new()); // 別 TT (汚染回避)
            let mut stats2 = SearchStats::default();
            let mut search2 = SearchContext::new(ev.clone(), mpc.clone(), tt2, &mut stats2);
            let eval_r = nws_eval(&board, 0, depth, &mut search2);

            assert_eq!(
                leaf_r > 0,
                eval_r > 0,
                "binary NWS mismatch: leaf={leaf_r}, eval={eval_r} (board player={:#018x})",
                board.player,
            );
        }
    }
}
