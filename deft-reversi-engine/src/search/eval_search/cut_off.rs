//! 中盤評価探索用の置換表カット helpers。
//!
//! 終盤探索 (`final_search/cut_off.rs`) との違い:
//! - lv の比較が **以上** (`stored_lv >= depth`)。深く探索された TT エントリは
//!   現在の探索よりも信頼できるため、現在の depth 以上に探索された値があれば使う。
//! - selectivity_lv も同様に `stored_selectivity_lv >= selectivity_lv` で判定する。

use crate::search::final_search::move_list::MoveBoard;
use crate::search::search::SearchContext;
use crate::t_table::TTValue;

/// TT 値で alpha/beta を絞り込み、早期カットを試みる。
///
/// 保存値の `lv` が `depth` 以上、`selectivity_lv` が現探索以上のときのみ
/// 値を信頼してカットする。
#[inline(always)]
pub fn tt_cut(
    value: TTValue,
    depth: i32,
    selectivity_lv: i32,
    alpha: &mut i32,
    beta: &mut i32,
) -> Option<i32> {
    if (value.lv() as i32) < depth || (value.selectivity_lv() as i32) < selectivity_lv {
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

/// 子ノードの TT 値を見て早期カットを試みる(Enhanced TT Cutoff)。
///
/// 子は親の `depth - 1` で探索されるため、`stored_lv >= depth - 1` を要求する。
#[inline(always)]
pub fn e_tt_cut(
    alpha: &mut i32,
    beta: &mut i32,
    move_list: &mut [MoveBoard],
    depth: i32,
    selectivity_lv: i32,
    n_skip: &mut i32,
    search: &SearchContext,
) -> Option<i32> {
    let child_lv = depth - 1;
    for mb in move_list.iter_mut() {
        if mb.skip {
            continue;
        }
        let Some(t) = search.tt.get(&mb.board) else {
            continue;
        };
        if (t.lv() as i32) < child_lv || (t.selectivity_lv() as i32) < selectivity_lv {
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
