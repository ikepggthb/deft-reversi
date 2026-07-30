//! 終盤の Null-Window Search (NWS) 完全読み。
//!
//! ## 関数階層
//!
//! ```text
//! nws_final               (TT + ETC + MPC + 浅い eval ordering)
//!   └─ nws_final_simple   (MPC + FFS ordering のみ)
//!        └─ negaalpha_final  (parity ordering のみ)
//!             └─ solve_score_2_empties
//! ```
//!
//! ## 定数
//!
//! - `SWITCH_EMPTIES_NEGA_ALPHA` 空きマス以下では `negaalpha_final` に切り替える
//! - `SWITCH_EMPTIES_SIMPLE_NWS` 空きマス以下では `nws_final_simple` に切り替える
//! - `FINAL_LV` 置換表に登録する探索レベル(60 = 完全読み)

use crate::{
    board::{board::Board, constant::NO_COORD},
    eval::evaluator_const::SCORE_MAX,
    search::{
        final_search::{
            leaf::QUADRANT_ID,
            negaalpha::negaalpha_final,
            solve_score::{final_parity, solve_score},
        },
        move_list::*,
        mpc::{final_search_mpc, ProbCutResult},
        search::{SearchContext, SearchStats},
        stability_cut::stability_cut_nws,
        thread_pool::{DetachedJob, HelperSlot, Job, TaskHandle, TaskResult},
        tt_cut::*,
    },
    t_table::{TTProbe, TTSlot, TTValue},
};
use arrayvec::ArrayVec;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::{cell::UnsafeCell, cmp};

const TT_MOVE0_SCORE: i32 = 1 << 20;
const TT_MOVE1_SCORE: i32 = 1 << 19;

const FINAL_LV: i32 = 60;

/// 空きマスがこれ以下のとき `negaalpha_final` に切り替える。
const SWITCH_EMPTIES_NEGA_ALPHA: i32 = 5;

/// 空きマスがこれ以下のとき `nws_final_simple` に切り替える。
const SWITCH_EMPTIES_SIMPLE_NWS: i32 = 13;

const YBWC_END_SPLIT_MIN_EMPTIES: i32 = 16;
const YBWC_TAIL_SPLIT_EMPTIES: i32 = 15;
const YBWC_TAIL_MIN_IDLE_WORKERS: usize = 4;
const YBWC_MAX_SLAVES: usize = 3;
/// master が待機中に「未実行の仕事がキューに残っていないか」を見に行く間隔。
/// slave の完了自体は condvar で即座に通知されるため、完了検知の遅延ではない。
const HELPER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_micros(50);
const YBWC_SCORE_UNSET: i32 = SCORE_MAX + 1;
const LEGAL_UNDEFINED: u64 = u64::MAX;

struct NwsSplitPoint {
    work: Box<[(usize, Board)]>,
    next: AtomicUsize,
    scores: [AtomicI32; 64],
    searching: Arc<AtomicBool>,
    beta: i32,
    /// slave の完了を master へ伝える受け口。子孫からの仕事の受け口でもある。
    helper: Arc<HelperSlot>,
    /// slave が積み上げた探索統計。master が最後に自分へ加算する。
    slave_stats: Mutex<SearchStats>,
    /// いま走っている slave の数。終了したら減るので、その分だけ追加投入できる。
    active_slaves: AtomicUsize,
}

impl NwsSplitPoint {
    fn new(work: Box<[(usize, Board)]>, beta: i32) -> Self {
        Self {
            work,
            next: AtomicUsize::new(0),
            scores: std::array::from_fn(|_| AtomicI32::new(YBWC_SCORE_UNSET)),
            searching: Arc::new(AtomicBool::new(true)),
            beta,
            helper: Arc::new(HelperSlot::new()),
            slave_stats: Mutex::new(SearchStats::default()),
            active_slaves: AtomicUsize::new(0),
        }
    }

    /// まだ誰にも割り当てられていない仕事が残っているか。
    #[inline(always)]
    fn has_unclaimed_work(&self) -> bool {
        self.searching.load(Ordering::Acquire) && self.next.load(Ordering::Relaxed) < self.work.len()
    }

