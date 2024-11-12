use crate::board::*;
use crate::eval::Evaluator;
use crate::eval_search::*;
use crate::t_table::*;

const SCORE_INF: i32 = i8::MAX as i32;


pub struct PutBoard {
    eval: i32,
    pub board: Board,
    pub put_place: u8
}

/// 評価関数に基づいて、手の順序を決定するための関数。
///
/// この関数は、オセロの盤面上で着手可能な各手に対して、評価値の基づく探索を実施し、
/// それぞれの手の評価値に基づいて手を並び替える。
///
/// # 引数
/// * `board` - 現在のオセロの盤面を表す `Board` オブジェクトの参照。
/// * `legal_moves` - 現在の手番のプレイヤーが打てる合法手を表すビットボード。
/// * `lv` - 探索の深さを表す整数値。
///
/// # 戻り値
/// * `PutBoard` オブジェクトのベクター。
///   * 各手の評価値(`i32`), 
///   * 着手された盤面(`Board`), 
///   * ビットボードで表された着手した箇所(`u64`)
/// 
/// # 注記
/// * `move_ordering_ffs`との違い
///   * `move_ordering_eval`は、評価値の高い順に並び替える。
///   * `move_ordering_ffs`は、相手の合法手が少ない順に並び替える。
pub fn move_ordering_eval(board: &Board, mut legal_moves: u64, lv: i32, search: &mut Search) -> Vec<PutBoard>
{
    let mut put_boards: Vec<PutBoard> = Vec::with_capacity(legal_moves.count_ones() as usize);
    let tt_best_move =
        if let Some(t) = search.t_table.get(board) {
            1u64 << t.best_move
        } else {
            0u64
        };
    
    while legal_moves != 0 {
        let put_place = (!legal_moves + 1) & legal_moves;
        legal_moves &= legal_moves - 1;
        let mut put_board = board.clone();
        put_board.put_piece_fast(put_place);
        let eval = 
        if tt_best_move == put_place {
            SCORE_INF
        } else {
            let main_search_selectivity_lv = search.selectivity_lv;
            let e = -pvs_eval(&put_board, -SCORE_INF, SCORE_INF, lv-1, search);
            search.selectivity_lv = main_search_selectivity_lv;
            e
        };
        put_boards.push(PutBoard{eval: eval, board: put_board, put_place: put_place.trailing_zeros() as u8});
    }

    if put_boards.len() > 2 {
        put_boards.sort_unstable_by(|a, b| b.eval.partial_cmp(&a.eval).unwrap());
    }

    put_boards
}




/// 速さ優先探索(Fast First Search)のための、move ordering
/// 合法手を評価し、手の順序を決定するための関数。
///
/// オセロの盤面上で着手可能な各手に対して、
/// 合法手が少ない順に手を並び替える。
///
/// # 引数
/// * `board` - 現在のオセロの盤面を表す `Board` オブジェクトの参照。
/// * `legal_moves` - 現在の手番のプレイヤーが打てる合法手を表すビットボード。
///
/// # 戻り値
/// * `PutBoard` オブジェクトのベクター。
///   * 各手の評価値(`i32`), 
///   * 着手された盤面(`Board`), 
///   * ビットボードで表された着手した箇所(`u64`)
/// 
/// # 注記
/// * `move_ordering_eval`との違い
///   * `move_ordering_eval`は、評価値の高い順に並び替える。
///   * `move_ordering_ffs`は、相手の合法手が少ない順に並び替える。
#[inline(always)]
pub fn move_ordering_ffs(board: &Board, mut legal_moves: u64, _search: &mut Search) -> Vec<PutBoard>
{
    let mut put_boards: Vec<PutBoard> = Vec::with_capacity(legal_moves.count_ones() as usize);

    while legal_moves != 0 {
        let put_place = (!legal_moves + 1) & legal_moves;
        legal_moves &= legal_moves - 1;
        let mut put_board = board.clone();
        put_board.put_piece_fast(put_place);

        let eval = -(put_board.put_able().count_ones() as i32);
        put_boards.push(PutBoard{eval: eval, board: put_board, put_place: put_place.trailing_zeros() as u8})
    }

    if put_boards.len() > 2{
        put_boards.sort_unstable_by(|a, b| b.eval.partial_cmp(&a.eval).unwrap());
    }
    put_boards
}

#[inline(always)]
pub fn get_put_boards(board: &Board, mut legal_moves: u64) -> Vec<PutBoard>
{
    let mut put_boards: Vec<PutBoard> = Vec::with_capacity(legal_moves.count_ones() as usize);

    while legal_moves != 0 {
        let put_place = (!legal_moves + 1) & legal_moves;
        legal_moves &= legal_moves - 1;
        let mut put_board = board.clone();
        put_board.put_piece_fast(put_place);
        put_boards.push(PutBoard{eval: 0, board: put_board, put_place: put_place.trailing_zeros() as u8})
    }

    put_boards
}

#[inline(always)]
pub fn t_table_cut_off(
    board   :       & Board,
    alpha   :       &mut i32,
    beta    :       &mut i32,
    lv      :       i32,
    selectivity_lv: i32,
    t_table :       & TranspositionTable ) -> Option<i32>
{
    if let Some(t) = t_table.get(board) {
        if t.lv as i32 != lv || t.selectivity_lv as i32 != selectivity_lv {return None;}
        let max = t.max as i32;
        let min = t.min as i32;
        if max <= *alpha {return Some(max);}
        else if min >= *beta {return Some(min);}
        else if max == min {return Some(max);}
        if min > *alpha {*alpha = min};
        if max < *beta {*beta = max};
    }
    None
}

pub struct Search {
    pub eval_search_node_count: u64,
    pub eval_search_leaf_node_count: u64,
    pub perfect_search_node_count: u64,
    pub perfect_search_leaf_node_count: u64,
    pub t_table: TranspositionTable,
    pub origin_board: Board,
    pub eval_func: Evaluator,
    pub selectivity_lv: i32,
}

impl Search {
    pub fn new(evaluator: Evaluator) -> Search{
        Search{
            eval_search_node_count: 0,
            eval_search_leaf_node_count: 0,
            perfect_search_node_count: 0,
            perfect_search_leaf_node_count: 0,
            t_table: TranspositionTable::new(),
            origin_board: Board::new(),
            eval_func: evaluator,
            selectivity_lv: 0
        }
    }
    pub fn clear_node_count(&mut self){
        self.eval_search_node_count = 0;
        self.eval_search_leaf_node_count = 0;
        self.perfect_search_node_count = 0;
        self.perfect_search_leaf_node_count = 0;
    }
    pub fn clear_t_table(&mut self){
        self.t_table = TranspositionTable::new();
    }

    pub fn set_board(&mut self, board :&Board) {
        self.origin_board = board.clone();
        self.clear_node_count();
    }
}