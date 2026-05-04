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

use crate::board::board::Board;
use crate::board::constant::NO_COORD;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::final_search::cut_off::{assign_ordering_scores, e_tt_cut, tt_cut, FINAL_LV};
use crate::search::final_search::move_list::{
    assign_ffs_scores, build_tt_move_list, set_move_list, sort_move_list, uninit_move_array,
    MoveBoard,
};
use crate::search::final_search::negaalpha::negaalpha_final;
use crate::search::final_search::solve_score::solve_score;
use crate::search::mpc::{final_search_mpc, ProbCutResult};
use crate::search::search::SearchContext;
use crate::t_table::TT_MOVES_CAPACITY;

/// 空きマスがこれ以下のとき `negaalpha_final` に切り替える。
const SWITCH_EMPTIES_NEGA_ALPHA: i32 = 5;

/// 空きマスがこれ以下のとき `nws_final_simple` に切り替える。
const SWITCH_EMPTIES_SIMPLE_NWS: i32 = 10;

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

    let moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -nws_final_simple(&passed, -beta, search);
    }

    match final_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    let move_count = moves_bit.count_ones() as usize;
    let mut move_list = uninit_move_array();
    let move_list = &mut move_list[..move_count];
    set_move_list(board, moves_bit, move_list);

    if move_count >= 2 {
        assign_ffs_scores(move_list);
        sort_move_list(move_list);
    }

    let mut best_score = -SCORE_MAX;
    for mb in move_list.iter() {
        let score = -nws_final_simple(&mb.board, -beta, search);
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
    let mut beta = alpha + 1;

    let n_empties = (board.player | board.opponent).count_zeros() as i32;
    if n_empties <= SWITCH_EMPTIES_SIMPLE_NWS {
        return nws_final_simple(board, alpha, search);
    }

    search.stats.final_search_nodes += 1;

    let mut moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -nws_final(&passed, -beta, search);
    }

    // ── 置換表プローブ ───────────────────────────────────────────────────────
    let probe = search.tt.probe(board);
    let tt_value = probe.value();
    let mut alpha_cur = alpha;

    // TT に登録済みの手を moves_bit から除いて、先に探索できるようにする
    let tt_moves: Option<[u8; TT_MOVES_CAPACITY]> = if let Some(v) = tt_value {
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

    // tt_cut
    if let Some(v) = tt_value {
        if let Some(score) = tt_cut(v, FINAL_LV, search.selectivity_lv, &mut alpha_cur, &mut beta) {
            return score;
        }
    }

    // MPC
    match final_search_mpc(board, alpha_cur, beta, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    // ── TT 手リスト (最大 N_TT_MOVES) ────────────────────────────────────────
    let mut tt_move_list: [MoveBoard; TT_MOVES_CAPACITY] = [MoveBoard::SENTINEL; TT_MOVES_CAPACITY];
    let tt_move_count = build_tt_move_list(board, &tt_moves, &mut tt_move_list);
    let tt_move_list = &mut tt_move_list[..tt_move_count];

    // ── 通常手リスト ──────────────────────────────────────────────────────────
    let move_count = moves_bit.count_ones() as usize;
    let mut move_list = uninit_move_array();
    let move_list = &mut move_list[..move_count];
    set_move_list(board, moves_bit, move_list);

    // ETC (n_empties > 12 のときだけ有益)
    let mut n_skip = 0i32;
    if n_empties > 12 {
        let sl = search.selectivity_lv;
        if let Some(score) = e_tt_cut(&mut alpha_cur, &mut beta, tt_move_list, sl, &mut 0, search) {
            return score;
        }
        if let Some(score) = e_tt_cut(&mut alpha_cur, &mut beta, move_list, sl, &mut n_skip, search) {
            return score;
        }
    }

    // ── move ordering: 浅い eval + mobility ──────────────────────────────────
    if move_count - n_skip as usize >= 2 {
        let eval_depth = match n_empties {
            11..13 => 0,
            13..17 => 1,
            17..21 => 2,
            _ => ((n_empties / 3) - 1).max(0),
        };
        assign_ordering_scores(move_list, alpha_cur, eval_depth, 1, search);
        sort_move_list(move_list);
    }

    // ── 探索ループ ────────────────────────────────────────────────────────────
    let mut best_score = -SCORE_MAX;
    let mut best_move = NO_COORD;

    // TT 手から先に探索
    for mb in tt_move_list.iter() {
        let score = -nws_final(&mb.board, -beta, search);
        if score >= beta {
            search.tt.store(
                probe.slot(), board, score, SCORE_MAX, FINAL_LV,
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

    // 通常手
    for mb in move_list.iter() {
        if mb.skip {
            continue;
        }
        let score = -nws_final(&mb.board, -beta, search);
        if score >= beta {
            search.tt.store(
                probe.slot(), board, score, SCORE_MAX, FINAL_LV,
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

    // TT に結果を保存
    if best_score > alpha {
        search.tt.store(
            probe.slot(), board, best_score, best_score, FINAL_LV,
            search.selectivity_lv, best_move,
        );
    } else {
        search.tt.store(
            probe.slot(), board, -SCORE_MAX, best_score, FINAL_LV,
            search.selectivity_lv, best_move,
        );
    }

    best_score
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
        Board { player, opponent: !player & !empties }
    }

    /// NWS の正しさを検証するヘルパー。
    ///
    /// `brute_force` で真スコアを求めてから、`alpha = true_score - 1` と `alpha = true_score`
    /// で NWS を呼ぶ。
    /// - `alpha = T-1` → beta=T → true_score ≥ T=beta → fail-high → result ≥ beta=T
    /// - `alpha = T`   → beta=T+1 → true_score=T ≤ alpha=T → fail-low → result ≤ T
    fn check_nws<F>(board: &Board, ev: &Arc<Evaluator>, mpc: &Arc<MpcConfig>, tt: &Arc<TranspositionTable>, mut nws: F)
    where
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
            true_score - 1, board.player,
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
            check_nws(&board, &ev, &mpc, &tt, |b, alpha, s| nws_final_simple(b, alpha, s));
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
