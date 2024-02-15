use crate::board::*;
use crate::search::*;


use crate::mpc::*;

const SCORE_INF: i32 = i8::MAX as i32;

/// 空きマスが残り`SWITCH_EMPTIES_SIMPLE_PVS`以下である場合、
/// `pvs_perfect`から、`pvs_perfect_simple`へ切り替える
const SWITCH_EMPTIES_SIMPLE_PVS: i32 = 8;

/// 空きマスが残り`SWITCH_EMPTIES_SIMPLE_NWS`以下である場合、
/// `nws_perfect`から、`nws_perfect_simple`へ切り替える
const SWITCH_EMPTIES_SIMPLE_NWS: i32 = 8;

/// 空きマスが残り`SWITCH_EMPTIES_NEGA_ALPHA`以下である場合、
/// `pvs_perfect_simple`や`nws_perfect_simple`から、`negaalpha_perfect`へ切り替える
const SWITCH_EMPTIES_NEGA_ALPHA: i32 = 6;

/// `pvs_perfect`, `nws_perfect`でのmove orderingにおいて、
/// 空きマスが`SWITCH_EMPTIES_MOVE_ORDER`より多い場合、
/// 評価関数とNegascout探索を用いた`move_ordering_eval`を使用する。
/// SWITCH_EMPTIES_MOVE_ORDER以下である場合は、`move_ordering_ffs`を使用する。
const SWITCH_EMPTIES_MOVE_ORDER: i32 = 14;

