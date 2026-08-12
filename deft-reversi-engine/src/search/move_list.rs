//! 終盤探索のための手リスト管理。
//!
//! `MoveBoard` は 1 手分の情報(着手後の盤面・着手位置・move ordering スコア・skip フラグ)
//! を保持する。`set_move_list` で手ビットマスクから一括生成し、
//! `sort_move_list` で eval 降順に並べ替える。

use arrayvec::ArrayVec;
use std::cmp;

use crate::board::board::Board;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::eval_search::negaalpha_eval_ordering;
use crate::search::SearchContext;

/// オセロの最大合法手数。
/// 参考: https://eukaryote.hateblo.jp/entry/2023/05/17/163629
///       https://github.com/eukaryo/reversi33
pub const MOVE_MAX: usize = 33;

/// 1 手分の情報を保持する構造体。
#[derive(Clone, Copy)]
pub struct MoveBoard {
    /// move ordering スコア(高いほど先に探索)。
    pub score: i32,
    /// 着手位置(0-63)。
    pub move_num: u8,
    /// 探索をスキップするか
    pub is_skip: bool,
    /// 着手する際に反転する石(bit)
    pub flip_bit: u64,
}

/// 4 コーナーのビットマスク。FFS ordering の corner penalty に使う。
const CORNER_MASK: u64 = 0x8100_0000_0000_0081;

pub fn make_move_list(board: &Board, moves_bit: u64) -> ArrayVec<MoveBoard, MOVE_MAX> {
    let mut move_list = ArrayVec::<MoveBoard, MOVE_MAX>::new();

    let mut moves = moves_bit;
    while moves != 0 {
        let move_num = moves.trailing_zeros() as u8;
        let flip_bit = board.flip_bit(1u64 << move_num);
        move_list.push(MoveBoard {
            score: 0,
            move_num,
            is_skip: false,
            flip_bit,
        });
        moves &= moves - 1;
    }

    move_list
}

#[allow(dead_code)]
pub fn simplest_eval(board: &Board) -> i32 {
    const SCORES: [i32; 64] = [
        120, -40, 1, 0, 0, 1, -40, 120, -40, -60, -5, -4, -4, -5, -60, -40, 1, -5, -1, -2, -2, -1,
        -5, 1, 0, -4, -2, -1, -1, -2, -4, 0, 0, -4, -2, -1, -1, -2, -4, 0, 1, -5, -1, -2, -2, -1,
        -5, 1, -40, -60, -5, -4, -4, -5, -60, -40, 120, -40, 1, 0, 0, 1, -40, 120,
    ];

    let m1 = [
        0x7E00000000000000u64,
        0x1010101010100,
        0x80808080808000,
        0x7e,
    ];
    let m2 = [
        0x8100000000000000u64,
        0x100000000000001,
        0x8000000000000080,
        0x81,
    ];

    let mut place_score = 0;

    let player_board = board.player;
    let opponent_board = board.opponent;

    for i in 0..4 {
        if ((player_board & m1[i]) | (opponent_board & m2[i])) == m1[i] {
            place_score += 120;
        }
        if ((opponent_board & m1[i]) | (player_board & m2[i])) == m1[i] {
            place_score -= 120;
        }
        let side = m1[i] | m2[i];
        if side & (player_board | opponent_board) == side {
            place_score += ((player_board & side).count_ones() as i32
                - (opponent_board & side).count_ones() as i32)
                * 20;
        }
    }

    let mut player_board_bit = player_board;
    let mut opponent_board_bit = opponent_board;
    while player_board_bit != 0 {
        let bit_index = player_board_bit.trailing_zeros() as usize;
        player_board_bit &= player_board_bit - 1; // 1番小さい桁の1を0にする。
        place_score += SCORES[bit_index];
    }
    while opponent_board_bit != 0 {
        let bit_index = opponent_board_bit.trailing_zeros() as usize;
        opponent_board_bit &= opponent_board_bit - 1; // 1番小さい桁の1を0にする。
        place_score -= SCORES[bit_index];
    }

    let player_piece_count = player_board.count_ones() as i32;
    let opponent_piece_count = opponent_board.count_ones() as i32;
    let piece_count_score = if player_piece_count + opponent_piece_count < 40 {
        opponent_piece_count - player_piece_count
    } else {
        0
    };

    let player_mobility = board.moves().count_ones() as i32;
    let opponent_mobility = board.opponent_moves().count_ones() as i32;

    let mobility_score = player_mobility - opponent_mobility;

    if player_mobility == 0 && opponent_mobility == 0 {
        if player_piece_count > opponent_piece_count {
            1000
        } else {
            -1000
        }
    } else {
        (place_score * 10 + mobility_score * 85 + piece_count_score * 40) / 40
    }
}

