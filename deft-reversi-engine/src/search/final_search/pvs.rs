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

use arrayvec::ArrayVec;

use crate::board::board::Board;
use crate::board::constant::NO_COORD;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::final_search::cut_off::{assign_ordering_scores, e_tt_cut, tt_cut, FINAL_LV};
use crate::search::move_list::{MoveIterator, MoveIteratorParity, MoveBoard, MOVE_MAX};
use crate::search::final_search::negaalpha::negaalpha_final;
use crate::search::final_search::nws::nws_final;
use crate::search::final_search::solve_score::solve_score;
use crate::search::mpc::{final_search_mpc, ProbCutResult};
use crate::search::search::SearchContext;
use crate::t_table::{TT_MOVES_CAPACITY, TTProbe, TTValue};


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

    let mut moves_bit = board.moves();

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

    // ── 置換表 取得 ────────────────────────────────────────────────────────
    let probe: TTProbe = search.tt.probe(board);
    let tt_value = probe.value();
    let mut alpha_cur = alpha;
    let mut beta_cur = beta;

    if let Some(v) = tt_value {
        if let Some(score) = tt_cut(v, FINAL_LV, search.selectivity_lv, &mut alpha_cur, &mut beta_cur) {
            return score;
        }
    }

    // ── MPC ──────────────────────────────────────────────────────────────────
    match final_search_mpc(board, alpha_cur, beta_cur, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    // ── TT 手リスト ───────────────────────────────────────────────────────────
    let (tt_move_list, tt_moves_bit): (Option<[MoveBoard; TT_MOVES_CAPACITY]>, u64) = {
        match tt_value {
            Some(value) => {
                let tt_moves = [
                    MoveBoard {
                        score: 0,
                        move_num: value.move0,
                        flip_bit: board.flip_bit(1u64 << value.move0),
                    },
                    MoveBoard {
                        score: 0,
                        move_num: value.move1,
                        flip_bit: board.flip_bit(1u64 << value.move1),
                    },
                ];

                // 全消しの場合は、即時return
                if (board.opponent ^ tt_moves[0].flip_bit == 0)
                    || (board.opponent ^ tt_moves[1].flip_bit == 0) {
                    return SCORE_MAX;
                }

                (Some(tt_moves), (1u64 << value.move0) | (1u64 << value.move1))
            },
            None => (None, 0),
        }
    };

    // ── 通常手リストの生成 ──────────────────────────────────────────────────────
    let mut move_list = ArrayVec::<MoveBoard, MOVE_MAX>::new();
    let normal_moves_bit = moves_bit & !tt_moves_bit; // tt 手以外の通常手のbit
    while normal_moves_bit != 0 {
        let move_num = normal_moves_bit.trailing_zeros() as u8;
        let flip_bit = board.flip_bit(1u64 << move_num);

        // score算出やmove_listのソートは、探索直前に行う
        move_list.push(MoveBoard { score: 0, move_num, flip_bit });
        // 全消しの場合は、即時return
        if board.opponent ^ flip_bit == 0 {
            return SCORE_MAX;
        }
        normal_moves_bit &= normal_moves_bit - 1;
    }

    // ── ETC ───────────────────────────────────────────────────────────────────
    let mut n_skip = 0i32;
    {
        let sl = search.selectivity_lv;
        if let Some(score) = e_tt_cut(&mut alpha_cur, &mut beta_cur, tt_move_list, sl, &mut 0, search) {
            return score;
        }
        if let Some(score) = e_tt_cut(&mut alpha_cur, &mut beta_cur, move_list, sl, &mut n_skip, search) {
            return score;
        }
    }

    // ── move ordering ─────────────────────────────────────────────────────────
    let eval_depth = match empty_count {
        13..17 => 1,
        17..21 => 2,
        _ => ((empty_count / 3) - 1).max(1),
    };

    if move_count - n_skip as usize >= 2 {
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
            -pvs_final(&mb.board, -beta_cur, -alpha_cur, search)
        } else {
            let s = -nws_final(&mb.board, -(alpha_cur + 1), search);
            if s > alpha_cur && s < beta_cur {
                -pvs_final(&mb.board, -beta_cur, -alpha_cur, search)
            } else {
                s
            }
        };

        if score >= beta_cur {
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

    for mb in move_list.iter() {
        if mb.skip {
            continue;
        }
        let score = if is_first {
            is_first = false;
            -pvs_final(&mb.board, -beta_cur, -alpha_cur, search)
        } else {
            let s = -nws_final(&mb.board, -(alpha_cur + 1), search);
            if s > alpha_cur && s < beta_cur {
                -pvs_final(&mb.board, -beta_cur, -alpha_cur, search)
            } else {
                s
            }
        };

        if score >= beta_cur {
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
        Board { player, opponent: !player & !empties }
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
                let mut search = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
                let result = pvs_final(&board, alpha, beta, &mut search);

                if true_score >= beta {
                    assert!(result >= beta,
                        "fail-high broken: alpha={alpha} beta={beta} result={result} true={true_score}");
                } else if true_score <= alpha {
                    assert!(result <= alpha,
                        "fail-low broken: alpha={alpha} beta={beta} result={result} true={true_score}");
                } else {
                    assert_eq!(result, true_score,
                        "exact mismatch: alpha={alpha} beta={beta} true={true_score}");
                }
            }
        }
    }
}
