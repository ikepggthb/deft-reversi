use std::mem;

use crate::board::*;
use crate::count_last_flip::count_last_flip;
use crate::cut_off::*;

use crate::evaluator_const::SCORE_MAX;
use crate::move_list::*;
use crate::eval::evaluator_const::SCORE_INF;

use crate::solver::SearchEngine;
use crate::mpc::*;
use crate::t_table::N_TT_MOVES;

use crate::eval_search::{negaalpha_eval, negaalpha_eval_no_mo};
/// 空きマスが残り`SWITCH_EMPTIES_SIMPLE_NWS`以下である場合、
/// `nws_perfect`から、`nws_perfect_simple`へ切り替える
const SWITCH_EMPTIES_SIMPLE_NWS: i32 = 10;

/// 空きマスが残り`SWITCH_EMPTIES_NEGA_ALPHA`以下である場合、
/// `pvs_perfect_simple`や`nws_perfect_simple`から、`negaalpha_perfect`へ切り替える
const SWITCH_EMPTIES_NEGA_ALPHA: i32 = 5;


const MC: u64 = 0b1000000100000000000000000000000000000000000000000000000010000001_u64;
const MX: u64 = 0b0000000001000010000000000000000000000000000000000100001000000000_u64;



// evaluation of move ordering



/// オセロの盤面に基づいて最終スコアを計算
///
/// この関数は、現在のプレイヤーと対戦相手の石の数に基づいて、
/// ゲーム終了時のスコアを計算する。空きマスがある場合は、それらを勝っている側の
/// スコアに加算する。
///
/// # 引数
/// * `board` - スコアを計算するオセロの盤面を表す `Board` オブジェクトの参照。
///
/// # 戻り値
/// * 計算されたゲームの最終スコアを表す整数値。
///
/// # 例
/// ```ignore
/// let board = Board::new(); // ゲーム終了時の盤面を生成
/// let score = solve_score(&board);
/// println!("Final Score: {}", score);
/// ```
///
/// # 注記
/// * スコアは、現在のプレイヤーの石の数から対戦相手の石の数を引いた値である。
/// * 空きマスが存在する場合、それらを勝っている側のスコアに加算する。
#[inline(always)]
pub fn solve_score(board: &Board) -> i32 {
    let n_player: i32 = board.player.count_ones() as i32;
    let n_opponent: i32 = board.opponent.count_ones() as i32;
    let diff: i32 = n_player - n_opponent;

    // https://github.com/rust-lang/rust-clippy/issues/5354
    // 速度重視のため、match文ではなく、if文を使用
    #[allow(clippy::comparison_chain)]
    if diff > 0 {
        let n_empties: i32 = 64 - n_player - n_opponent;
        diff + n_empties
    } else if diff < 0 {
        let n_empties: i32 = 64 - n_player - n_opponent;
        diff - n_empties
    } else {
        0
    }
}

/// 空きマスがないオセロ盤面の最終スコアを高速に計算
///
/// # 引数
/// * `board` - スコアを計算するオセロの盤面(`Board` オブジェクトの参照)。
///
/// # 戻り値
/// * 最終スコアを表す整数値。
///
/// # 例
/// ```ignore
/// let board = Board::new(); // 空きマスがないゲーム終了時の盤面を生成
/// let score = solve_score_0_empties(&board);
/// println!("Final Score: {}", score);
/// ```
///
/// # 注記
/// * この関数は、盤面上に空きマスがない場合にのみ正確なスコアを返します。
/// * スコアは、現在のプレイヤーの石の数から対戦相手の石の数を引いた値だが、
///   盤面上に空きマスがないことから、現在のプレイヤーの石の数の2倍から64を引いた値を用いることで高速化している。
#[inline(always)]
pub fn solve_score_0_empties(board: &Board) -> i32 {
    #[cfg(debug_assertions)]
    {
        assert_eq!((board.player | board.opponent), u64::MAX);
        assert_eq!(
            2 * (board.player.count_ones() as i32) - 64,
            solve_score(board)
        );
    }

    2 * (board.player.count_ones() as i32) - 64
}