#[inline(always)]
pub fn assign_ordering_scores(
    board: &Board,
    move_list: &mut [MoveBoard],
    lv: i32,
    alpha: i32,
    search: &mut SearchContext,
) {
    assign_ordering_scores_weighted(board, move_list, lv, alpha, 1, 1, 0, search);
}

#[inline(always)]
pub fn assign_ordering_scores_weighted(
    board: &Board,
    move_list: &mut [MoveBoard],
    lv: i32,
    alpha: i32,
    value_weight: i32,
    mobility_weight: i32,
    tt_presence_weight: i32,
    search: &mut SearchContext,
) {
    assign_ordering_scores_weighted_window(
        board,
        move_list,
        lv,
        cmp::max(-alpha - 6, -SCORE_MAX),
        cmp::min(-alpha + 16, SCORE_MAX),
        value_weight,
        mobility_weight,
        tt_presence_weight,
        search,
    );
}

#[inline(always)]
pub fn assign_ordering_scores_weighted_window(
    board: &Board,
    move_list: &mut [MoveBoard],
    lv: i32,
    eval_alpha: i32,
    eval_beta: i32,
    value_weight: i32,
    mobility_weight: i32,
    tt_presence_weight: i32,
    search: &mut SearchContext,
) {
    for ml in move_list.iter_mut() {
        if ml.is_skip {
            continue;
        }
        let move_board = board.make_move_from_flip_bit(1 << ml.move_num, ml.flip_bit);
        if lv > 2 && tt_presence_weight != 0 && search.tt.probe(&move_board).value().is_some() {
            ml.score += tt_presence_weight;
        }
        let search_eval = if lv < 1 {
            search.stats.eval_search_nodes += 1;
            if search.check_abort() {
                return;
            }
            search.stats.eval_search_leaf_nodes += 1;
            -search
                .ordering_evaluator
                .evaluate_move(board, 1 << ml.move_num, ml.flip_bit)
        } else {
            -negaalpha_eval_ordering(&move_board, eval_alpha, eval_beta, lv - 1, search)
        };
        let opp_moves = move_board.moves();
        let mobility_score =
            -(opp_moves.count_ones() as i32) * 2 - ((opp_moves & CORNER_MASK).count_ones() as i32);
        ml.score += search_eval * value_weight + mobility_score * mobility_weight;
    }
}

/// `score` 降順に `move_list` を安定でなく部分ソートする。
///
/// 上位 `TOP_N` 手だけを sorted にし、残りは未ソートのまま。
#[inline(always)]
pub fn sort_move_list(move_list: &mut [MoveBoard]) {
    const TOP_N: usize = 7;
    let n = move_list.len();
    if n <= TOP_N {
        move_list.sort_unstable_by_key(|mb| std::cmp::Reverse(mb.score));
    } else {
        move_list.select_nth_unstable_by_key(TOP_N - 1, |mb| std::cmp::Reverse(mb.score));
        move_list[..TOP_N].sort_unstable_by_key(|mb| std::cmp::Reverse(mb.score));
    }
}

/// 立っているビットを最下位から順に取り出すだけの iterator。
// Basic bit iterator kept for search experiments and parity iterator comparisons.
#[allow(dead_code)]
pub struct MoveIterator {
    bits: u64,
}

impl MoveIterator {
    // Constructor retained with MoveIterator for search experiments.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn new(bits: u64) -> Self {
        Self { bits }
    }
}

impl Iterator for MoveIterator {
    type Item = u64;

    #[inline(always)]
    fn next(&mut self) -> Option<u64> {
        if self.bits == 0 {
            None
        } else {
            let lsb = self.bits & self.bits.wrapping_neg();
            self.bits &= self.bits - 1;
            Some(lsb)
        }
    }
}

/// 4 つの 4×4 象限のパリティ + コーナーで move ordering する iterator。
///
/// 列挙順:
/// 1. 奇数パリティ象限のコーナー
/// 2. 奇数パリティ象限の非コーナー
/// 3. 偶数パリティ象限のコーナー
/// 4. 偶数パリティ象限の非コーナー
pub struct MoveIteratorParity {
    corner_odd: u64,
    odd: u64,
    corner_even: u64,
    even: u64,
}

