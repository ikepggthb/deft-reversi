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
        final_search::{
            leaf::QUADRANT_ID,
            negaalpha::negaalpha_final,
            solve_score::{final_parity, solve_score},
        },
        move_list::*,
        mpc::{final_search_mpc, ProbCutResult},
        search::SearchContext,
        stability_cut::stability_cut_nws,
        tt_cut::*,
    },
    t_table::{TTProbe, TTValue},
};
use arrayvec::ArrayVec;
use std::{cell::UnsafeCell, cmp};

const TT_MOVE0_SCORE: i32 = 1 << 20;
const TT_MOVE1_SCORE: i32 = 1 << 19;

const FINAL_LV: i32 = 60;

/// 空きマスがこれ以下のとき `negaalpha_final` に切り替える。
const SWITCH_EMPTIES_NEGA_ALPHA: i32 = 5;

/// 空きマスがこれ以下のとき `nws_final_simple` に切り替える。
const SWITCH_EMPTIES_SIMPLE_NWS: i32 = 13;

const LEGAL_UNDEFINED: u64 = u64::MAX;

#[derive(Clone, Copy)]
struct SimpleMove {
    score: i32,
    move_num: u8,
    is_skip: bool,
    flip_bit: u64,
    /// 局所TTで解決した手では `LEGAL_UNDEFINED` のまま(探索に使われない)。
    child_moves: u64,
}

const LOCAL_TT_SIZE: usize = 2048;
const LOCAL_TT_LAYERS: usize = (SWITCH_EMPTIES_SIMPLE_NWS - SWITCH_EMPTIES_NEGA_ALPHA) as usize;

#[derive(Clone, Copy)]
struct LocalTTEntry {
    player: u64,
    opponent: u64,
    lower: i8,
    upper: i8,
    selectivity_lv: u8,
}

const EMPTY_LOCAL_TT_ENTRY: LocalTTEntry = LocalTTEntry {
    player: 0,
    opponent: 0,
    lower: -64,
    upper: 64,
    selectivity_lv: u8::MAX,
};

thread_local! {
    static LOCAL_END_TT: UnsafeCell<[LocalTTEntry; LOCAL_TT_SIZE * LOCAL_TT_LAYERS]> =
        const { UnsafeCell::new([EMPTY_LOCAL_TT_ENTRY; LOCAL_TT_SIZE * LOCAL_TT_LAYERS]) };
}