#[inline(always)]
pub fn solve_score_1_empties(board_player: u64, alpha: i32, x: usize) -> i32 {
    let n_flips = count_last_flip(x, board_player);
    let mut score = 2 * board_player.count_ones() as i32 - 64 + 2 + n_flips;

    if n_flips == 0 {
        let n_flips = count_last_flip(x, !board_player);
        if n_flips != 0 {
            score -= n_flips + 2;
        }
    }
    // * use lazy cut-off :
    // if n_flips == 0 {
    //     if score <= 0 {
    //         score -= 2;
    //         if score > alpha {
    //             // lazy cut-off
    //             let n_flips = count_last_flip(x, !board_player);
    //             score -= n_flips;
    //         }
    //     } else if score > alpha {
    //         // lazy cut-off
    //         let n_flips = count_last_flip(x, !board_player);
    //         if n_flips != 0 {
    //             score -= n_flips + 2;
    //         }
    //     }
    // }
    score
}

// https://eukaryote.hateblo.jp/entry/2020/04/26/031246
const NEIGHBOUR: [u64; 66] = [
    0x0000000000000302,
    0x0000000000000604,
    0x0000000000000e0a,
    0x0000000000001c14,
    0x0000000000003828,
    0x0000000000007050,
    0x0000000000006020,
    0x000000000000c040,
    0x0000000000030200,
    0x0000000000060400,
    0x00000000000e0a00,
    0x00000000001c1400,
    0x0000000000382800,
    0x0000000000705000,
    0x0000000000602000,
    0x0000000000c04000,
    0x0000000003020300,
    0x0000000006040600,
    0x000000000e0a0e00,
    0x000000001c141c00,
    0x0000000038283800,
    0x0000000070507000,
    0x0000000060206000,
    0x00000000c040c000,
    0x0000000302030000,
    0x0000000604060000,
    0x0000000e0a0e0000,
    0x0000001c141c0000,
    0x0000003828380000,
    0x0000007050700000,
    0x0000006020600000,
    0x000000c040c00000,
    0x0000030203000000,
    0x0000060406000000,
    0x00000e0a0e000000,
    0x00001c141c000000,
    0x0000382838000000,
    0x0000705070000000,
    0x0000602060000000,
    0x0000c040c0000000,
    0x0003020300000000,
    0x0006040600000000,
    0x000e0a0e00000000,
    0x001c141c00000000,
    0x0038283800000000,
    0x0070507000000000,
    0x0060206000000000,
    0x00c040c000000000,
    0x0002030000000000,
    0x0004060000000000,
    0x000a0e0000000000,
    0x00141c0000000000,
    0x0028380000000000,
    0x0050700000000000,
    0x0020600000000000,
    0x0040c00000000000,
    0x0203000000000000,
    0x0406000000000000,
    0x0a0e000000000000,
    0x141c000000000000,
    0x2838000000000000,
    0x5070000000000000,
    0x2060000000000000,
    0x40c0000000000000,
    0,
    0,
];

