//! YBWC の分割点。終盤 (`final_search::nws`) と中盤 (`eval_search::nws`) で共有する。
//!
//! master は最初の手を単独で探索したあと、残りの手を [`SplitPoint`] の共有作業
//! リストに載せる。slave は `next_work` で 1 手ずつ取り、`finish` で結果を書く。
//! master は自分も `next_work` を回しつつ、最後に [`SplitPoint::join_slaves`] で
//! spawn 済みの slave を待つ。
//!
//! 終盤と中盤の違いは「子局面をどう探索するか」だけなので、その部分は
//! 呼び出し側が組み立てた [`DetachedJob`] に閉じ込めてある。

use crate::board::board::Board;
use crate::eval::evaluator_const::SCORE_MAX;
use crate::search::search::{AbortNode, SearchContext, SearchStats};
use crate::search::thread_pool::{DetachedJob, HelperSlot};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 1 つの分割点で同時に走らせる slave の上限。
const YBWC_MAX_SLAVES: usize = 3;

/// master が待機中に「未実行の仕事がキューに残っていないか」を見に行く間隔。
/// slave の完了自体は condvar で即座に通知されるため、完了検知の遅延ではない。
const HELPER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_micros(50);

/// まだ誰も結果を書いていない手を表す番兵。取り得るスコアの外側の値。
const SPLIT_SCORE_UNSET: i32 = SCORE_MAX + 1;

/// 1 つの分割点。master と slave が `Arc` で共有する。
pub(crate) struct SplitPoint {
    /// `(元の move_list 上の index, 子局面)`。master が構築したあとは不変。
    work: Box<[(usize, Board)]>,
    /// 次に取り出す `work` の位置。`work.len()` を超えて増えることがある。
    next: AtomicUsize,
    /// `work_index` ごとの探索結果。未探索は [`SPLIT_SCORE_UNSET`]。
    scores: [AtomicI32; 64],
    /// beta cut/祖先中断を、この分割点以下の探索文脈へ伝播する。
    pub(crate) abort_node: Arc<AbortNode>,
    beta: i32,
    /// slave の完了を master へ伝える受け口。子孫からの仕事の受け口でもある。
    pub(crate) helper: Arc<HelperSlot>,
    /// slave が積み上げた探索統計。master が最後に自分へ加算する。
    slave_stats: Mutex<SearchStats>,
    /// いま走っている slave の数。終了したら減るので、その分だけ追加投入できる。
    active_slaves: AtomicUsize,
}

impl SplitPoint {
    pub(crate) fn new(
        work: Box<[(usize, Board)]>,
        beta: i32,
        abort_parent: &Arc<AbortNode>,
    ) -> Self {
        debug_assert!(work.len() <= 64);
        Self {
            work,
            next: AtomicUsize::new(0),
            scores: std::array::from_fn(|_| AtomicI32::new(SPLIT_SCORE_UNSET)),
            abort_node: AbortNode::child(abort_parent),
            beta,
            helper: Arc::new(HelperSlot::new()),
            slave_stats: Mutex::new(SearchStats::default()),
            active_slaves: AtomicUsize::new(0),
        }
    }

    pub(crate) fn beta(&self) -> i32 {
        self.beta
    }

    /// `(work_index, 元の move_list 上の index)` の列。master が結果を集める時に使う。
    pub(crate) fn work_indices(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.work
            .iter()
            .enumerate()
            .map(|(work_index, &(move_index, _))| (work_index, move_index))
    }

    /// `work_index` の探索結果。未探索なら `None`。
    pub(crate) fn score_at(&self, work_index: usize) -> Option<i32> {
        let score = self.scores[work_index].load(Ordering::Acquire);
        (score != SPLIT_SCORE_UNSET).then_some(score)
    }

    /// まだ誰にも割り当てられていない仕事が残っているか。
    #[inline(always)]
    fn has_unclaimed_work(&self) -> bool {
        !self.abort_node.is_aborted() && self.next.load(Ordering::Relaxed) < self.work.len()
    }

    /// slave を 1 つ追加できる状態か。
    #[inline(always)]
    fn can_add_slave(&self) -> bool {
        self.active_slaves.load(Ordering::Relaxed) < YBWC_MAX_SLAVES && self.has_unclaimed_work()
    }

    /// 次の仕事を 1 件取り出す。`(work_index, move_index, 子局面)`。
    #[inline(always)]
    pub(crate) fn next_work(&self) -> Option<(usize, usize, Board)> {
        if self.abort_node.is_aborted() {
            return None;
        }
        let work_index = self.next.fetch_add(1, Ordering::Relaxed);
        self.work
            .get(work_index)
            .map(|&(move_index, board)| (work_index, move_index, board))
    }

    /// 1 件の探索結果を書き込む。beta cut なら以降の取り出しを止める。
    #[inline(always)]
    pub(crate) fn finish(&self, work_index: usize, score: i32) {
        self.scores[work_index].store(score, Ordering::Release);
        if score >= self.beta {
            self.abort_node.abort_subtree();
        }
    }

