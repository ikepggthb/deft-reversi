use crate::t_table::TranspositionTable;
use crate::{board::*, TableData};

use crate::move_list::*;


#[inline(always)]
pub fn t_table_cut_off_td(
    alpha: &mut i32,
    beta: &mut i32,
    lv: i32,
    selectivity_lv: i32,
    table_data: &Option<&TableData>,
) -> Option<i32> {
    if let Some(t) = table_data {
        if t.lv as i32 != lv || t.selectivity_lv as i32 != selectivity_lv {
            return None;
        }
        let max = t.max as i32;
        let min = t.min as i32;
        if max <= *alpha {
            return Some(max);
        } else if min >= *beta {
            return Some(min);
        } else if max == min {
            return Some(max);
        }
        if min > *alpha {
            *alpha = min
        };
        if max < *beta {
            *beta = max
        };
    }
    None
}

#[inline(always)]
pub fn t_table_cut_off(
    board: &Board,
    alpha: &mut i32,
    beta: &mut i32,
    lv: i32,
    selectivity_lv: i32,
    t_table: &TranspositionTable,
) -> Option<i32> {
    if let Some(t) = t_table.get(board) {
        if t.lv as i32 != lv || t.selectivity_lv as i32 != selectivity_lv {
            return None;
        }
        let max = t.max as i32;
        let min = t.min as i32;
        if max <= *alpha {
            return Some(max);
        } else if min >= *beta {
            return Some(min);
        } else if max == min {
            return Some(max);
        }
        if min > *alpha {
            *alpha = min
        };
        if max < *beta {
            *beta = max
        };
    }
    None
}


#[inline(always)]
pub fn et_cut_off(
    alpha: &mut i32,
    beta: &mut i32,
    move_list: &mut [MoveBoard],
    lv: i32,
    selectivity_lv: i32,
    n_skip: &mut i32,
    t_table: &TranspositionTable,
) -> Option<i32> {
    for move_board in move_list.iter_mut() {
        if move_board.skip {
            continue;
        }
        if let Some(t) = t_table.get(&move_board.board) {
            if t.lv as i32 != (if lv == 60 { 60 } else { lv - 1 })
                || t.selectivity_lv as i32 != selectivity_lv
            {
                continue;
            }
            let u: i32 = t.max as i32;
            let l = t.min as i32;
            move_board.eval += 5; // 置換表に登録されている手を優先

            // 1手進めた手: [l, u]
            // 親ノード [-u, -l]
            if -u >= *beta {
                // alpha < beta <= -u <= -l
                // println!("fail high !");
                return Some(-u); // fail high
            } else if *alpha <= -u {
                // alpha <= -u <= beta <= -l or alpha <= -u <= -l <= beta
                *alpha = -u; // update alpha (alpha <= -u)
                if -l <= *alpha || u == l {
                    //この条件ならば、この子ノードは探索する必要がない
                    move_board.skip = true;
                    *n_skip += 1;
                }
            } else if -l <= *alpha {
                // -u <= -l <= alpha < beta
                move_board.skip = true;
                *n_skip += 1;
            }
        }
    }
    None
}