#[inline(always)]
pub fn solve_score_2_empties(board: &Board, alpha: i32, beta: i32, search: &mut SearchEngine) -> i32 {
    search.status.perfect_search_node_count += 1;
    let empties = !(board.player | board.opponent);

    let first = empties & (!empties + 1);
    let second = empties & (empties - 1);

    let mut score = alpha;
    let mut no_move = true;

    if (NEIGHBOUR[first.trailing_zeros() as usize] & board.opponent) != 0 {
        let flip = board.flip_bit(first);
        if flip != 0 {
            no_move = false;
            search.status.perfect_search_node_count += 2;
            search.status.perfect_search_leaf_node_count += 1;
            let first_score = -solve_score_1_empties(
                board.opponent ^ flip,
                -beta,
                second.trailing_zeros() as usize,
            );
            if first_score >= beta {
                return first_score;
            }
            if first_score > score {
                score = first_score;
            }
        }
    }

    if (NEIGHBOUR[second.trailing_zeros() as usize] & board.opponent) != 0 {
        let flip = board.flip_bit(second);
        if flip != 0 {
            no_move = false;
            search.status.perfect_search_node_count += 2;
            search.status.perfect_search_leaf_node_count += 1;
            let second_score = -solve_score_1_empties(
                board.opponent ^ flip,
                -beta,
                first.trailing_zeros() as usize,
            );
            if second_score > score {
                score = second_score;
            }
        }
    }

    if no_move {
        if board.opponent_moves() == 0 {
            search.status.perfect_search_leaf_node_count += 1;
            return solve_score(board);
        } else {
            return -solve_score_2_empties(&board.swapped_board(), -beta, -alpha, search);
        }
    }
    score
}

/// NegaAlpha法を用いて、完全読みを行い、オセロの盤面のスコアを計算する。
///
/// 探索速度を向上させるため、葉に近いノードで使用される。
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha` - 探索の下限値を示すアルファ値。
/// * `beta` - 探索の上限値を示すベータ値。
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへのミュータブルな参照。
///
/// # 戻り値
/// * 探索結果として計算された盤面のスコアを表す整数値。
///   スコアは現在のプレイヤーから見た盤面のスコアを表す。
///
pub fn negaalpha_perfect(board: &Board, mut alpha: i32, beta: i32, search: &mut SearchEngine) -> i32 {
    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    search.status.perfect_search_node_count += 1;

    // 空きマスが残り2のとき
    let n_empties = board.empties_count();
    // if (board.player | board.opponent).count_zeros() == 2 {
    //     return solve_score_2_empties(board, alpha, beta, search);
    // }

    let legal_moves = board.moves();

    // 合法手がない
    if legal_moves == 0 {
        if board.opponent_moves() == 0 {
            // passしても置くところがない == ゲーム終了
            search.status.perfect_search_leaf_node_count += 1;
            return solve_score(board);
        }
        let passed_board = {
            let mut b = board.clone();
            b.swap();
            b
        };
        return -negaalpha_perfect(&passed_board, -beta, -alpha, search);
    }

    // 探索範囲: [alpha, beta]
    let mut best_score: i32 = -SCORE_INF;

    let move_iter = MoveIteratorParity::new(legal_moves, board);
    // let move_iter = MoveIterator::new(legal_moves);
    for legal_move in move_iter {
        let mut current_board = board.clone();
        current_board.put_piece_fast(legal_move);
        let score: i32 = if n_empties - 1 == 2 {
            -solve_score_2_empties(&current_board, -beta, -alpha, search)
        } else {
            -negaalpha_perfect(&current_board, -beta, -alpha, search)
        };
        if score >= beta {
            return score;
        }
        if score > alpha {
            alpha = score;
        }
        if score > best_score {
            best_score = score
        }
    }

    best_score
}

/// 関数`pvs_perfect_simple`で用いられるヌルウィンドウ探索（Null Window Search, NWS）
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha` - 探索の下限値を示すアルファ値。
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへのミュータブルな参照。
///
/// # 戻り値
/// * 探索結果として計算された盤面のスコアを表す整数値。
///   スコアは現在のプレイヤーから見た盤面のスコアを表す。
///
/// # 注記
/// * 終盤の局面では、`negaalpha_perfect` 関数に切り替わります。
pub fn nws_perfect_simple(board: &Board, mut alpha: i32, search: &mut SearchEngine) -> i32 {
    // 探索範囲: [alpha, beta]
    let beta: i32 = alpha + 1;

    let n_empties = board.empties_count();
    if n_empties < SWITCH_EMPTIES_NEGA_ALPHA {
        return negaalpha_perfect(board, alpha, beta, search);
    }

    search.status.perfect_search_node_count += 1;

    let moves_bit: u64 = board.moves();

    if moves_bit == 0 {
        if board.opponent_moves() == 0 {
            // passしても置くところがない == ゲーム終了
            search.status.perfect_search_leaf_node_count += 1;
            
            return solve_score(&board);
        }        
        return -nws_perfect_simple(&board.swapped_board(), -beta, search);
    }

    match perfect_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => (),
    }

    let mut move_list: [MoveBoard; MOVE_MAX] = unsafe { mem::uninitialized() };

    // gen move list
    let move_list_len = moves_bit.count_ones() as i32;
    let move_list = &mut move_list[..(move_list_len as usize)];
    set_move_list(board, moves_bit, move_list);

    // move ordering
    if move_list_len >= 2 {
        set_move_eval_ffs(move_list);
        sort_move_list(move_list);
    }

    let mut best_score: i32 = -SCORE_INF;
    for move_board in move_list.iter_mut() {
        let current_put_board = &move_board.board;
        let score: i32 = -nws_perfect_simple(current_put_board, -beta, search);
        if score >= beta {
            return score;
        }
        if score > alpha {
            alpha = score;
        }
        if score > best_score {
            best_score = score;
        }
    }

    best_score
}

