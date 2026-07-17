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
        final_search::{negaalpha::negaalpha_final, nws::nws_final, solve_score::solve_score},
        move_list::*,
        mpc::{final_search_mpc, ProbCutResult},
        search::SearchContext,
        stability_cut::stability_cut_pvs,
    },
    t_table::{TTProbe, TTValue},
};

const TT_MOVE0_SCORE: i32 = 1 << 8;
const TT_MOVE1_SCORE: i32 = 1 << 7;

const FINAL_LV: i32 = 60;

/// 空きマスがこれ以下のとき `negaalpha_final` に切り替える。
///
/// PVS は全幅探索なので、基底は NWS ではなく厳密探索に委ねる。
const SWITCH_EMPTIES_NEGAALPHA: i32 = 12;

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
    let tt_value: Option<TTValue> = probe.value();

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
        13..17 => 1,
        17..21 => 2,
        _ => ((empty_count / 3) - 1).max(1),
    };

    assign_ordering_scores(board, &mut move_list, eval_depth, alpha_cur, search);
    sort_move_list(&mut move_list);
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