    /// slave を 1 つ追加できる状態か。
    #[inline(always)]
    fn can_add_slave(&self) -> bool {
        self.active_slaves.load(Ordering::Relaxed) < YBWC_MAX_SLAVES && self.has_unclaimed_work()
    }

    /// slave が 1 件終わったときに呼ぶ。統計を積んで master を起こす。
    fn slave_finished(&self, stats: SearchStats, aborted: bool) {
        self.active_slaves.fetch_sub(1, Ordering::Relaxed);
        {
            let mut acc = self.slave_stats.lock().unwrap();
            acc.add_assign(stats);
            if aborted {
                acc.ybwc_split_aborts += 1;
            }
        }
        self.helper.notify_completion();
    }

    /// master が spawn 済みの slave をすべて待ち、統計を回収する。
    fn join_slaves(&self, spawned: u64, search: &mut SearchContext) {
        if spawned == 0 {
            return;
        }
        // 待っている間はキューに残った仕事を実行する。
        // ワーカー自身も分割点の master になってブロックしうるため、
        // 待機中の master が実行を肩代わりしないと、キューの仕事を走らせる者が
        // いなくなり停止する。
        let pool = search.thread_pool.clone();
        loop {
            if self.helper.is_complete(spawned) {
                break;
            }
            // 1. 子孫から直接渡された仕事
            if let Some(job) = self.helper.take_offered_job() {
                job();
                continue;
            }
            // 2. キューに残った仕事 (誰も実行できず停止するのを防ぐ)
            if let Some(pool) = pool.as_deref() {
                if let Some(job) = pool.try_pop_job() {
                    let _ = job();
                    continue;
                }
            }
            // 3. 完了か仕事の受け取りまで待つ
            if let Some(job) = self.helper.wait_or_take_job(spawned, HELPER_POLL_INTERVAL) {
                job();
            }
        }
        let stats = std::mem::take(&mut *self.slave_stats.lock().unwrap());
        search.stats.add_assign(stats);
    }

    #[inline(always)]
    fn next_work(&self) -> Option<(usize, usize, Board)> {
        if !self.searching.load(Ordering::Acquire) {
            return None;
        }
        let work_index = self.next.fetch_add(1, Ordering::Relaxed);
        self.work
            .get(work_index)
            .map(|&(move_index, board)| (work_index, move_index, board))
    }

    #[inline(always)]
    fn finish(&self, work_index: usize, score: i32) {
        self.scores[work_index].store(score, Ordering::Release);
        if score >= self.beta {
            self.searching.store(false, Ordering::Release);
        }
    }
}

#[derive(Clone, Copy)]
struct SimpleMove {
    score: i32,
    move_num: u8,
    mobility: u8,
    is_skip: bool,
    flip_bit: u64,
    child_moves: u64,
}

const LOCAL_TT_SIZE: usize = 2048;
const LOCAL_TT_LAYERS: usize = (SWITCH_EMPTIES_SIMPLE_NWS - SWITCH_EMPTIES_NEGA_ALPHA) as usize;

#[derive(Clone, Copy)]
struct LocalTTEntry {
    player: u64,
    opponent: u64,
    lower: i8,
    upper: i8,
    selectivity_lv: u8,
}

const EMPTY_LOCAL_TT_ENTRY: LocalTTEntry = LocalTTEntry {
    player: 0,
    opponent: 0,
    lower: -64,
    upper: 64,
    selectivity_lv: u8::MAX,
};

thread_local! {
    static LOCAL_END_TT: UnsafeCell<[LocalTTEntry; LOCAL_TT_SIZE * LOCAL_TT_LAYERS]> =
        const { UnsafeCell::new([EMPTY_LOCAL_TT_ENTRY; LOCAL_TT_SIZE * LOCAL_TT_LAYERS]) };
}