/// `pvs_perfect`, `nws_perfect`でのmove orderingにおいて、評価関数とalpha-beta探索を用いた`move_ordering_eval`を使用する場合の、探索の深さ
const MOVE_ORDERING_EVAL_LEVEL_T: [i32; 61] = [
    0,
    0,  0,  0,  0,  0,  0,  0,  0,  1,  1,
    1,  1,  2,  2,  2,  2,  2,  2,  2,  2,
    2,  2,  3,  3,  3,  3,  3,  3,  3,  3,
    3,  3,  3,  3,  3,  3,  3,  3,  4,  4,
    4,  4,  4,  4,  4,  4,  4,  4,  4,  4,
    4,  4,  4,  4,  4,  4,  4,  4,  4,  4,
];


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
/// ```
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
    let n_player: i32 = board.bit_board[board.next_turn].count_ones() as i32;
    let n_opponent: i32 = board.bit_board[board.next_turn^1].count_ones() as i32;
    let diff: i32 = n_player - n_opponent;

    // https://github.com/rust-lang/rust-clippy/issues/5354
    // 速度重視のため、match分ではなく、if文を使用
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
/// ```
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
pub fn solve_score_0_empties(board: &Board) -> i32
{
    #[cfg(debug_assertions)]
    assert_eq!((board.bit_board[0]|board.bit_board[1]), u64::MAX);

    2 * (board.bit_board[board.next_turn].count_ones() as i32) - 64
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
pub fn negaalpha_perfect(board: &Board, mut alpha: i32, beta: i32, search: &mut Search) -> i32
{    

    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    // 空きマスがない場合
    if (board.bit_board[Board::BLACK] | board.bit_board[Board::WHITE]) == u64::MAX {
        search.perfect_search_node_count += 1;
        search.perfect_search_leaf_node_count += 1;
        return  solve_score_0_empties(board);
    }
    
    let mut legal_moves = board.put_able();

    // 合法手がない
    if legal_moves == 0 {
        if board.opponent_put_able() == 0 { // passしても置くところがない == ゲーム終了
            search.perfect_search_node_count += 1;
            search.perfect_search_leaf_node_count += 1;
            return solve_score(board);
        }
        let passed_board = {
            let mut b = board.clone();
            b.next_turn ^= 1;
            b
        };
        return -negaalpha_perfect(&passed_board, -beta, -alpha, search);
    }

    match perfect_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => {return score},
        ProbCutResult::FAIL => ()
    }

    // 探索範囲: [alpha, beta]
    search.perfect_search_node_count += 1;
    let mut best_score: i32 = -SCORE_INF;

    while legal_moves != 0 {
        let mut current_board = board.clone();
        let put_place: u64 = (!legal_moves + 1) & legal_moves;
        legal_moves &= legal_moves - 1; // bitを削除
        current_board.put_piece_fast(put_place);
        let score: i32 = -negaalpha_perfect(&current_board, -beta, -alpha, search);
        if score >= beta {
            return score;
        }
        if score > alpha {alpha = score;}
        if score > best_score {best_score = score}
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
pub fn nws_perfect_simple(board: &Board, mut alpha: i32, search: &mut Search) -> i32
{    

   search.perfect_search_node_count += 1;
    // 探索範囲: [alpha, beta]
    let beta: i32 = alpha + 1;
    
    if board.empties_count() < SWITCH_EMPTIES_NEGA_ALPHA  {
        return negaalpha_perfect(board, alpha, beta, search);
    }

    let legal_moves: u64 = board.put_able();

    if legal_moves == 0 {
        let mut board: Board = board.clone();
        board.next_turn ^= 1; //pass
        if board.put_able() == 0 { // passしても置くところがない == ゲーム終了
            search.perfect_search_leaf_node_count += 1;
            board.next_turn ^= 1;
            return  solve_score(&board);
        }
        return -nws_perfect_simple(&board, -beta, search);
    }

    match perfect_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => {return score},
        ProbCutResult::FAIL => ()
    }

    // move ordering
    let put_boards: Vec<PutBoard> = move_ordering_ffs(board, legal_moves, search);

    let mut best_score: i32 = -SCORE_INF;
    for current_put_board in put_boards.iter() {
        let current_put_board = &current_put_board.board;
        let score: i32 = -nws_perfect_simple(current_put_board, -beta, search);
        if score >= beta {
            return score;
        }
        if score > alpha {alpha = score;}
        if score > best_score {best_score = score;}
   }

   best_score
}


/// Principal Variation Search (PVS) を用いて、完全読みを行い、オセロの盤面のスコアを計算する。
/// 
/// `pvs_perfect`とは異なり、探索速度を優先するため、置換表を使用しない。
/// 浅い探索で用いられる。
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
pub fn pvs_perfect_simple(board: &Board, alpha: i32,beta: i32, search: &mut Search) -> i32
{
    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    if board.empties_count() < SWITCH_EMPTIES_NEGA_ALPHA  {
        return negaalpha_perfect(board, alpha, beta, search);
    }

    search.perfect_search_node_count += 1;

    // 探索範囲: [alpha, beta]
    let legal_moves: u64 = board.put_able();

    if legal_moves == 0 {
        let mut board: Board = board.clone();
        board.next_turn ^= 1; //pass
        if board.put_able() == 0 { // passしても置くところがない == ゲーム終了
            board.next_turn ^= 1;
            search.perfect_search_leaf_node_count += 1;
            return  solve_score(&board);
        }
        return -pvs_perfect_simple(&board, -beta, -alpha, search);
    }

    match perfect_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => {return score},
        ProbCutResult::FAIL => ()
    }

    // move ordering
    let mut put_boards: Vec<PutBoard> = move_ordering_ffs(board, legal_moves, search);
    let mut put_boards_iter = put_boards.iter_mut();
    
    let mut this_node_alpha: i32 = alpha;
    let mut best_score: i32;
    
    // first move
    let first_child_board: &mut PutBoard = put_boards_iter.next().unwrap();
    best_score =  -pvs_perfect_simple(&first_child_board.board, -beta, -this_node_alpha, search);
    if best_score >= beta { 
        return best_score;
    }
    if best_score > this_node_alpha {this_node_alpha = best_score;}

    // other move
    for current_put_board in put_boards_iter {
        let current_put_board: &Board = &current_put_board.board;
        let mut score: i32 = -nws_perfect_simple(current_put_board, -this_node_alpha - 1, search);
        if score >= beta {
            return score;
        }
        if best_score < score {
            score = -pvs_perfect_simple(current_put_board, -beta, -this_node_alpha, search);
            if beta <= score { 
                return score;
            }
            if best_score > this_node_alpha {this_node_alpha = best_score}
            best_score = score;
        }
        if score > this_node_alpha {this_node_alpha = score}
    }

    best_score
}


