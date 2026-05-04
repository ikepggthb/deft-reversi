//! 終盤探索のための手リスト管理。
//!
//! `MoveBoard` は 1 手分の情報(着手後の盤面・着手位置・move ordering スコア・skip フラグ)
//! を保持する。`set_move_list` で手ビットマスクから一括生成し、
//! `sort_move_list` で eval 降順に並べ替える。

use crate::board::board::Board;
use crate::board::constant::NO_COORD;
use crate::search::final_search::move_iterator::MoveIterator;
use crate::t_table::N_TT_MOVES;

/// オセロの最大合法手数。
/// 参考: <https://eukaryote.hateblo.jp/entry/2023/05/17/163629>
pub const MOVE_MAX: usize = 33;

/// 1 手分の情報を保持する構造体。
#[derive(Clone, Copy)]
pub struct MoveBoard {
    /// move ordering スコア(高いほど先に探索)。
    pub eval: i32,
    /// 着手後の盤面。
    pub board: Board,
    /// 着手位置(0-63)。
    pub put_place: u8,
    /// ETC 等でカット済みの手をスキップするフラグ。
    pub skip: bool,
}

impl MoveBoard {
    pub const SENTINEL: Self = Self {
        eval: 0,
        board: Board { player: 0, opponent: 0 },
        put_place: 0,
        skip: true,
    };
}

/// `[MoveBoard; MOVE_MAX]` の初期化済みスタック配列を返す。
///
/// 全要素に `MoveBoard::SENTINEL` を書き込む。`set_move_list` で
/// 使用前に必ず実際の手で上書きすること。
#[inline(always)]
pub fn uninit_move_array() -> [MoveBoard; MOVE_MAX] {
    [MoveBoard::SENTINEL; MOVE_MAX]
}

/// `moves_bit` のすべての合法手を `move_list` に書き込む。
///
/// `move_list` の長さは `moves_bit.count_ones()` と一致しなければならない。
#[inline(always)]
pub fn set_move_list(board: &Board, moves_bit: u64, move_list: &mut [MoveBoard]) {
    for (i, move_bit) in MoveIterator::new(moves_bit).enumerate() {
        move_list[i] = MoveBoard {
            eval: 0,
            board: board.make_move(move_bit),
            put_place: move_bit.trailing_zeros() as u8,
            skip: false,
        };
    }
}

/// 4 コーナーのビットマスク。FFS ordering の corner penalty に使う。
const CORNER_MASK: u64 = 0x8100_0000_0000_0081;

/// "Few Freedoms Search" に基づく move ordering スコアを `move_list` に付加する。
///
/// 各手の後で相手が置ける手が少ない方が良いとみなし、
/// コーナーへの合法手はさらにペナルティを与える。
#[inline(always)]
pub fn assign_ffs_scores(move_list: &mut [MoveBoard]) {
    for mb in move_list.iter_mut() {
        let opp_moves = mb.board.moves();
        let n_moves = -(opp_moves.count_ones() as i32);
        let n_corners = -((opp_moves & CORNER_MASK).count_ones() as i32);
        mb.eval = n_moves * 2 + n_corners;
    }
}

/// 置換表から取り出した最大 `N_TT_MOVES` 手分の [`MoveBoard`] を `out` に書き込み、有効な手数を返す。
///
/// TT に記録された座標が合法手かどうかを `board.moves()` で確認する。
/// 重複チェックはしない(TT 手は通常手リストからあらかじめ除いておくこと)。
pub fn build_tt_move_list(
    board: &Board,
    tt_moves: &Option<[u8; N_TT_MOVES]>,
    out: &mut [MoveBoard; N_TT_MOVES],
) -> usize {
    let Some(moves) = tt_moves else {
        return 0;
    };
    let legal = board.moves();
    let mut count = 0;
    for &coord in moves.iter() {
        if coord == NO_COORD {
            continue;
        }
        let move_bit = 1u64 << coord;
        if legal & move_bit == 0 {
            continue;
        }
        out[count] = MoveBoard {
            eval: 0,
            board: board.make_move(move_bit),
            put_place: coord,
            skip: false,
        };
        count += 1;
    }
    count
}

/// `eval` 降順に `move_list` を安定でなく部分ソートする。
///
/// 上位 `TOP_N` 手だけを sorted にし、残りは未ソートのまま。
#[inline(always)]
pub fn sort_move_list(move_list: &mut [MoveBoard]) {
    const TOP_N: usize = 7;
    let n = move_list.len();
    if n <= TOP_N {
        move_list.sort_unstable_by_key(|mb| std::cmp::Reverse(mb.eval));
    } else {
        move_list.select_nth_unstable_by_key(TOP_N - 1, |mb| std::cmp::Reverse(mb.eval));
        move_list[..TOP_N].sort_unstable_by_key(|mb| std::cmp::Reverse(mb.eval));
    }
}