    /// これ以上の探索を止める。beta cut と abort の両方で使う。
    #[inline(always)]
    pub(crate) fn stop_searching(&self) {
        self.abort_node.abort_subtree();
    }

    /// slave が 1 件終わったときに呼ぶ。統計を積んで master を起こす。
    pub(crate) fn slave_finished(&self, stats: SearchStats, aborted: bool) {
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
    pub(crate) fn join_slaves(&self, spawned: u64, search: &mut SearchContext) {
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
}

/// 空きが出ている間 slave を追加投入する。投入できた数だけ `spawned` を増やす。
///
/// 分割点に入った瞬間だけ投入すると、その時点で全スレッドが忙しい場合に
/// この分割点は最後まで master の直列探索になってしまう。
/// 後から空いたスレッドを拾えるよう、edax の `node_split` と同様に
/// master の探索ループの中で毎回呼ぶ。
pub(crate) fn try_add_slaves(
    split: &Arc<SplitPoint>,
    search: &mut SearchContext,
    spawned: &mut u64,
    make_job: impl Fn(&Arc<SplitPoint>, &SearchContext) -> DetachedJob,
) {
    while split.can_add_slave() && search.can_spawn_split_job() {
        let job = make_job(split, search);
        // 投入に失敗したら戻すので、先に予約しておく。
        split.active_slaves.fetch_add(1, Ordering::Relaxed);
        match search.spawn_split_job(job) {
            Ok(()) => {
                search.stats.ybwc_splits += 1;
                *spawned += 1;
            }
            Err(_) => {
                split.active_slaves.fetch_sub(1, Ordering::Relaxed);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::mpc::MpcConfig;
    use crate::t_table::TranspositionTable;
    use std::thread;

    fn make_split(n: usize, beta: i32) -> Arc<SplitPoint> {
        let work = (0..n)
            .map(|i| (i * 2, Board::new()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Arc::new(SplitPoint::new(work, beta, &AbortNode::root()))
    }

    /// 各仕事はちょうど 1 回だけ配られ、使い切ると `None` になる。
    #[test]
    fn each_work_item_is_handed_out_exactly_once() {
        let split = make_split(3, 100);
        let mut seen = Vec::new();
        while let Some((work_index, move_index, _)) = split.next_work() {
            seen.push((work_index, move_index));
        }
        assert_eq!(seen, vec![(0, 0), (1, 2), (2, 4)]);
        assert!(split.next_work().is_none());
    }

    /// beta 以上の結果が入ったら、以降は誰も仕事を取れない。
    #[test]
    fn beta_cutoff_stops_further_work() {
        let split = make_split(4, 10);
        let (work_index, _, _) = split.next_work().unwrap();
        split.finish(work_index, 10);
        assert!(
            split.next_work().is_none(),
            "cut 後に仕事を配ってはいけない"
        );
    }

    /// 結果を書くまでは未探索、書いたあとは読み出せる。
    #[test]
    fn score_at_reflects_finished_work_only() {
        let split = make_split(2, 100);
        assert_eq!(split.score_at(0), None);
        assert_eq!(split.score_at(1), None);

        split.finish(1, -7);
        assert_eq!(split.score_at(0), None);
        assert_eq!(split.score_at(1), Some(-7));
        assert_eq!(
            split.work_indices().collect::<Vec<_>>(),
            vec![(0, 0), (1, 2)]
        );
    }

    /// `join_slaves` は spawn した数だけ完了を待ち、統計を master へ移す。
    #[test]
    fn join_slaves_waits_for_every_slave_and_collects_stats() {
        let split = make_split(4, 100);
        let spawned = 3u64;

        let threads = (0..spawned)
            .map(|_| {
                let split = split.clone();
                thread::spawn(move || {
                    let stats = SearchStats {
                        final_search_nodes: 5,
                        ..SearchStats::default()
                    };
                    // 実際の slave と同じく active_slaves を予約してから終える。
                    split.active_slaves.fetch_add(1, Ordering::Relaxed);
                    split.slave_finished(stats, false);
                })
            })
            .collect::<Vec<_>>();

        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            Arc::new(Evaluator::default()),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
            &mut stats,
        );
        split.join_slaves(spawned, &mut search);
        drop(search);

        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(stats.final_search_nodes, 5 * spawned);
        assert_eq!(split.active_slaves.load(Ordering::Relaxed), 0);
    }

    /// spawn していなければ `join_slaves` は何も待たない。
    #[test]
    fn join_slaves_returns_immediately_when_nothing_was_spawned() {
        let split = make_split(2, 100);
        let mut stats = SearchStats::default();
        let mut search = SearchContext::new(
            Arc::new(Evaluator::default()),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
            &mut stats,
        );
        split.join_slaves(0, &mut search);
    }
}
