use std::mem;

use crate::board::*;
use crate::perfect_search::solve_score;
use crate::search::*;

use crate::mpc::*;
use crate::N_TT_MOVES;

// TranspositionTableでは、評価値をi8で管理している
const SCORE_INF: i32 = i8::MAX as i32;

const MOVE_ORDERING_EVAL_LEVEL: i32 = 2;
const MOVE_ORDERING_EVAL_LEVEL_SIMPLE_SEARCH: i32 = 1;
const SWITCH_SIMPLE_SEARCH_LEVEL: i32 = 6;
const SWITCH_NEGAALPHA_SEARCH_LEVEL: i32 = 4;

pub fn negaalpha_eval_no_mo(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    lv: i32,
    search: &mut Search,
) -> i32 {
    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    search.eval_search_node_count += 1;
    if lv <= 0 {
        search.eval_search_leaf_node_count += 1;
        return search.eval_func.clac_features_eval(board);
    }

    let legal_moves = board.moves();

    // 合法手がない
    if legal_moves == 0 {
        let mut board = board.clone();
        board.swap();
        if board.moves() == 0 {
            // passしても置くところがない == ゲーム終了
            search.eval_search_leaf_node_count += 1;
            board.swap();
            return solve_score(&board);
        }
        return -negaalpha_eval_no_mo(&board, -beta, -alpha, lv, search);
    }

    // 探索範囲: [alpha, beta]

    let mut best_score = -SCORE_INF;
    for l in MoveIterator::new(legal_moves) {
        let mut board = board.clone();
        board.put_piece_fast(l);
        let score = -negaalpha_eval_no_mo(&board, -beta, -alpha, lv - 1, search);
        if score >= beta {
            return score;
        }
        if score > alpha {
            alpha = score
        };
        if score > best_score {
            best_score = score
        };
    }

    best_score
}

/// NegaAlpha法を用いて、オセロの盤面の評価値を計算する。
///
/// 探索速度を向上させるため、葉に近いノードで使用される。
///
/// # 引数
/// * `board`  - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha`  - 探索の下限値を示すアルファ値。
/// * `beta`   - 探索の上限値を示すベータ値。
/// * `lv`     - 探索レベル (あと何手先まで読むか)
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへのミュータブルな参照。
///
/// # 戻り値
/// * 探索結果として計算された盤面のスコアを表す整数値。
///   スコアは現在のプレイヤーから見た盤面のスコアを表す。
///
pub fn negaalpha_eval(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    lv: i32,
    search: &mut Search,
) -> i32 {
    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    if lv <= 2 {
        return negaalpha_eval_no_mo(board, alpha, beta, lv, search);
    }

    search.eval_search_node_count += 1;
    let moves_bit = board.moves();

    // 合法手がない
    if moves_bit == 0 {
        let mut board = board.clone();
        board.swap();
        if board.moves() == 0 {
            // passしても置くところがない == ゲーム終了
            search.eval_search_leaf_node_count += 1;

            board.swap();
            return solve_score(&board);
        }
        return -negaalpha_eval(&board, -beta, -alpha, lv, search);
    }

    // 探索範囲: [alpha, beta]

    match eval_search_mpc(board, alpha, beta, lv, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => (),
    }

    let mut move_list: [MoveBoard; MOVE_MAX] = unsafe { std::mem::uninitialized() };

    // gen move list
    let move_list_len = moves_bit.count_ones() as i32;
    let move_list = &mut move_list[..(move_list_len as usize)];
    set_move_list(board, moves_bit, move_list);

    // move ordering
    if move_list_len >= 2 {
        set_move_eval(move_list, 0, alpha, search);
        sort_move_list(move_list);
    }

    let mut best_score = -SCORE_INF;
    for move_board in move_list.iter() {
        let score = -negaalpha_eval(&move_board.board, -beta, -alpha, lv - 1, search);
        if score >= beta {
            return score;
        }
        if score > alpha {
            alpha = score
        };
        if score > best_score {
            best_score = score
        };
    }

    best_score
}