/// Principal Variation Search (PVS) を用いて、完全読みを行い、オセロの盤面のスコアを計算する。
///
/// `pvs_perfect`とは異なり、探索速度を優先するため、置換表を使用しない。
/// 浅い探索で用いられる。
/// 現在は使われていない
///
///  PVS(Negascout)について :
///   https://ja.wikipedia.org/wiki/Negascout
///
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha` - 探索の下限値を示すアルファ値。
/// * `beta` - 探索の上限値を示すベータ値。
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへのミュータブルな参照。
///
/// # 戻り値
/// * 探索結果として計算された盤面のスコアを表す整数値。
///   スコアは現在のプレイヤーから見た盤面のスコアを表す。
// pub fn pvs_perfect_simple(board: &Board, alpha: i32, beta: i32, search: &mut Search) -> i32 {
//     #[cfg(debug_assertions)]
//     assert!(alpha <= beta);

//     if board.empties_count() < SWITCH_EMPTIES_NEGA_ALPHA {
//         return negaalpha_perfect(board, alpha, beta, search);
//     }

//     search.status.perfect_search_node_count += 1;

//     // 探索範囲: [alpha, beta]
//     let moves_bit: u64 = board.moves();

//     if moves_bit == 0 {
//         if board.opponent_moves() == 0 {
//             search.status.perfect_search_leaf_node_count += 1;
//             return solve_score(board);
//         } else {
//             let mut board: Board = board.clone();
//             board.swap();
//             return -pvs_perfect_simple(&board, -beta, -alpha, search);
//         }
//     }

//     match perfect_search_mpc(board, alpha, beta, search) {
//         ProbCutResult::Cut(score) => return score,
//         ProbCutResult::Fail => (),
//     }

//     let mut move_list: [MoveBoard; MOVE_MAX] = unsafe { mem::uninitialized() };
//     // set_move_list(board, legal_moves, &mut move_list);
//     let move_list_len = moves_bit.count_ones() as i32;
//     let move_list = &mut move_list[..(move_list_len as usize)];
//     set_move_list(board, moves_bit, move_list);

//     // move ordering
//     if move_list_len >= 2 {
//         set_move_eval(move_list, 0, search);
//         sort_move_list(move_list);
//     }

//     let mut this_node_alpha: i32 = alpha;
//     let mut best_score: i32;

//     let mut move_list_iter = move_list.iter();

//     // first move
//     let first_move = move_list_iter.next().unwrap();
//     best_score = -pvs_perfect_simple(&first_move.board, -beta, -this_node_alpha, search);
//     if best_score >= beta {
//         return best_score;
//     }
//     if best_score > this_node_alpha {
//         this_node_alpha = best_score;
//     }

//     // other move
//     for other_move in move_list_iter {
//         let board: &Board = &other_move.board;
//         let mut score: i32 = -nws_perfect_simple(board, -this_node_alpha - 1, search);
//         if score >= beta {
//             return score;
//         }
//         if best_score < score {
//             score = -pvs_perfect_simple(board, -beta, -this_node_alpha, search);
//             if beta <= score {
//                 return score;
//             }
//             if best_score > this_node_alpha {
//                 this_node_alpha = best_score
//             }
//             best_score = score;
//         }
//         if score > this_node_alpha {
//             this_node_alpha = score
//         }
//     }

//     best_score
// }

/// # 関数 `pvs_perfect`で用いられるヌルウィンドウ探索（Null Window Search, NWS）
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha` - 探索の下限値を示すアルファ値。
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへのミュータブルな参照。
///
/// # 戻り値
/// * 探索結果として計算された盤面のスコアを表す整数値。
///   スコアは現在のプレイヤーから見た盤面の評価値を表す。
///
/// # 注記
/// * 終盤の局面では、`negaalpha_perfect` 関数に切り替わります。

