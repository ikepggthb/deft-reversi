//! 置換表カット helpers。
//!
//! 現在の実装では、保存済みの `lv` / `selectivity_lv` が要求値と完全一致する
//! TT entry だけを cut / ETC に使う。

use crate::board::board::Board;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::move_list::MoveBoard;
use crate::search::search::SearchContext;
use crate::t_table::*;

pub enum ETCResult {
    BetaCut(i32),
    NarrowAlpha(i32),
    AllMovesSkipped(i32),
}

/// TT 値で alpha/beta を絞り込み、早期カットを試みる。
///
/// 保存値の `lv` / `selectivity_lv` が現在の探索条件と完全一致するときのみ
/// 値を信頼する。
#[inline(always)]
pub fn tt_cut(
    tt_value: TTValue,
    lv: i32,
    selectivity_lv: i32,
    alpha: &mut i32,
    beta: &mut i32,
) -> Option<i32> {
    if (tt_value.lv as i32) != lv || (tt_value.selectivity_lv as i32) != selectivity_lv {
        return None;
    }
    let upper = tt_value.upper as i32;
    let lower = tt_value.lower as i32;

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

/// 子ノードの TT 値を見て早期カットを試みる(Enhanced TT Cutoff)。
///
/// `child_lv` と完全一致する子entryだけを使う。中盤探索では親depth-1、
/// 完全読みでは全ノード共通のFINAL_LVを呼び出し側が渡す。
/// `parent_upper <= alpha` の手は、呼び出し元の元の alpha を超えられないため
/// `is_skip` を立てる。ETC 中に更新した alpha では skip 判定しない。
#[inline(always)]
pub fn e_tt_cut(
    board: &Board,
    alpha: i32,
    beta: i32,
    move_list: &mut [MoveBoard],
    child_lv: i32,
    selectivity_lv: i32,
    search: &SearchContext,
) -> ETCResult {
    let mut best_upper_on_skip = -SCORE_MAX;

    let mut alpha_cur = alpha;
    let mut count_skip = 0;

    for mb in move_list.iter_mut() {
        let board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        let tt_value = match search.tt.probe(&board) {
            TTProbe::Hit { value, .. } => value,
            TTProbe::Miss { .. } => continue,
        };

        if (tt_value.lv as i32) != child_lv || (tt_value.selectivity_lv as i32) != selectivity_lv {
            continue;
        }

        // 子ノードスコアを親視点に変換: child [lower, upper] → parent [-upper, -lower]
        let parent_lower = -(tt_value.upper as i32);
        let parent_upper = -(tt_value.lower as i32);

        mb.score += 1 << 6; // TT 登録済みの手を優先

        if parent_lower >= beta {
            return ETCResult::BetaCut(parent_lower);
        }
        if parent_lower > alpha_cur {
            alpha_cur = parent_lower;
        }
        if parent_upper <= alpha {
            mb.is_skip = true;
            count_skip += 1;
            if parent_upper > best_upper_on_skip {
                best_upper_on_skip = parent_upper;
            }
        }
    }

    if move_list.len() == count_skip {
        ETCResult::AllMovesSkipped(best_upper_on_skip)
    } else {
        ETCResult::NarrowAlpha(alpha_cur)
    }
}