/// 関数`pvs_perfect`で用いられるヌルウィンドウ探索（Null Window Search, NWS）
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

pub fn nws_perfect(board: &Board, mut alpha: i32, search: &mut Search) -> i32
{
    let mut beta = alpha + 1;

    let n_empties: i32 = board.empties_count();
    if n_empties < SWITCH_EMPTIES_SIMPLE_NWS  {
        return nws_perfect_simple(board, alpha, search);
    }

    search.perfect_search_node_count += 1;

    // 探索範囲: [alpha, beta]
    let legal_moves: u64 = board.put_able();

    if legal_moves == 0 {
        if board.opponent_put_able() == 0 {
            search.perfect_search_leaf_node_count += 1;
            return  solve_score(board);
        }

        // 合法手がある -> 探索を続ける
        let passed_board: Board = {
            let mut b: Board = board.clone();
            b.next_turn ^= 1;
            b
        };
        return -nws_perfect(&passed_board, -beta, search);
    }

    if let Some(score) = t_table_cut_off(board, &mut alpha, &mut beta,60, search.selectivity_lv, search.t_table) {
        return score;
    }

    match perfect_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => {return score},
        ProbCutResult::FAIL => ()
    }
    // move ordering
    let put_boards: Vec<PutBoard> = {
        if n_empties > SWITCH_EMPTIES_MOVE_ORDER {
            let mo_lv = (8 - 2 * (search.origin_board.empties_count() - n_empties)).max(MOVE_ORDERING_EVAL_LEVEL_T[n_empties as usize]);
            move_ordering_eval(board, legal_moves, mo_lv, search)
        } else {
            move_ordering_ffs(board, legal_moves, search)
        }
    };

    let mut best_move: u8 = NO_COORD;
    let mut this_node_alpha: i32 = alpha;
    let mut best_score: i32 = -SCORE_INF;
    for put in put_boards.iter() {
        let score: i32 = -nws_perfect(&put.board, -beta, search);
        if score >= beta {
            search.t_table.add(board, score, SCORE_INF, 60, search.selectivity_lv, put.put_place);
            return score;
        }
        if score > this_node_alpha {this_node_alpha = score;}
        if score > best_score {
            best_score = score;
            best_move = put.put_place;
        }
    }

    if best_score > alpha {
        search.t_table.add(board, best_score, best_score, 60, search.selectivity_lv,  best_move);
    } else {
        search.t_table.add(board, -SCORE_INF, best_score, 60,search.selectivity_lv, best_move);
    }

    best_score
}