#[inline(always)]
fn local_tt_index(board: &Board, n_empties: i32) -> usize {
    let hash = (board.player ^ board.opponent.rotate_left(32)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let layer =
        (n_empties - SWITCH_EMPTIES_NEGA_ALPHA).clamp(0, LOCAL_TT_LAYERS as i32 - 1) as usize;
    layer * LOCAL_TT_SIZE + ((hash >> 53) as usize)
}

// ── nws_final_simple ──────────────────────────────────────────────────────────

/// TT なしの簡易 NWS 完全読み。FFS 手順 + MPC のみ。
///
/// 空きマスが少ないとき `negaalpha_final` へ降格する。
pub fn nws_final_simple(board: &Board, alpha: i32, search: &mut SearchContext) -> i32 {
    let n_empties = (board.player | board.opponent).count_zeros() as i32;
    LOCAL_END_TT.with(|table| {
        // SAFETY: the table is thread-local, and recursive calls stay in the impl.
        let table = unsafe { &mut *table.get() };
        nws_final_simple_impl(board, alpha, LEGAL_UNDEFINED, n_empties, search, table)
    })
}

#[inline(always)]
fn local_tt_child_entry(
    board: &Board,
    n_empties: i32,
    selectivity_lv: i32,
    local_tt: &[LocalTTEntry],
) -> Option<LocalTTEntry> {
    let entry = local_tt[local_tt_index(board, n_empties)];
    (entry.player == board.player
        && entry.opponent == board.opponent
        && entry.selectivity_lv as i32 == selectivity_lv)
        .then_some(entry)
}

#[inline(always)]
fn local_tt_store_child(
    board: &Board,
    score: i32,
    is_lower_bound: bool,
    n_empties: i32,
    selectivity_lv: i32,
    local_tt: &mut [LocalTTEntry],
) {
    let entry = &mut local_tt[local_tt_index(board, n_empties)];
    entry.player = board.player;
    entry.opponent = board.opponent;
    entry.selectivity_lv = selectivity_lv as u8;
    if is_lower_bound {
        entry.lower = score as i8;
        entry.upper = 64;
    } else {
        entry.lower = -64;
        entry.upper = score as i8;
    }
}

fn nws_final_simple_impl(
    board: &Board,
    alpha: i32,
    moves_bit: u64,
    n_empties: i32,
    search: &mut SearchContext,
    local_tt: &mut [LocalTTEntry],
) -> i32 {
    let beta = alpha + 1;

    if n_empties <= SWITCH_EMPTIES_NEGA_ALPHA {
        return negaalpha_final(board, alpha, beta, search);
    }

    search.stats.final_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    let moves_bit = if moves_bit == LEGAL_UNDEFINED {
        board.moves()
    } else {
        moves_bit
    };
    if moves_bit == 0 {
        let passed = board.passed();
        let passed_moves = passed.moves();
        if passed_moves == 0 {
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -nws_final_simple_impl(&passed, -beta, passed_moves, n_empties, search, local_tt);
    }

    if let Some(score) = stability_cut_nws(board, alpha, n_empties, search) {
        return score;
    }

    match final_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    if moves_bit.is_power_of_two() {
        let child_board = board.make_move(moves_bit);
        let child_moves = child_board.moves();
        let score = -nws_final_simple_impl(
            &child_board,
            -beta,
            child_moves,
            n_empties - 1,
            search,
            local_tt,
        );
        if search.is_aborted() {
            return alpha;
        }
        return score;
    }

    let mut move_list = ArrayVec::<SimpleMove, { SWITCH_EMPTIES_SIMPLE_NWS as usize }>::new();
    let parity = final_parity(board.player, board.opponent);
    let mut moves = moves_bit;
    while moves != 0 {
        let move_num = moves.trailing_zeros() as u8;
        let move_bit = 1u64 << move_num;
        let flip_bit = board.flip_bit(move_bit);
        if flip_bit == board.opponent {
            return SCORE_MAX;
        }
        // 4 分割した盤面のどこに属するか。`QUADRANT_ID` と同じ値を返す。
        let region = QUADRANT_ID[move_num as usize];
        move_list.push(SimpleMove {
            score: i32::from(parity & region != 0) * 17,
            move_num,
            is_skip: false,
            flip_bit,
            child_moves: LEGAL_UNDEFINED,
        });
        moves &= moves - 1;
    }
    let mut best_score = -SCORE_MAX;
    for mb in move_list.iter_mut() {
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        if let Some(entry) =
            local_tt_child_entry(&child_board, n_empties - 1, search.selectivity_lv, local_tt)
        {
            if entry.lower as i32 > alpha {
                return entry.lower as i32;
            }
            if entry.upper as i32 <= alpha {
                best_score = cmp::max(best_score, entry.upper as i32);
                mb.is_skip = true;
                continue;
            }
        }
        // 局所TTで解決しなかった手にだけ合法手生成の費用を払う。
        let child_moves = child_board.moves();
        mb.child_moves = child_moves;
        let mobility =
            child_moves.count_ones() + (child_moves & 0x8100_0000_0000_0081).count_ones();
        if mobility <= 1 {
            let score = -nws_final_simple_impl(
                &child_board,
                -beta,
                child_moves,
                n_empties - 1,
                search,
                local_tt,
            );
            if search.is_aborted() {
                return alpha;
            }
            if score >= beta {
                local_tt_store_child(
                    &child_board,
                    score,
                    true,
                    n_empties - 1,
                    search.selectivity_lv,
                    local_tt,
                );
                return score;
            }
            best_score = cmp::max(best_score, score);
            local_tt_store_child(
                &child_board,
                score,
                false,
                n_empties - 1,
                search.selectivity_lv,
                local_tt,
            );
            mb.is_skip = true;
            continue;
        }
        mb.score -= mobility as i32 * 18;
    }

    for move_index in 0..move_list.len() {
        let mut best_index = move_index;
        for i in (move_index + 1)..move_list.len() {
            if move_list[i].score > move_list[best_index].score {
                best_index = i;
            }
        }
        move_list.swap(move_index, best_index);
        let mb = &move_list[move_index];
        if mb.is_skip {
            continue;
        }
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        let score = -nws_final_simple_impl(
            &child_board,
            -beta,
            mb.child_moves,
            n_empties - 1,
            search,
            local_tt,
        );
        if search.is_aborted() {
            return alpha;
        }
        if score >= beta {
            local_tt_store_child(
                &child_board,
                score,
                true,
                n_empties - 1,
                search.selectivity_lv,
                local_tt,
            );
            return score;
        }
        if score > best_score {
            best_score = score;
        }
        local_tt_store_child(
            &child_board,
            score,
            false,
            n_empties - 1,
            search.selectivity_lv,
            local_tt,
        );
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
            //    11..13 => 0,
            //    13..17 => 1,
            //    17..21 => 2,
            1..24 => n_empties >> 4,
            _ => 1,
        };
        assign_ordering_scores_weighted(
            board,
            &mut move_list,
            eval_depth,
            alpha_cur,
            7 + 25 * eval_depth,
            17,
            1 << 15,
            search,
        );
        sort_move_list(&mut move_list);
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

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{default_evaluator, Evaluator};
    use crate::search::final_search::solve_score::solve_score;
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
        ev: &Arc<dyn Evaluator>,
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
            check_nws(&board, &ev, &mpc, &tt, nws_final);
        }
    }

    #[test]
    fn local_tt_does_not_reuse_selective_bound_for_exact_search() {
        let (ev, mpc, tt) = shared_resources();
        let board = Board {
            player: 0xbc61_9192_4c6b_1c86,
            opponent: 0x431e_6e24_2314_e159,
        };
        let n_empties = board.empties_count() as i32;
        let mut local_tt = [EMPTY_LOCAL_TT_ENTRY; LOCAL_TT_SIZE * LOCAL_TT_LAYERS];
        let move_bit = board.moves() & board.moves().wrapping_neg();
        let child = board.make_move(move_bit);
        let entry = &mut local_tt[local_tt_index(&child, n_empties - 1)];
        *entry = LocalTTEntry {
            player: child.player,
            opponent: child.opponent,
            lower: 64,
            upper: 64,
            selectivity_lv: 0,
        };

        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(ev, mpc, tt, &mut stats);
        search.selectivity_lv = crate::search::mpc::SELECTIVITY_LV_MAX;
        let true_score = brute_force(&board);
        let actual = nws_final_simple_impl(
            &board,
            true_score,
            LEGAL_UNDEFINED,
            n_empties,
            &mut search,
            &mut local_tt,
        );

        assert!(actual <= true_score);
    }
}