const QUADRANT_MASKS: [u64; 4] = [
    0x0000_0000_0f0f_0f0f,
    0x0000_0000_f0f0_f0f0,
    0xf0f0_f0f0_0000_0000,
    0x0f0f_0f0f_0000_0000,
];

impl MoveIteratorParity {
    pub fn new(legal_moves: u64, board: &Board) -> Self {
        let empties = !(board.player | board.opponent);

        let corner_moves = legal_moves & CORNER_MASK;
        let other_moves = legal_moves & !CORNER_MASK;

        let mut corner_odd = 0u64;
        let mut odd = 0u64;
        let mut corner_even = 0u64;
        let mut even = 0u64;

        for &mask in &QUADRANT_MASKS {
            if legal_moves & mask == 0 {
                continue;
            }
            if (empties & mask).count_ones() & 1 == 1 {
                corner_odd |= corner_moves & mask;
                odd |= other_moves & mask;
            } else {
                corner_even |= corner_moves & mask;
                even |= other_moves & mask;
            }
        }

        Self {
            corner_odd,
            odd,
            corner_even,
            even,
        }
    }
}

impl Iterator for MoveIteratorParity {
    type Item = u64;

    #[inline(always)]
    fn next(&mut self) -> Option<u64> {
        for bucket in [
            &mut self.corner_odd,
            &mut self.odd,
            &mut self.corner_even,
            &mut self.even,
        ] {
            if *bucket != 0 {
                let lsb = *bucket & bucket.wrapping_neg();
                *bucket &= *bucket - 1;
                return Some(lsb);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::evaluator::evaluator_from_data;
    use crate::eval::{default_evaluator, Evaluator};
    use crate::file::{EvaluatorData, PatternEvaluatorData};
    use crate::search::mpc::MpcConfig;
    use crate::search::search::SearchStats;
    use crate::t_table::TranspositionTable;
    use std::sync::Arc;

    fn bias_evaluator(raw_bias: i16) -> Arc<dyn Evaluator> {
        let mut data = PatternEvaluatorData::default();
        for phase in &mut data.phases {
            phase.bias = raw_bias;
        }
        evaluator_from_data(EvaluatorData::Pattern(data)).unwrap()
    }

    fn search_with_ordering<'a>(
        stats: &'a mut SearchStats,
        ordering: Arc<dyn Evaluator>,
    ) -> SearchContext<'a> {
        SearchContext::new(
            default_evaluator(),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
            stats,
        )
        .with_ordering_evaluator(ordering)
    }

    #[test]
    fn assign_ordering_scores_lv_zero_uses_ordering_static_eval() {
        let board = Board::new();
        let mut moves = make_move_list(&board, board.moves());
        let ordering = bias_evaluator(1280);
        let mut stats = SearchStats::default();
        let mut search = search_with_ordering(&mut stats, ordering.clone());

        assign_ordering_scores(&board, &mut moves, 0, 0, &mut search);

        for mv in moves {
            let child = board.make_move_from_flip_bit(1 << mv.move_num, mv.flip_bit);
            let opp_moves = child.moves();
            let mobility_score = -(opp_moves.count_ones() as i32) * 2
                - ((opp_moves & CORNER_MASK).count_ones() as i32);
            assert_eq!(mv.score, -ordering.evaluate(&child) + mobility_score);
        }
    }

    #[test]
    fn assign_ordering_scores_lv_two_uses_shallow_search_value() {
        let board = Board::new();
        let mut static_moves = make_move_list(&board, board.moves());
        let mut search_moves = static_moves.clone();
        let ordering = bias_evaluator(1280);

        let mut static_stats = SearchStats::default();
        let mut static_search = search_with_ordering(&mut static_stats, ordering.clone());
        assign_ordering_scores(&board, &mut static_moves, 0, 0, &mut static_search);

        let mut search_stats = SearchStats::default();
        let mut search = search_with_ordering(&mut search_stats, ordering);
        assign_ordering_scores(&board, &mut search_moves, 2, 0, &mut search);

        assert!(search_stats.eval_search_nodes > 0);
        for (static_mv, search_mv) in static_moves.iter().zip(search_moves.iter()) {
            assert_eq!(search_mv.score - static_mv.score, 20);
        }
    }
}