/// Principal Variation Search (PVS) を用いて、完全読みを行い、オセロの盤面のスコアを計算する。
///
///  PVS(Negascout)について :
///   https://ja.wikipedia.org/wiki/Negascout
///
/// # 引数
/// * `board` - 評価するオセロの盤面を表す `Board` オブジェクトの参照。
/// * `alpha` - 探索の下限値を示すアルファ値。
/// * `beta` - 探索の上限値を示すベータ値。
/// * `search` - 探索の状態を追跡する `Search` オブジェクトへのミュータブルな参照。
///
/// # 戻り値
/// * 探索結果として計算された盤面のスコアを表す整数値。
///   スコアは現在のプレイヤーから見た盤面の評価値を表す。
///
/// # 例
/// ```
/// let board = Board::new(); // オセロの初期盤面を生成
/// let mut search = Search::new();
/// let alpha = -SCORE_INF; // 初期アルファ値の設定
/// let beta = SCORE_INF; // 初期ベータ値の設定
/// let score = pvs_perfect(&board, alpha, beta, &mut search);
/// println!("Score: {}", score);
/// ```
///
/// # 注記
/// * 終盤の局面では、`pvs_perfect_simple` 関数に切り替わります。
/// * 置換表を使用して探索効率を向上させます。
pub fn pvs_perfect(board: &Board, mut alpha: i32,mut beta: i32, search: &mut Search) -> i32
{
    let n_empties = board.empties_count();
    if n_empties < SWITCH_EMPTIES_SIMPLE_PVS  {
        return pvs_perfect_simple(board, alpha, beta, search);
    }

    #[cfg(debug_assertions)]
    assert!(alpha <= beta);

    search.perfect_search_node_count += 1;

    // 探索範囲: [alpha, beta]
    let legal_moves: u64 = board.put_able();

    // pass or end ?
    if legal_moves == 0 { // 合法手がないならば
        if board.opponent_put_able() == 0 {
            search.perfect_search_leaf_node_count += 1;
            return  solve_score(board);
        }

        // 合法手がある -> 探索を続ける
        let passed_board: Board = {
            let mut b: Board = board.clone();
            b.next_turn ^= 1;
            b
        };
        return -pvs_perfect(&passed_board, -beta, -alpha, search);
    }

    // TranspositionTable Cut off
    if let Some(score) = t_table_cut_off(board, &mut alpha, &mut beta,60, search.selectivity_lv, search.t_table) {
        return score;
    }

    match perfect_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => {return score},
        ProbCutResult::FAIL => ()
    }

    // move ordering
    let put_boards: Vec<PutBoard> = {
        if n_empties > SWITCH_EMPTIES_MOVE_ORDER {
            let mo_lv = (8 - 2 * (search.origin_board.empties_count() - n_empties)).max(MOVE_ORDERING_EVAL_LEVEL_T[n_empties as usize]);
            move_ordering_eval(board, legal_moves, mo_lv, search)
        } else {
            move_ordering_ffs(board, legal_moves, search)
        }
    };

    let mut put_boards_iter: std::slice::Iter<'_, PutBoard> = put_boards.iter();
    
    let mut this_node_alpha: i32 = alpha;

    // first move
    let first_child_board: &PutBoard = put_boards_iter.next().unwrap();
    let mut best_move: u8 = first_child_board.put_place;
    let mut best_score: i32 =  -pvs_perfect(&first_child_board.board, -beta, -this_node_alpha, search);
    if best_score >= beta { 
        search.t_table.add(board, best_score, SCORE_INF, 60, search.selectivity_lv, best_move);
        return best_score;
    }
    this_node_alpha = this_node_alpha.max(best_score);

    // other move
    for put in put_boards_iter {
        let mut score: i32 = -nws_perfect(&put.board, -this_node_alpha - 1, search);
        if score >= beta {
            search.t_table.add(board, score, SCORE_INF, 60, search.selectivity_lv, first_child_board.put_place);
            return score;
        }
        if score > best_score {
            // 再探索
            score = -pvs_perfect(&put.board, -beta, -this_node_alpha, search);
            if score >= beta { 
                search.t_table.add(board, score, SCORE_INF, 60, search.selectivity_lv, best_move);
                return score;
            }
            if score > best_score {
                if score > this_node_alpha {this_node_alpha = score;}
                best_move = put.put_place;
                best_score = score;
            }
        }
        if score > this_node_alpha {this_node_alpha = score;}
    }

    if best_score > alpha { // alpha < best_score < beta
        search.t_table.add(board, best_score, best_score, 60, search.selectivity_lv, best_move);
    } else { // best_score <= alpha
        search.t_table.add(board, -SCORE_INF, best_score, 60, search.selectivity_lv, best_move);
    }

    best_score
}