/// 関数`pvs_perfect_simple`で用いられるヌルウィンドウ探索（Null Window Search, NWS）
///
/// `alpha`から、`alpha + 1`までの範囲で、alpha-beta探索を行う。
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha` - 探索の下限値を示すアルファ値。
/// * `lv`     - 探索レベル (あと何手先まで読むか)
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへの可変な参照。
///
/// # 戻り値
/// * 探索結果として計算された盤面の評価値を表す整数値。
///   現在のプレイヤーから見た盤面の評価値を表す。
///
/// # 注記
/// * 置換表を使用しない。
/// * 最後の残り数手は、`negaalpha_eval`関数を使用した探索結果を用いる。
///     * 最後の残り数手は、`SWITCH_NEGAALPHA_SEARCH_LEVEL`で定義される。
pub fn nws_eval_simple(board: &Board, alpha: i32, lv: i32, search: &mut Search) -> i32 {
    let beta = alpha + 1;

    if lv < SWITCH_NEGAALPHA_SEARCH_LEVEL {
        return negaalpha_eval(board, alpha, beta, lv, search);
    }
    search.eval_search_node_count += 1;

    // 探索範囲: [alpha, beta]
    let moves_bit: u64 = board.moves();

    if moves_bit == 0 {
        let mut board = board.clone();
        board.swap(); //pass
        if board.moves() == 0 {
            // passしても置くところがない == ゲーム終了
            board.swap();
            search.eval_search_leaf_node_count += 1;
            return solve_score(&board);
        }
        return -nws_eval_simple(&board, -beta, lv, search);
    }

    match eval_search_mpc(board, alpha, beta, lv, search) {
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
        set_move_eval(move_list, MOVE_ORDERING_EVAL_LEVEL_SIMPLE_SEARCH, alpha, search);
        sort_move_list(move_list);
    }

    let mut this_node_alpha = alpha;
    let mut best_score = -SCORE_INF;
    for move_board in move_list.iter() {
        let score = -nws_eval_simple(&move_board.board, -beta, lv - 1, search);
        if score >= beta {
            return score;
        }
        if score > this_node_alpha {
            this_node_alpha = score
        };
        if score > best_score {
            best_score = score
        };
    }

    best_score
}

/// Principal Variation Search (PVS) を用いて、盤面の評価値を計算する。
///
/// ## 引数
/// * `board`  - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha`  - 探索の下限値を示すアルファ値。
/// * `beta`   - 探索の上限値を示すベータ値。
/// * `lv`     - 探索レベル (何手先まで読むか)
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへのミュータブルな参照。
///
/// # 戻り値
/// * 探索結果として計算された評価値を表す整数値。
///   スコアは現在のプレイヤーから見た盤面の評価値を表す。
///
/// # 注記
/// * 置換表を使用しない。
/// * 最後の残り数手は、`negaalpha_eval`関数を使用した探索結果を用いる。
///     * 最後の残り数手は、`SWITCH_NEGAALPHA_SEARCH_LEVEL`で定義される。
///
pub fn pvs_eval_simple(board: &Board, alpha: i32, beta: i32, lv: i32, search: &mut Search) -> i32 {
    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    if lv < SWITCH_NEGAALPHA_SEARCH_LEVEL {
        return negaalpha_eval(board, alpha, beta, lv, search);
    }

    search.eval_search_node_count += 1;
    // 探索範囲: [alpha, beta]
    let moves_bit = board.moves();

    // pass or end ?
    if moves_bit == 0 {
        // 合法手がないならば
        let mut board = board.clone();
        board.swap(); //pass
        if board.moves() == 0 {
            // passしても合法手がない -> ゲーム終了
            board.swap();
            search.eval_search_leaf_node_count += 1;
            return solve_score(&board);
            // return simplest_eval(&mut board);
        }

        // passしたら、合法手がある -> 探索を続ける
        return -pvs_eval_simple(&board, -beta, -alpha, lv, search);
    }

    match eval_search_mpc(board, alpha, beta, lv, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => (),
    }

    let mut move_list: [MoveBoard; MOVE_MAX] = unsafe { mem::uninitialized() };
    // set_move_list(board, legal_moves, &mut move_list);
    let move_list_len = moves_bit.count_ones() as i32;
    let move_list = &mut move_list[..(move_list_len as usize)];
    set_move_list(board, moves_bit, move_list);

    // move ordering
    if move_list_len >= 2 {
        set_move_eval(move_list, MOVE_ORDERING_EVAL_LEVEL_SIMPLE_SEARCH, alpha, search);
        sort_move_list(move_list);
    }

    let mut this_node_alpha = alpha;
    let mut best_score;

    let mut move_list_iter = move_list.iter();

    // first move
    let first_move = move_list_iter.next().unwrap();
    best_score = -pvs_eval_simple(&first_move.board, -beta, -this_node_alpha, lv - 1, search);
    if best_score >= beta {
        return best_score;
    }
    if best_score > this_node_alpha {
        this_node_alpha = best_score
    };

    // other move
    for other_move in move_list_iter {
        let board = &other_move.board;
        let mut score = -nws_eval_simple(board, -this_node_alpha - 1, lv - 1, search);
        if score >= beta {
            return score;
        }
        if score > best_score {
            // 再探索
            score = -pvs_eval_simple(board, -beta, -this_node_alpha, lv - 1, search);
            if score >= beta {
                return score;
            }
            best_score = score;
            if score > this_node_alpha {
                this_node_alpha = score
            };
        }
    }

    best_score
}