pub fn nws_perfect(board: &Board, mut alpha: i32, search: &mut SearchEngine) -> i32 {
    let mut beta = alpha + 1;

    let n_empties: i32 = board.empties_count();
    if n_empties < SWITCH_EMPTIES_SIMPLE_NWS {
        return nws_perfect_simple(board, alpha, search);
    }

    search.status.perfect_search_node_count += 1;

    // 探索範囲: [alpha, beta]
    let mut moves_bit: u64 = board.moves();

    if moves_bit == 0 {
        if board.opponent_moves() == 0 {
            search.status.perfect_search_leaf_node_count += 1;
            return solve_score(board);
        } else {
            let passed_board: Board = {
                let mut b: Board = board.clone();
                b.swap();
                b
            };
            return -nws_perfect(&passed_board, -beta, search);
        }
    }

    let td = search.t_table.get(board);
    let tt_moves: Option<[u8; 2]> = {
        match td {
            Some(t) if n_empties >= 14 => {
                let f = t.moves[0];
                let s = t.moves[1];
                if f != NO_COORD {
                            moves_bit &= !(1u64 << f);
                        }
                if s != NO_COORD {
                            moves_bit &= !(1u64 << s);
                        }
                Some([f, s])
            }
            None | Some(_) => None,
        }
    };

    if let Some(score) = t_table_cut_off_td(&mut alpha, &mut beta, 60, search.selectivity_lv, &td) {
        return score;
    }

    match perfect_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => (),
    }

    let mut tt_move_list: Option<[MoveBoard; N_TT_MOVES]> = match tt_moves {
        Some(tt_moves) => {
            let tt_move_list: [MoveBoard; N_TT_MOVES] = gen_tt_move_list(board, &tt_moves);
            Some(tt_move_list)
        }
        None => None,
    };

    let mut move_list: [MoveBoard; MOVE_MAX] = unsafe { mem::uninitialized() };
    // set_move_list(board, legal_moves, &mut move_list);
    let move_list_len = moves_bit.count_ones() as i32;
    let move_list = &mut move_list[..(move_list_len as usize)];
    set_move_list(board, moves_bit, move_list);
    let mut n_skip = 0;

    if n_empties > 12 {
        if let Some(tt_move_list) = tt_move_list.as_mut() {
            let mut n_tt_skip = 0;
            if let Some(score) = et_cut_off(
                &mut alpha,
                &mut beta,
                tt_move_list,
                60,
                search.selectivity_lv,
                &mut n_tt_skip,
                &search.t_table,
            ) {
                return score;
            }
        }
        if let Some(score) = et_cut_off(
            &mut alpha,
            &mut beta,
            move_list,
            60,
            search.selectivity_lv,
            &mut n_skip,
            &search.t_table,
        ) {
            return score;
        }
    }

    let mut best_move: u8 = NO_COORD;
    let mut this_node_alpha: i32 = alpha;
    let mut best_score: i32 = -SCORE_INF;

    if let Some(tt_move_list) = tt_move_list.as_mut() {
        for move_board in tt_move_list.iter_mut() {
            if move_board.skip {
                continue;
            }
            let score: i32 = -nws_perfect(&move_board.board, -beta, search);
            if score >= beta {
                search.t_table.add(
                    board,
                    score,
                    SCORE_INF,
                    60,
                    search.selectivity_lv,
                    move_board.put_place,
                );
                return score;
            }
            if score > this_node_alpha {
                this_node_alpha = score;
            }
            if score > best_score {
                best_score = score;
                best_move = move_board.put_place;
            }
        }
    }

    // move ordering
    if move_list_len - n_skip >= 2 {

        /* 
        move ordering evalution 
        - legal moves
        - evalation of shallow search
        */
        {
            let lv = {
                match n_empties {
                    15..17 => 1,
                    17..20 => 2,
                    20..24 => 2,
                    24..60 => n_empties / 3,
                    _ => 0,
                }
            };
            let alpha = std::cmp::max(alpha - 6, -SCORE_MAX);
            let beta = std::cmp::min(alpha + 12, SCORE_MAX);
            for move_board in move_list.iter_mut() {
                if move_board.skip {
                    continue;
                }
                if move_board.board.moves().count_ones() as i32 <= 1  && n_empties < 16 {
                    move_board.eval += 160;
                }
                // let node_count_tmp = search.status.eval_search_node_count;
                let search_eval = match lv - 1 {
                    0 => -search.eval_func.clac_features_eval(&move_board.board),
                    1 | 2 => -negaalpha_eval_no_mo(&move_board.board, alpha, beta, lv - 1, search),
                    3..=60 => -negaalpha_eval(&move_board.board, alpha, beta, lv - 1, search),
                    _ => 0
                };
                // let node_count = search.status.eval_search_node_count - node_count_tmp;

                let opponent_mobility_score = {
                    let moves_bit = move_board.board.moves();
                    let n_move = -(moves_bit.count_ones() as i32);
                    let n_coner_moves = -((moves_bit & MC).count_ones() as i32);
                    n_move * 2 + n_coner_moves
                };
                move_board.eval += search_eval + opponent_mobility_score * 2;

            }
        };

        sort_move_list(move_list);
    }

    for move_board in move_list.iter_mut() {
        if move_board.skip {
            continue;
        }
        let score: i32 = -nws_perfect(&move_board.board, -beta, search);
        if score >= beta {
            search.t_table.add(
                board,
                score,
                SCORE_INF,
                60,
                search.selectivity_lv,
                move_board.put_place,
            );
            return score;
        }
        if score > this_node_alpha {
            this_node_alpha = score;
        }
        if score > best_score {
            best_score = score;
            best_move = move_board.put_place;
        }
    }
    if best_move == NO_COORD {
        return -SCORE_INF;
    }

    if best_score > alpha {
        search.t_table.add(
            board,
            best_score,
            best_score,
            60,
            search.selectivity_lv,
            best_move,
        );
    } else {
        search.t_table.add(
            board,
            -SCORE_INF,
            best_score,
            60,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}

/// Performs a Principal Variation Search (PVS) to evaluate the board state.
///
/// This function implements the PVS algorithm to efficiently search and evaluate
/// possible moves in the game. It uses various optimizations such as transposition
/// table lookups, enhanced transposition cutoffs, and move ordering to reduce
/// the search space and improve performance.
///
/// # Parameters
/// - `board`: The current game board.
/// - `alpha`: The alpha value for alpha-beta pruning (lower bound of the search window).
/// - `beta`: The beta value for alpha-beta pruning (upper bound of the search window).
/// - `search`: The search context containing search state and transposition table.
///
/// # Returns
/// The best score for the current player given the board state and the search window.
pub fn pvs_perfect(board: &Board, mut alpha: i32, mut beta: i32, search: &mut SearchEngine) -> i32 {
    let n_empties = board.empties_count();
    if n_empties < SWITCH_EMPTIES_NEGA_ALPHA {
        return negaalpha_perfect(board, alpha, beta, search);
    }
    // println!("{}, {}", alpha, beta);
    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    search.status.perfect_search_node_count += 1;

    // 探索範囲: [alpha, beta]
    let mut moves_bit: u64 = board.moves();

    // pass or end ?
    if moves_bit == 0 {
        // 合法手がないならば
        if board.opponent_moves() == 0 {
            search.status.perfect_search_leaf_node_count += 1;
            return solve_score(board);
        }

        // 合法手がある -> 探索を続ける
        let passed_board: Board = {
            let mut b: Board = board.clone();
            b.swap();
            b
        };
        return -pvs_perfect(&passed_board, -beta, -alpha, search);
    }

    let td = search.t_table.get(board);

    let tt_moves = {
        if let Some(t) = td {
            let f = t.moves[0];
            let s = t.moves[1];
            if f != NO_COORD {
                moves_bit &= !(1u64 << f);
            }
            if s != NO_COORD {
                moves_bit &= !(1u64 << s);
            }
            Some([f, s])
        } else {
            None
        }
    };

    // TranspositionTable Cut off
    if let Some(score) = t_table_cut_off_td(&mut alpha, &mut beta, 60, search.selectivity_lv, &td) {
        return score;
    }

    // Multi prub cut
    match perfect_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => (),
    }

    let mut tt_move_list: Option<[MoveBoard; N_TT_MOVES]> = match tt_moves {
        Some(tt_moves) => {
            let tt_move_list: [MoveBoard; N_TT_MOVES] = gen_tt_move_list(board, &tt_moves);
            Some(tt_move_list)
        }
        None => None,
    };

    let mut move_list: [MoveBoard; MOVE_MAX] = unsafe { mem::uninitialized() };
    // set_move_list(board, legal_moves, &mut move_list);
    let move_list_len = moves_bit.count_ones() as i32;
    let move_list = &mut move_list[..(move_list_len as usize)];
    set_move_list(board, moves_bit, move_list);
    let mut n_skip = 0;

    if n_empties > 12 {
        if let Some(tt_move_list) = tt_move_list.as_mut() {
            let mut n_tt_skip = 0;
            if let Some(score) = et_cut_off(
                &mut alpha,
                &mut beta,
                tt_move_list,
                60,
                search.selectivity_lv,
                &mut n_tt_skip,
                &search.t_table,
            ) {
                return score;
            }
        }

        if let Some(score) = et_cut_off(
            &mut alpha,
            &mut beta,
            move_list,
            60,
            search.selectivity_lv,
            &mut n_skip,
            &search.t_table,
        ) {
            return score;
        }
    }

    let mut best_move: u8 = NO_COORD;
    let mut this_node_alpha: i32 = alpha;
    let mut best_score: i32 = -SCORE_INF;
    let mut pvs_ok = false;

    if let Some(tt_move_list) = tt_move_list.as_mut() {
        for move_board in tt_move_list.iter_mut() {
            if move_board.skip {
                continue;
            }
            if !pvs_ok {
                let score: i32 = -pvs_perfect(&move_board.board, -beta, -alpha, search);
                if score >= beta {
                    search.t_table.add(
                        board,
                        score,
                        SCORE_INF,
                        60,
                        search.selectivity_lv,
                        move_board.put_place,
                    );
                    return score;
                }
                if score > this_node_alpha {
                    this_node_alpha = score;
                }
                if score > best_score {
                    best_score = score;
                    best_move = move_board.put_place;
                }
                pvs_ok = true;
            } else {
                let mut score: i32 = -nws_perfect(&move_board.board, -this_node_alpha - 1, search);
                if score >= beta {
                    search.t_table.add(
                        board,
                        score,
                        SCORE_INF,
                        60,
                        search.selectivity_lv,
                        move_board.put_place,
                    );
                    return score;
                }
                if score > best_score {
                    // 再探索
                    score = -pvs_perfect(&move_board.board, -beta, -this_node_alpha, search);
                    if score >= beta {
                        search.t_table.add(
                            board,
                            score,
                            SCORE_INF,
                            60,
                            search.selectivity_lv,
                            move_board.put_place,
                        );
                        return score;
                    }
                    best_move = move_board.put_place;
                    best_score = score;
                    if score > this_node_alpha {
                        this_node_alpha = score;
                    }
                }
            }
        }
    }
    // move ordering
    if move_list_len - n_skip >= 2 {
        {
            let lv = {
                match n_empties {
                    13..16 => 1,
                    16..24 => 2,
                    24..60 => n_empties / 3,
                    _ => 0,
                }
            };
            let alpha = std::cmp::max(alpha - 6, -SCORE_MAX);
            let beta = std::cmp::min(alpha + 12, SCORE_MAX);
            let (mut max_eval, mut max_eval_index) = (-SCORE_MAX, 0);

            for (i, move_board) in move_list.iter_mut().enumerate() {
                if move_board.skip {
                    continue;
                }
                // let node_count_tmp = search.status.eval_search_node_count;
                let search_eval = if lv > 0 {-negaalpha_eval(&move_board.board, -beta, -alpha, lv - 1, search)} else {0};
                // let node_count = search.status.eval_search_node_count - node_count_tmp;

                if search_eval > max_eval {
                    (max_eval, max_eval_index) = (search_eval, i);
                }

                let opponent_mobility_score = {
                    let moves_bit = move_board.board.moves();
                    let n_move = -(moves_bit.count_ones() as i32);
                    let n_coner_moves = -((moves_bit & MC).count_ones() as i32);
                    // let emc = 40 - (em + ec);
                    n_move * 2 + n_coner_moves
                };
                move_board.eval += search_eval + opponent_mobility_score;

            }

            if !pvs_ok {
                move_list[max_eval_index].eval += 1000;
            }
        }


        
        sort_move_list(move_list);
    }

    if !pvs_ok {
        for move_board in move_list.iter_mut() {
            if move_board.skip {
                continue;
            }
            best_move = move_board.put_place;
            best_score = -pvs_perfect(&move_board.board, -beta, -this_node_alpha, search);
            if best_score >= beta {
                search.t_table.add(
                    board,
                    best_score,
                    SCORE_INF,
                    60,
                    search.selectivity_lv,
                    move_board.put_place,
                );
                return best_score;
            }
            this_node_alpha = this_node_alpha.max(best_score);
            move_board.skip = true;
            break;
        }
    }

    // other move
    for move_board in move_list.iter() {
        if move_board.skip {
            continue;
        }
        let mut score: i32 = -nws_perfect(&move_board.board, -this_node_alpha - 1, search);
        if score >= beta {
            search.t_table.add(
                board,
                score,
                SCORE_INF,
                60,
                search.selectivity_lv,
                move_board.put_place,
            );
            return score;
        }
        if score > best_score {
            // 再探索
            score = -pvs_perfect(&move_board.board, -beta, -this_node_alpha, search);
            if score >= beta {
                search.t_table.add(
                    board,
                    score,
                    SCORE_INF,
                    60,
                    search.selectivity_lv,
                    move_board.put_place,
                );
                return score;
            }
            best_move = move_board.put_place;
            best_score = score;
            if score > this_node_alpha {
                this_node_alpha = score;
            }
        }
    }

    if best_move == NO_COORD {
        return -SCORE_INF;
    }
    if best_score > alpha {
        // alpha < best_score < beta
        search.t_table.add(
            board,
            best_score,
            best_score,
            60,
            search.selectivity_lv,
            best_move,
        );
    } else {
        // best_score <= alpha
        search.t_table.add(
            board,
            -SCORE_INF,
            best_score,
            60,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}