#[inline(always)]
fn local_tt_index(board: &Board, n_empties: i32) -> usize {
    let hash = (board.player ^ board.opponent.rotate_left(32)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let layer =
        (n_empties - SWITCH_EMPTIES_NEGA_ALPHA).clamp(0, LOCAL_TT_LAYERS as i32 - 1) as usize;
    layer * LOCAL_TT_SIZE + ((hash >> 53) as usize)
}

// ── nws_final_simple ──────────────────────────────────────────────────────────

/// TT なしの簡易 NWS 完全読み。FFS 手順 + MPC のみ。
///
/// 空きマスが少ないとき `negaalpha_final` へ降格する。
pub fn nws_final_simple(board: &Board, alpha: i32, search: &mut SearchContext) -> i32 {
    let n_empties = (board.player | board.opponent).count_zeros() as i32;
    LOCAL_END_TT.with(|table| {
        // SAFETY: the table is thread-local, and recursive calls stay in the impl.
        let table = unsafe { &mut *table.get() };
        nws_final_simple_impl(board, alpha, LEGAL_UNDEFINED, n_empties, search, table)
    })
}

#[inline(always)]
fn local_tt_child_entry(
    board: &Board,
    n_empties: i32,
    selectivity_lv: i32,
    local_tt: &[LocalTTEntry],
) -> Option<LocalTTEntry> {
    let entry = local_tt[local_tt_index(board, n_empties)];
    (entry.player == board.player
        && entry.opponent == board.opponent
        && entry.selectivity_lv as i32 == selectivity_lv)
        .then_some(entry)
}

#[inline(always)]
fn local_tt_store_child(
    board: &Board,
    score: i32,
    is_lower_bound: bool,
    n_empties: i32,
    selectivity_lv: i32,
    local_tt: &mut [LocalTTEntry],
) {
    let entry = &mut local_tt[local_tt_index(board, n_empties)];
    entry.player = board.player;
    entry.opponent = board.opponent;
    entry.selectivity_lv = selectivity_lv as u8;
    if is_lower_bound {
        entry.lower = score as i8;
        entry.upper = 64;
    } else {
        entry.lower = -64;
        entry.upper = score as i8;
    }
}

fn nws_final_simple_impl(
    board: &Board,
    alpha: i32,
    moves_bit: u64,
    n_empties: i32,
    search: &mut SearchContext,
    local_tt: &mut [LocalTTEntry],
) -> i32 {
    let beta = alpha + 1;

    if n_empties <= SWITCH_EMPTIES_NEGA_ALPHA {
        return negaalpha_final(board, alpha, beta, search);
    }

    search.stats.final_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    let moves_bit = if moves_bit == LEGAL_UNDEFINED {
        board.moves()
    } else {
        moves_bit
    };
    if moves_bit == 0 {
        let passed = board.passed();
        let passed_moves = passed.moves();
        if passed_moves == 0 {
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -nws_final_simple_impl(&passed, -beta, passed_moves, n_empties, search, local_tt);
    }

    if let Some(score) = stability_cut_nws(board, alpha, n_empties, search) {
        return score;
    }

    match final_search_mpc(board, alpha, beta, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    if moves_bit.is_power_of_two() {
        let child_board = board.make_move(moves_bit);
        let child_moves = child_board.moves();
        let score = -nws_final_simple_impl(
            &child_board,
            -beta,
            child_moves,
            n_empties - 1,
            search,
            local_tt,
        );
        if search.is_aborted() {
            return alpha;
        }
        return score;
    }

    let mut move_list = ArrayVec::<SimpleMove, { SWITCH_EMPTIES_SIMPLE_NWS as usize }>::new();
    let parity = final_parity(board.player, board.opponent);
    let mut moves = moves_bit;
    while moves != 0 {
        let move_num = moves.trailing_zeros() as u8;
        let move_bit = 1u64 << move_num;
        let flip_bit = board.flip_bit(move_bit);
        if flip_bit == board.opponent {
            return SCORE_MAX;
        }
        let child_board = board.make_move_from_flip_bit(move_bit, flip_bit);
        let child_moves = child_board.moves();
        // 4 分割した盤面のどこに属するか。`QUADRANT_ID` と同じ値を返す。
        let region = QUADRANT_ID[move_num as usize];
        let mobility =
            (child_moves.count_ones() + (child_moves & 0x8100_0000_0000_0081).count_ones()) as u8;
        let score = -i32::from(mobility) * 18 + i32::from(parity & region != 0) * 17;
        move_list.push(SimpleMove {
            score,
            move_num,
            mobility,
            is_skip: false,
            flip_bit,
            child_moves,
        });
        moves &= moves - 1;
    }
    let mut best_score = -SCORE_MAX;
    for mb in move_list.iter_mut() {
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        if let Some(entry) =
            local_tt_child_entry(&child_board, n_empties - 1, search.selectivity_lv, local_tt)
        {
            if entry.lower as i32 > alpha {
                return entry.lower as i32;
            }
            if entry.upper as i32 <= alpha {
                best_score = cmp::max(best_score, entry.upper as i32);
                mb.is_skip = true;
                continue;
            }
        }
        if mb.mobility <= 1 {
            let score = -nws_final_simple_impl(
                &child_board,
                -beta,
                mb.child_moves,
                n_empties - 1,
                search,
                local_tt,
            );
            if search.is_aborted() {
                return alpha;
            }
            if score >= beta {
                local_tt_store_child(
                    &child_board,
                    score,
                    true,
                    n_empties - 1,
                    search.selectivity_lv,
                    local_tt,
                );
                return score;
            }
            best_score = cmp::max(best_score, score);
            local_tt_store_child(
                &child_board,
                score,
                false,
                n_empties - 1,
                search.selectivity_lv,
                local_tt,
            );
            mb.is_skip = true;
        }
    }

    for move_index in 0..move_list.len() {
        let mut best_index = move_index;
        for i in (move_index + 1)..move_list.len() {
            if move_list[i].score > move_list[best_index].score {
                best_index = i;
            }
        }
        move_list.swap(move_index, best_index);
        let mb = &move_list[move_index];
        if mb.is_skip {
            continue;
        }
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        let score = -nws_final_simple_impl(
            &child_board,
            -beta,
            mb.child_moves,
            n_empties - 1,
            search,
            local_tt,
        );
        if search.is_aborted() {
            return alpha;
        }
        if score >= beta {
            local_tt_store_child(
                &child_board,
                score,
                true,
                n_empties - 1,
                search.selectivity_lv,
                local_tt,
            );
            return score;
        }
        if score > best_score {
            best_score = score;
        }
        local_tt_store_child(
            &child_board,
            score,
            false,
            n_empties - 1,
            search.selectivity_lv,
            local_tt,
        );
    }
    best_score
}

// ── nws_final ─────────────────────────────────────────────────────────────────

/// TT + ETC + MPC + 浅い eval ordering を使った NWS 完全読み。
///
/// 空きマスが少ないとき `nws_final_simple` へ降格する。
pub fn nws_final(board: &Board, alpha: i32, search: &mut SearchContext) -> i32 {
    let beta = alpha + 1;

    let n_empties = (board.player | board.opponent).count_zeros() as i32;
    if n_empties <= SWITCH_EMPTIES_SIMPLE_NWS {
        return nws_final_simple(board, alpha, search);
    }

    search.stats.final_search_nodes += 1;
    if search.check_abort() {
        return alpha;
    }

    let moves_bit = board.moves();
    if moves_bit == 0 {
        let passed = board.passed();
        if passed.moves() == 0 {
            search.stats.final_search_leaf_nodes += 1;
            return solve_score(board);
        }
        return -nws_final(&passed, -beta, search);
    }

    // ── 通常手リストの生成 ───────────────────────────────────────────────────
    let mut move_list = make_move_list(board, moves_bit);

    // 全消しがある場合は、即時return
    for move_board in move_list.iter() {
        if board.opponent ^ move_board.flip_bit == 0 {
            return SCORE_MAX;
        }
    }

    if let Some(score) = stability_cut_nws(board, alpha, n_empties, search) {
        return score;
    }

    // ── 置換表プローブ ───────────────────────────────────────────────────────
    let probe: TTProbe = search.tt.probe(board);
    let tt_value: Option<TTValue> = probe.value();
    let mut alpha_cur = alpha;
    let mut beta_cur = beta;

    if let Some(v) = tt_value {
        if let Some(score) = tt_cut(
            v,
            FINAL_LV,
            search.selectivity_lv,
            &mut alpha_cur,
            &mut beta_cur,
        ) {
            return score;
        }
    }

    // ── MPC ──────────────────────────────────────────────────────────────────
    match final_search_mpc(board, alpha_cur, beta_cur, search) {
        ProbCutResult::Cut(score) => return score,
        ProbCutResult::Fail => {}
    }

    // ── ETC ───────────────────────────────────────────────────────────────────
    if n_empties > 12 {
        match e_tt_cut(
            board,
            alpha,
            beta_cur,
            &mut move_list,
            FINAL_LV,
            search.selectivity_lv,
            search,
        ) {
            ETCResult::BetaCut(beta) => return beta,
            ETCResult::NarrowAlpha(na) => alpha_cur = na,
            ETCResult::AllMovesSkipped(upper) => return upper,
        };
    }

    // ── TT 手の ordering score 反映 ──────────────────────────────────────────
    if let Some(value) = tt_value {
        for ml in move_list.iter_mut() {
            if ml.move_num == value.move0 {
                ml.score = TT_MOVE0_SCORE;
            } else if ml.move_num == value.move1 {
                ml.score = TT_MOVE1_SCORE;
            }
        }
    }

    // ── move ordering ─────────────────────────────────────────────────────────
    if move_list.iter().filter(|mb| !mb.is_skip).take(2).count() >= 2 {
        let eval_depth = match n_empties {
        //    11..13 => 0,
        //    13..17 => 1,
        //    17..21 => 2,
            1..24 => n_empties >> 4,
            _ => 1
        };
        assign_ordering_scores_weighted(
            board,
            &mut move_list,
            eval_depth,
            alpha_cur,
            7 + 25 * eval_depth,
            17,
            1 << 15,
            search,
        );
        sort_move_list(&mut move_list);
    }

    if should_split_ybwc(n_empties, &move_list, search) {
        return nws_final_ybwc(
            board,
            alpha,
            beta_cur,
            alpha_cur,
            probe.slot(),
            &move_list,
            search,
        );
    }

    // ── 探索ループ ────────────────────────────────────────────────────────────
    let mut best_score = -SCORE_MAX;
    let mut best_move = NO_COORD;

    for mb in move_list.iter() {
        if mb.is_skip {
            continue;
        }
        let child_board = board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit);
        let score = -nws_final(&child_board, -beta_cur, search);
        if search.is_aborted() {
            return alpha;
        }
        if score >= beta_cur {
            search.tt.store(
                probe.slot(),
                board,
                score,
                SCORE_MAX,
                FINAL_LV,
                search.selectivity_lv,
                mb.move_num,
            );
            return score;
        }
        if score > alpha_cur {
            alpha_cur = score;
        }
        if score > best_score {
            best_score = score;
            best_move = mb.move_num;
        }
    }

    debug_assert_ne!(best_move, NO_COORD);

    if best_score > alpha {
        search.tt.store(
            probe.slot(),
            board,
            best_score,
            best_score,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    } else {
        search.tt.store(
            probe.slot(),
            board,
            -SCORE_MAX,
            best_score,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}

fn should_split_ybwc(n_empties: i32, move_list: &[MoveBoard], search: &SearchContext) -> bool {
    let can_split_depth = n_empties >= YBWC_END_SPLIT_MIN_EMPTIES
        || (n_empties == YBWC_TAIL_SPLIT_EMPTIES
            && search
                .thread_pool
                .as_ref()
                .is_some_and(|pool| pool.idle_worker_count() >= YBWC_TAIL_MIN_IDLE_WORKERS));
    can_split_depth
        && search.thread_pool.is_some()
        && move_list.iter().filter(|mb| !mb.is_skip).take(2).count() >= 2
}

fn nws_final_ybwc(
    board: &Board,
    alpha: i32,
    beta: i32,
    mut alpha_cur: i32,
    tt_slot: TTSlot,
    move_list: &[MoveBoard],
    search: &mut SearchContext,
) -> i32 {
    if search.thread_pool.is_none() {
        return alpha;
    }
    let Some((first_index, first_move)) = move_list.iter().enumerate().find(|(_, mv)| !mv.is_skip)
    else {
        return alpha;
    };
    let first_child = board.make_move_from_flip_bit(1 << first_move.move_num, first_move.flip_bit);
    let first_score = -nws_final(&first_child, -beta, search);
    if search.is_aborted() {
        return alpha;
    }
    if first_score >= beta {
        search.tt.store(
            tt_slot,
            board,
            first_score,
            SCORE_MAX,
            FINAL_LV,
            search.selectivity_lv,
            first_move.move_num,
        );
        return first_score;
    }

    alpha_cur = alpha_cur.max(first_score);
    let mut best_score = first_score;
    let mut best_move = first_move.move_num;
    let work = move_list
        .iter()
        .enumerate()
        .skip(first_index + 1)
        .filter(|(_, mb)| !mb.is_skip)
        .map(|(move_index, mb)| {
            (
                move_index,
                board.make_move_from_flip_bit(1 << mb.move_num, mb.flip_bit),
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let split = Arc::new(NwsSplitPoint::new(work, beta));
    let mut spawned = 0u64;

    // 手を1つ探索するたびに slave の追加投入を試みる。
    //
    // 分割点に入った瞬間だけ投入すると、その時点で全スレッドが忙しい場合に
    // この分割点は最後まで master の直列探索になってしまう。
    // 後から空いたスレッドを拾えるよう、edax の node_split と同様に
    // ループの中で毎回試す。
    macro_rules! try_add_slaves {
        () => {
            while split.can_add_slave() && search.can_spawn_split_job() {
                let job = make_nws_split_worker(split.clone(), search);
                split.active_slaves.fetch_add(1, Ordering::Relaxed);
                match search.spawn_split_job(job) {
                    Ok(()) => {
                        search.stats.ybwc_splits += 1;
                        spawned += 1;
                    }
                    Err(_) => {
                        split.active_slaves.fetch_sub(1, Ordering::Relaxed);
                        break;
                    }
                }
            }
        };
    }

    try_add_slaves!();
    while let Some((work_index, _, child_board)) = split.next_work() {
        try_add_slaves!();
        search.searchings.push(split.searching.clone());
        search.helper_chain.push(split.helper.clone());
        let score = -nws_final(&child_board, -beta, search);
        search.helper_chain.pop();
        search.searchings.pop();
        if search.is_aborted() {
            if search.recover_from_split_abort(&split.searching) {
                break;
            }
            split.searching.store(false, Ordering::Release);
            split.join_slaves(spawned, search);
            return alpha;
        }
        split.finish(work_index, score);
        if score >= beta {
            break;
        }
    }

    split.join_slaves(spawned, search);
    if search.check_abort_now() {
        split.searching.store(false, Ordering::Release);
        return alpha;
    }
    for (work_index, &(move_index, _)) in split.work.iter().enumerate() {
        let score = split.scores[work_index].load(Ordering::Acquire);
        if score == YBWC_SCORE_UNSET {
            continue;
        }
        let mb = &move_list[move_index];
        if score >= beta {
            search.tt.store(
                tt_slot,
                board,
                score,
                SCORE_MAX,
                FINAL_LV,
                search.selectivity_lv,
                mb.move_num,
            );
            return score;
        }
        alpha_cur = alpha_cur.max(score);
        if score > best_score {
            best_score = score;
            best_move = mb.move_num;
        }
    }

    debug_assert_ne!(best_move, NO_COORD);

    if best_score > alpha {
        search.tt.store(
            tt_slot,
            board,
            best_score,
            best_score,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    } else {
        search.tt.store(
            tt_slot,
            board,
            -SCORE_MAX,
            best_score,
            FINAL_LV,
            search.selectivity_lv,
            best_move,
        );
    }

    best_score
}

fn make_nws_split_worker(split: Arc<NwsSplitPoint>, parent: &SearchContext) -> DetachedJob {
    let evaluator = parent.evaluator.clone();
    let ordering_evaluator = parent.ordering_evaluator.clone();
    let mpc_config = parent.mpc_config.clone();
    let tt = parent.tt.clone();
    let stop = parent.stop.clone();
    let thread_pool = parent.thread_pool.clone();
    let selectivity_lv = parent.selectivity_lv;
    let mut searchings = parent.searchings.clone();
    searchings.push(split.searching.clone());
    let mut helper_chain = parent.helper_chain.clone();
    helper_chain.push(split.helper.clone());

    Box::new(move || {
        let mut stats = crate::search::search::SearchStats::default();
        let mut search = SearchContext::new(evaluator, mpc_config, tt, &mut stats)
            .with_ordering_evaluator(ordering_evaluator)
            .with_stop(stop)
            .with_thread_pool(thread_pool)
            .with_searchings(searchings)
            .with_helper_chain(helper_chain);
        search.selectivity_lv = selectivity_lv;
        while let Some((work_index, _, child_board)) = split.next_work() {
            let score = -nws_final(&child_board, -split.beta, &mut search);
            if search.is_aborted() {
                break;
            }
            split.finish(work_index, score);
            if score >= split.beta {
                break;
            }
        }
        let aborted = search.is_aborted();
        drop(search);
        split.slave_finished(stats, aborted);
    })
}

pub(crate) fn make_ybwc_job(
    child_board: Board,
    child_alpha: i32,
    cutoff_score: i32,
    move_index: usize,
    split_searching: Arc<AtomicBool>,
    parent: &SearchContext,
) -> Job {
    let evaluator = parent.evaluator.clone();
    let ordering_evaluator = parent.ordering_evaluator.clone();
    let mpc_config = parent.mpc_config.clone();
    let tt = parent.tt.clone();
    let stop = parent.stop.clone();
    let thread_pool = parent.thread_pool.clone();
    let selectivity_lv = parent.selectivity_lv;
    let mut searchings = parent.searchings.clone();
    searchings.push(split_searching.clone());
    let helper_chain = parent.helper_chain.clone();

    Box::new(move || {
        let mut stats = crate::search::search::SearchStats::default();
        let mut search = SearchContext::new(evaluator, mpc_config, tt, &mut stats)
            .with_ordering_evaluator(ordering_evaluator)
            .with_stop(stop)
            .with_thread_pool(thread_pool)
            .with_searchings(searchings)
            .with_helper_chain(helper_chain);
        search.selectivity_lv = selectivity_lv;
        let score = -nws_final(&child_board, child_alpha, &mut search);
        let aborted = search.is_aborted();
        drop(search);
        if !aborted && score >= cutoff_score {
            split_searching.store(false, Ordering::Relaxed);
        }
        TaskResult {
            score,
            move_index,
            stats,
            aborted,
        }
    })
}

pub(crate) fn collect_ybwc_tasks(
    handles: Vec<TaskHandle>,
    search: &mut SearchContext,
) -> Vec<TaskResult> {
    let pool = search.thread_pool.clone();
    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        let result = match pool.as_deref() {
            Some(pool) => pool.join_helping(handle),
            None => handle.join(),
        };
        search.stats.add_assign(result.stats);
        if result.aborted {
            search.stats.ybwc_split_aborts += 1;
        }
        results.push(result);
    }
    results
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::final_search::solve_score::solve_score;
    use crate::search::mpc::MpcConfig;
    use crate::search::search::SearchStats;
    use crate::t_table::TranspositionTable;
    use std::sync::{Arc, Mutex};

    fn shared_resources() -> (Arc<Evaluator>, Arc<MpcConfig>, Arc<TranspositionTable>) {
        (
            Arc::new(Evaluator::default()),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
        )
    }

    fn brute_force(board: &Board) -> i32 {
        let moves = board.moves();
        if moves == 0 {
            if board.opponent_moves() == 0 {
                return solve_score(board);
            }
            return -brute_force(&board.passed());
        }
        let mut best = -SCORE_MAX;
        let mut bits = moves;
        while bits != 0 {
            let m = bits & bits.wrapping_neg();
            bits ^= m;
            best = best.max(-brute_force(&board.make_move(m)));
        }
        best
    }

    fn next_pseudo_random(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
    }

    fn make_board_with_empties(rng: &mut u64, n_empties: u32) -> Board {
        let mut empties = 0u64;
        while empties.count_ones() < n_empties {
            let pos = (next_pseudo_random(rng) % 64) as u32;
            empties |= 1u64 << pos;
        }
        let player = next_pseudo_random(rng) & !empties;
        Board {
            player,
            opponent: !player & !empties,
        }
    }

    /// NWS の正しさを検証するヘルパー。
    ///
    /// `brute_force` で真スコアを求めてから、`alpha = true_score - 1` と `alpha = true_score`
    /// で NWS を呼ぶ。
    /// - `alpha = T-1` → beta=T → true_score ≥ T=beta → fail-high → result ≥ beta=T
    /// - `alpha = T`   → beta=T+1 → true_score=T ≤ alpha=T → fail-low → result ≤ T
    fn check_nws<F>(
        board: &Board,
        ev: &Arc<Evaluator>,
        mpc: &Arc<MpcConfig>,
        tt: &Arc<TranspositionTable>,
        mut nws: F,
    ) where
        F: FnMut(&Board, i32, &mut SearchContext) -> i32,
    {
        let true_score = brute_force(board);

        // fail-high check
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
        let result = nws(board, true_score - 1, &mut search);
        assert!(
            result >= true_score,
            "fail-high broken: alpha={} result={result} true={true_score} (player={:#018x})",
            true_score - 1,
            board.player,
        );

        // fail-low check
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(ev.clone(), mpc.clone(), tt.clone(), &mut stats);
        let result = nws(board, true_score, &mut search);
        assert!(
            result <= true_score,
            "fail-low broken: alpha={true_score} result={result} true={true_score} (player={:#018x})",
            board.player,
        );
    }

    /// nws_final_simple: ランダム 6〜9 マス空き盤面で NWS 単調性を確認する。
    #[test]
    fn nws_final_simple_nws_property() {
        let (ev, mpc, tt) = shared_resources();
        let mut rng = 0xabcd_ef01_2345_6789_u64;

        for _ in 0..200 {
            let extra = (next_pseudo_random(&mut rng) % 4) as u32;
            let board = make_board_with_empties(&mut rng, 6 + extra);
            check_nws(&board, &ev, &mpc, &tt, |b, alpha, s| {
                nws_final_simple(b, alpha, s)
            });
        }
    }

    /// nws_final: ランダム 6〜9 マス空き盤面で NWS 単調性を確認する(同じ盤面に TT を活用)。
    #[test]
    fn nws_final_nws_property() {
        let (ev, mpc, tt) = shared_resources();
        let mut rng = 0x9876_5432_10fe_dcba_u64;

        for _ in 0..200 {
            let extra = (next_pseudo_random(&mut rng) % 4) as u32;
            let board = make_board_with_empties(&mut rng, 6 + extra);
            check_nws(&board, &ev, &mpc, &tt, |b, alpha, s| nws_final(b, alpha, s));
        }
    }

    #[test]
    fn local_tt_does_not_reuse_selective_bound_for_exact_search() {
        let (ev, mpc, tt) = shared_resources();
        let board = Board {
            player: 0xbc61_9192_4c6b_1c86,
            opponent: 0x431e_6e24_2314_e159,
        };
        let n_empties = board.empties_count() as i32;
        let mut local_tt = [EMPTY_LOCAL_TT_ENTRY; LOCAL_TT_SIZE * LOCAL_TT_LAYERS];
        let move_bit = board.moves() & board.moves().wrapping_neg();
        let child = board.make_move(move_bit);
        let entry = &mut local_tt[local_tt_index(&child, n_empties - 1)];
        *entry = LocalTTEntry {
            player: child.player,
            opponent: child.opponent,
            lower: 64,
            upper: 64,
            selectivity_lv: 0,
        };

        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(ev, mpc, tt, &mut stats);
        search.selectivity_lv = crate::search::mpc::SELECTIVITY_LV_MAX;
        let true_score = brute_force(&board);
        let actual = nws_final_simple_impl(
            &board,
            true_score,
            LEGAL_UNDEFINED,
            n_empties,
            &mut search,
            &mut local_tt,
        );

        assert!(actual <= true_score);
    }
}
