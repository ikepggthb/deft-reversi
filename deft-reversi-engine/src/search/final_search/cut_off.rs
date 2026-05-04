//! 終盤探索用の置換表カット helpers。
//!
//! - [`tt_cut`]: 現局面の TT 値で alpha/beta を絞り込む
//! - [`e_tt_cut`]: 子局面の TT 値で親ノードの早期カットを試みる(Enhanced TT Cutoff)

use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::final_search::move_list::MoveBoard;
use crate::search::search::SearchContext;
use crate::t_table::TTValue;

/// 完全読みを示す置換表レベル。
pub const FINAL_LV: i32 = 60;

/// 置換表の保存値を使って `alpha`/`beta` を絞り込み、早期カットを試みる。
///
/// `lv` / `selectivity_lv` が保存値と一致しない場合は何もしない。
/// fail-low / fail-high / exact score の場合は `Some(score)` を返す。
#[inline(always)]
pub fn tt_cut(
    value: TTValue,
    lv: i32,
    selectivity_lv: i32,
    alpha: &mut i32,
    beta: &mut i32,
) -> Option<i32> {
    if value.lv() as i32 != lv || value.selectivity_lv() as i32 != selectivity_lv {
        return None;
    }
    let upper = value.upper() as i32;
    let lower = value.lower() as i32;

    if upper <= *alpha {
        return Some(upper);
    }
    if lower >= *beta {
        return Some(lower);
    }
    if upper == lower {
        return Some(upper);
    }
    if lower > *alpha {
        *alpha = lower;
    }
    if upper < *beta {
        *beta = upper;
    }
    None
}

/// 子ノードの置換表値を見て親ノードで早期カットを試みる(Enhanced TT Cutoff)。
///
/// カット発生で `Some(score)` を返す。カット不要な子には `skip=true` を立て、
/// `n_skip` を加算する。
#[inline(always)]
pub fn e_tt_cut(
    alpha: &mut i32,
    beta: &mut i32,
    move_list: &mut [MoveBoard],
    selectivity_lv: i32,
    n_skip: &mut i32,
    search: &SearchContext,
) -> Option<i32> {
    for mb in move_list.iter_mut() {
        if mb.skip {
            continue;
        }
        let Some(t) = search.tt.get(&mb.board) else {
            continue;
        };
        if t.lv() as i32 != FINAL_LV || t.selectivity_lv() as i32 != selectivity_lv {
            continue;
        }
        // 子ノードスコアを親視点に変換: child [lower, upper] → parent [-upper, -lower]
        let parent_lower = -(t.upper() as i32);
        let parent_upper = -(t.lower() as i32);

        mb.eval += 5; // TT 登録済みの手を優先

        if parent_lower >= *beta {
            return Some(parent_lower);
        }
        if parent_lower > *alpha {
            *alpha = parent_lower;
        }
        if parent_upper <= *alpha {
            mb.skip = true;
            *n_skip += 1;
        }
    }
    None
}

/// move ordering に使う浅い評価 + mobility スコアを付与する。
///
/// `eval_depth == 0` のときは静的評価関数を使う。
/// `eval_depth > 0` のときは `nws_eval_leaf_no_mpc` で shallow NWS を実行する。
#[inline(always)]
pub fn assign_ordering_scores(
    move_list: &mut [MoveBoard],
    alpha: i32,
    eval_depth: i32,
    mobility_weight: i32,
    search: &mut SearchContext,
) {
    use crate::search::eval_search::nws_eval_leaf_no_mpc;

    const CORNER_MASK: u64 = 0x8100_0000_0000_0081;
    let order_alpha = (alpha - 6).max(-SCORE_MAX);

    for mb in move_list.iter_mut() {
        if mb.skip {
            continue;
        }
        let eval = if eval_depth == 0 {
            search.evaluator.evaluate_board_slow(&mb.board)
        } else {
            -nws_eval_leaf_no_mpc(&mb.board, order_alpha, eval_depth, search)
        };

        let opp_moves = mb.board.moves();
        let mobility = -(opp_moves.count_ones() as i32) * 2
            - (opp_moves & CORNER_MASK).count_ones() as i32;

        mb.eval += eval + mobility * mobility_weight;
    }
}
