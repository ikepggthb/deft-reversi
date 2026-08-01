//! 終盤の確定スコア計算と、空きマスが残り少ないときの高速 leaf solver。
//!
//! - `solve_score`: 任意の盤面に対する終局スコア(空きマスは勝っている側に加算)

use crate::board::board::Board;

/// 終局盤面の exact score を返す。
///
/// 空きマスが残っていても、勝っている側に空きマス数を加算する(終局時の慣習)。
#[inline(always)]
pub fn solve_score(board: &Board) -> i32 {
    let n_player = board.player.count_ones() as i32;
    let n_opponent = board.opponent.count_ones() as i32;
    let diff = n_player - n_opponent;

    if diff > 0 {
        diff + (64 - n_player - n_opponent)
    } else if diff < 0 {
        diff - (64 - n_player - n_opponent)
    } else {
        0
    }
}