/// 関数`pvs_perfect`で用いられるヌルウィンドウ探索（Null Window Search, NWS）
///
/// `alpha`から、`alpha + 1`までの範囲で、alpha-beta探索を行う。
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha` - 探索の下限値を示すアルファ値。
/// * `lv`     - 探索レベル (あと何手先まで読むか)
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへの可変な参照。
///
/// # 戻り値
/// * 探索結果として計算された盤面の評価値を表す整数値。
///   現在のプレイヤーから見た盤面の評価値を表す。
///
/// # 注記
/// * 置換表が存在しない場合は、`nvs_perfect_simple` 関数に切り替える。
/// * `nws_eval_simple` と大きく異なるところは、置換表を使用していることである。
/// * 最後の残り数手は、`nws_eval_simple`関数を使用した探索結果を用いる。
///     * 最後の残り数手は、`SWITCH_SIMPLE_SEARCH_LEVEL`で定義される。
pub fn nws_eval(board: &Board, mut alpha: i32, lv: i32, search: &mut Search) -> i32 {
    let mut beta = alpha + 1;

    if lv < SWITCH_SIMPLE_SEARCH_LEVEL {
        return nws_eval_simple(board, alpha, lv, search);
    }

    search.eval_search_node_count += 1;
    // 探索範囲: [alpha, beta]
    let mut moves_bit: u64 = board.moves();

    if moves_bit == 0 {
        let mut board = board.clone();
        board.swap(); //pass
        if board.moves() == 0 {
            // passしても置くところがない == ゲーム終了
            board.swap();
            search.eval_search_leaf_node_count += 1;
            return solve_score(&board);
            // return simplest_eval(&board);
        }
        return -nws_eval(&board, -beta, lv, search);
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

    if let Some(score) = t_table_cut_off_td(&mut alpha, &mut beta, lv, search.selectivity_lv, &td) {
        return score;
    }

    match eval_search_mpc(board, alpha, beta, lv, search) {
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

    if lv > 8 {
        if let Some(tt_move_list) = tt_move_list.as_mut() {
            let mut n_tt_skip = 0;
            if let Some(score) = et_cut_off(
                &mut alpha,
                &mut beta,
                tt_move_list,
                lv,
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
            lv,
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
            let score: i32 = -nws_eval(&move_board.board, -beta, lv - 1, search);
            if score >= beta {
                search.t_table.add(
                    board,
                    score,
                    SCORE_INF,
                    lv,
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
        set_move_eval(move_list, MOVE_ORDERING_EVAL_LEVEL, alpha, search);
        sort_move_list(move_list);
    }

    for move_board in move_list.iter() {
        if move_board.skip {
            continue;
        }
        let score = -nws_eval(&move_board.board, -beta, lv - 1, search);
        if score >= beta {
            search.t_table.add(
                board,
                score,
                SCORE_INF,
                lv,
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
            lv,
            search.selectivity_lv,
            best_move,
        );
    } else {
        search.t_table.add(
            board,
            -SCORE_INF,
            best_score,
            lv,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}

/// Principal Variation Search (PVS) を用いて、盤面の評価値を計算する。
///
///  PVS(Negascout)について :
///   https://ja.wikipedia.org/wiki/Negascout
///
/// # 引数
/// * `board`  - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha`  - 探索の下限値を示すアルファ値。
/// * `beta`   - 探索の上限値を示すベータ値。
/// * `lv`     - 探索レベル (あと何手先まで読むか)
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへのミュータブルな参照。
///
/// # 戻り値
/// * 探索結果として計算された評価値を表す整数値。
///   スコアは現在のプレイヤーから見た盤面の評価値を表す。
///
/// # 例
/// ```ignore
/// let board = Board::new(); // オセロの初期盤面を生成
/// let mut search = Search::new();
/// let alpha = -SCORE_INF; // 初期アルファ値の設定
/// let beta = SCORE_INF; // 初期ベータ値の設定
/// let lv = 10; // 10手先まで読む
/// let score = pvs_eval(&board, alpha, beta, lv, &mut search);
/// println!("Score: {}", score);
/// ```
///
/// # 注記
/// * 置換表が存在しない場合は、`pvs_perfect_simple` 関数に切り替える。
/// * `pvs_eval_simple` と大きく異なることは、置換表を使用していることである。
/// * 最後の残り数手は、`pvs_eval_simple`関数を使用した探索結果を用いる。
///     * 最後の残り数手は、`SWITCH_SIMPLE_SEARCH_LEVEL`で定義される。
///
pub fn pvs_eval(board: &Board, mut alpha: i32, mut beta: i32, lv: i32, search: &mut Search) -> i32 {
    if lv < SWITCH_SIMPLE_SEARCH_LEVEL {
        return pvs_eval_simple(board, alpha, beta, lv, search);
    }

    search.eval_search_node_count += 1;

    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    // 探索範囲: [alpha, beta]
    let mut moves_bit = board.moves();

    // pass or end ?
    if moves_bit == 0 {
        // 合法手がないならば
        let mut board = board.clone();
        board.swap(); //pass
        if board.moves() == 0 {
            // passしても合法手がない -> ゲーム終了
            board.swap();
            search.eval_search_leaf_node_count += 1;
            return solve_score(&board);
            // return simplest_eval(&board);
        }

        // passしたら、合法手がある -> 探索を続ける
        return -pvs_eval(&board, -beta, -alpha, lv, search);
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
    if let Some(score) = t_table_cut_off_td(&mut alpha, &mut beta, lv, search.selectivity_lv, &td) {
        return score;
    }

    match eval_search_mpc(board, alpha, beta, lv, search) {
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

    if lv > 8 {
        if let Some(tt_move_list) = tt_move_list.as_mut() {
            let mut n_tt_skip = 0;
            if let Some(score) = et_cut_off(
                &mut alpha,
                &mut beta,
                tt_move_list,
                lv,
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
            lv,
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
        if tt_move_list[0].put_place == NO_COORD {
            println!("err: ");
        }
        for move_board in tt_move_list.iter_mut() {
            if move_board.skip {
                continue;
            }
            if !pvs_ok {
                let score: i32 = -pvs_eval(&move_board.board, -beta, -alpha, lv - 1, search);
                if score >= beta {
                    search.t_table.add(
                        board,
                        score,
                        SCORE_INF,
                        lv,
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
                let put_board = &move_board.board;
                let mut score = -nws_eval(put_board, -this_node_alpha - 1, lv - 1, search);
                if score >= beta {
                    search.t_table.add(
                        board,
                        score,
                        SCORE_INF,
                        lv,
                        search.selectivity_lv,
                        move_board.put_place,
                    );
                    return score;
                }
                if score > best_score {
                    // 再探索
                    score = -pvs_eval(put_board, -beta, -this_node_alpha, lv - 1, search);
                    if score >= beta {
                        search.t_table.add(
                            board,
                            score,
                            SCORE_INF,
                            lv,
                            search.selectivity_lv,
                            move_board.put_place,
                        );
                        return score;
                    }
                    best_score = score;
                    best_move = move_board.put_place;
                    if score > this_node_alpha {
                        this_node_alpha = score
                    };
                }
            }
        }
    }

    // move ordering
    if move_list_len - n_skip >= 2 {
        set_move_eval(move_list, MOVE_ORDERING_EVAL_LEVEL, alpha, search);
        sort_move_list(move_list);
    }

    if !pvs_ok {
        for move_board in move_list.iter_mut() {
            if move_board.skip {
                continue;
            }
            best_move = move_board.put_place;
            best_score = -pvs_eval(&move_board.board, -beta, -this_node_alpha, lv - 1, search);
            if best_score >= beta {
                search.t_table.add(
                    board,
                    best_score,
                    SCORE_INF,
                    lv,
                    search.selectivity_lv,
                    best_move,
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
        let put_board = &move_board.board;
        let mut score = -nws_eval(put_board, -this_node_alpha - 1, lv - 1, search);
        if score >= beta {
            search.t_table.add(
                board,
                score,
                SCORE_INF,
                lv,
                search.selectivity_lv,
                move_board.put_place,
            );
            return score;
        }
        if score > best_score {
            // 再探索
            score = -pvs_eval(put_board, -beta, -this_node_alpha, lv - 1, search);
            if score >= beta {
                search.t_table.add(
                    board,
                    score,
                    SCORE_INF,
                    lv,
                    search.selectivity_lv,
                    move_board.put_place,
                );
                return score;
            }
            best_score = score;
            best_move = move_board.put_place;
            if score > this_node_alpha {
                this_node_alpha = score
            };
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
            lv,
            search.selectivity_lv,
            best_move,
        );
    } else {
        // best_score <= alpha
        search.t_table.add(
            board,
            -SCORE_INF,
            best_score,
            lv,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}
