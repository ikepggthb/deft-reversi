use crate::eval::Evaluator;
use crate::search::mpc::MpcConfig;
use crate::search::thread_pool::{DetachedJob, HelperSlot, ThreadPool};
use crate::t_table::TranspositionTable;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

pub const NO_MPC_SELECTIVITY_LV: i32 = 6;

/// 1つの探索文脈だけがホットパスで読む中断フラグ。
///
/// `flag` は false -> true の単調な通知であり、探索結果の publish には使わない。
/// 子の登録は `children` のロック中に親フラグを再確認するため、伝播とジョブ生成が
/// 競合しても新しい子が中断を取り逃さない。
#[repr(align(64))]
pub(crate) struct AbortNode {
    flag: AtomicBool,
    parent: Option<Arc<AbortNode>>,
    children: Mutex<Vec<Weak<AbortNode>>>,
}

struct StopWatcher {
    done: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl StopWatcher {
    fn new(stop: Arc<AtomicBool>, root: Arc<AbortNode>) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        if stop.load(Ordering::Relaxed) {
            root.abort_subtree();
        }
        let watcher_done = done.clone();
        let handle = std::thread::spawn(move || {
            while !watcher_done.load(Ordering::Relaxed) {
                if stop.load(Ordering::Relaxed) {
                    root.abort_subtree();
                    return;
                }
                std::thread::sleep(Duration::from_micros(25));
            }
        });
        Self {
            done,
            handle: Some(handle),
        }
    }
}

impl Drop for StopWatcher {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
    }
}

impl AbortNode {
    pub(crate) fn root() -> Arc<Self> {
        Arc::new(Self {
            flag: AtomicBool::new(false),
            parent: None,
            children: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn child(parent: &Arc<Self>) -> Arc<Self> {
        let child = Arc::new(Self {
            flag: AtomicBool::new(false),
            parent: Some(parent.clone()),
            children: Mutex::new(Vec::new()),
        });
        let mut children = parent.children.lock().unwrap();
        children.retain(|weak| weak.strong_count() != 0);
        children.push(Arc::downgrade(&child));
        // abort_subtree は flag を立ててから children をロックする。この再確認を
        // 同じロック内で行うことで、伝播と登録のどちらが先でも通知を失わない。
        if parent.flag.load(Ordering::Relaxed) {
            child.abort_subtree();
        }
        child
    }

    #[inline(always)]
    pub(crate) fn is_aborted(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    pub(crate) fn abort_subtree(&self) {
        // 複数の兄弟がほぼ同時に cutoff しても、木を歩くのは最初の1つだけ。
        if self.flag.swap(true, Ordering::Relaxed) {
            return;
        }
        let mut children = self.children.lock().unwrap();
        children.retain(|weak| {
            if let Some(child) = weak.upgrade() {
                child.abort_subtree();
                true
            } else {
                false
            }
        });
    }

    fn has_aborted_ancestor(&self) -> bool {
        let mut node = self.parent.as_deref();
        while let Some(current) = node {
            if current.flag.load(Ordering::Relaxed) {
                return true;
            }
            node = current.parent.as_deref();
        }
        false
    }
}

/// 祖先の helper 受け口をコピー無しで共有する永続リスト。
pub(crate) struct HelperNode {
    slot: Arc<HelperSlot>,
    parent: Option<Arc<HelperNode>>,
}

impl HelperNode {
    fn child(parent: Option<Arc<Self>>, slot: Arc<HelperSlot>) -> Arc<Self> {
        Arc::new(Self { slot, parent })
    }
}

/// 探索統計。
///
/// 最初は観測に必要な最小限だけ持つ。
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchStats {
    pub final_search_nodes: u64,
    pub eval_search_nodes: u64,
    pub final_search_leaf_nodes: u64,
    pub eval_search_leaf_nodes: u64,
    pub tt_hits: u64,
    pub tt_cuts: u64,
    pub mpc_tries: u64,
    pub mpc_cuts: u64,
    pub mpc_high_cuts: u64,
    pub mpc_low_cuts: u64,
    pub stability_tries: u64,
    pub stability_cuts: u64,
    pub ybwc_splits: u64,
    pub ybwc_split_aborts: u64,
    /// 待機中の祖先へ直接渡せた回数。
    pub ybwc_handoffs: u64,
    /// 祖先に渡せずワーカープールへ積んだ回数。
    pub ybwc_pool_pushes: u64,
    /// どこにも渡せず master が自分で探索した回数。
    pub ybwc_spawn_failures: u64,
}

/// 評価探索で共有する不変資源と探索設定。
///
/// board や alpha/beta/depth のようなノードごとに変わる状態は持たない。
pub struct SearchContext<'a> {
    pub evaluator: Arc<Evaluator>,
    pub ordering_evaluator: Arc<Evaluator>,
    pub mpc_config: Arc<MpcConfig>,
    pub tt: Arc<TranspositionTable>,
    pub pv_tt: Option<Arc<TranspositionTable>>,
    pub pv_tt_min_empties: i32,
    pub stats: &'a mut SearchStats,
    pub selectivity_lv: i32,
    pub thread_pool: Option<Arc<ThreadPool>>,
    pub(crate) abort_node: Arc<AbortNode>,
    /// 祖先の分割点の受け口。末尾が最も深い。
    ///
    /// 分割時にここを末尾から辿って待機中の master を探し、
    /// 見つかればその master へ仕事を直接渡す。
    pub(crate) helper_chain: Option<Arc<HelperNode>>,
    pub(crate) stop: Option<Arc<AtomicBool>>,
    stop_watcher: Option<StopWatcher>,
    aborted: bool,
}

impl<'a> SearchContext<'a> {
    pub fn new(
        evaluator: Arc<Evaluator>,
        mpc_config: Arc<MpcConfig>,
        tt: Arc<TranspositionTable>,
        stats: &'a mut SearchStats,
    ) -> Self {
        Self {
            ordering_evaluator: evaluator.clone(),
            evaluator,
            mpc_config,
            tt,
            pv_tt: None,
            pv_tt_min_empties: i32::MAX,
            stats,
            selectivity_lv: NO_MPC_SELECTIVITY_LV,
            thread_pool: None,
            abort_node: AbortNode::root(),
            helper_chain: None,
            stop: None,
            stop_watcher: None,
            aborted: false,
        }
    }

    pub fn with_stop(mut self, stop: Option<Arc<AtomicBool>>) -> Self {
        if let Some(stop) = stop {
            self.stop_watcher = Some(StopWatcher::new(stop.clone(), self.abort_node.clone()));
            self.stop = Some(stop);
        }
        self
    }

    /// 子ジョブは root の watcher が行う伝播を受ける。同じ外部フラグは、低頻度の
    /// split-abort 回復判定にだけ引き継ぐ。
    pub(crate) fn with_inherited_stop(mut self, stop: Option<Arc<AtomicBool>>) -> Self {
        self.stop = stop;
        self
    }

    pub fn with_ordering_evaluator(mut self, ev: Arc<Evaluator>) -> Self {
        self.ordering_evaluator = ev;
        self
    }

    pub fn with_thread_pool(mut self, thread_pool: Option<Arc<ThreadPool>>) -> Self {
        self.thread_pool = thread_pool;
        self
    }

    pub fn with_pv_tt(mut self, pv_tt: Arc<TranspositionTable>, root_empties: i32) -> Self {
        self.pv_tt = Some(pv_tt);
        self.pv_tt_min_empties = (root_empties - 4).max(0);
        self
    }

    pub(crate) fn with_abort_node(mut self, abort_node: Arc<AbortNode>) -> Self {
        self.abort_node = abort_node;
        self
    }

    pub(crate) fn with_helper_chain(mut self, helper_chain: Option<Arc<HelperNode>>) -> Self {
        self.helper_chain = helper_chain;
        self
    }

    pub(crate) fn push_abort_node(&mut self, abort_node: Arc<AbortNode>) -> Arc<AbortNode> {
        std::mem::replace(&mut self.abort_node, abort_node)
    }

    pub(crate) fn restore_abort_node(&mut self, abort_node: Arc<AbortNode>) {
        self.abort_node = abort_node;
    }

    pub(crate) fn push_helper(&mut self, slot: Arc<HelperSlot>) {
        self.helper_chain = Some(HelperNode::child(self.helper_chain.clone(), slot));
    }

    /// 自分の祖先チェーンの末尾に `slot` を足したチェーンを作る。
    /// 分割で投げる子ジョブに渡すため、自分自身の chain は変えない。
    pub(crate) fn helper_chain_with(&self, slot: Arc<HelperSlot>) -> Arc<HelperNode> {
        HelperNode::child(self.helper_chain.clone(), slot)
    }

    pub(crate) fn pop_helper(&mut self) {
        self.helper_chain = self
            .helper_chain
            .as_ref()
            .and_then(|node| node.parent.clone());
    }

    /// 今この瞬間に仕事を渡せる先がありそうか。ロックを取らない概算。
    ///
    /// 探索ループの中で毎回呼ぶため、ここで弾ければジョブの構築自体を省ける。
    #[inline(always)]
    pub(crate) fn can_spawn_split_job(&self) -> bool {
        let mut helper = self.helper_chain.as_deref();
        while let Some(node) = helper {
            if node.slot.is_waiting() {
                return true;
            }
            helper = node.parent.as_deref();
        }
        self.thread_pool
            .as_deref()
            .is_some_and(|pool| pool.has_queue_room())
    }

    /// 分割点の仕事を、待機中の祖先か、居なければワーカープールへ渡す。
    ///
    /// 近い祖先から順に試す。どこにも渡せなければ仕事をそのまま返し、
    /// 呼び出し側 (master) がその手を自分で探索する。
    pub(crate) fn spawn_split_job(&mut self, mut job: DetachedJob) -> Result<(), DetachedJob> {
        let mut helper = self.helper_chain.clone();
        while let Some(node) = helper {
            match node.slot.try_offer(job) {
                Ok(()) => {
                    self.stats.ybwc_handoffs += 1;
                    return Ok(());
                }
                Err(returned) => job = returned,
            }
            helper = node.parent.clone();
        }
        match self.thread_pool.as_deref() {
            Some(pool) => {
                let result = pool.try_push_detached(job);
                if result.is_ok() {
                    self.stats.ybwc_pool_pushes += 1;
                } else {
                    self.stats.ybwc_spawn_failures += 1;
                }
                result
            }
            None => {
                self.stats.ybwc_spawn_failures += 1;
                Err(job)
            }
        }
    }

    #[inline(always)]
    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    #[inline(always)]
    pub fn check_abort(&mut self) -> bool {
        self.check_abort_now()
    }

    /// 自分専用のフラグだけを読む。外部 stop は root の watcher が同じ木へ
    /// 伝播するため、ホットパスに追加の共有 atomic load はない。
    #[inline(always)]
    pub(crate) fn check_abort_now(&mut self) -> bool {
        if self.aborted {
            return true;
        }
        if self.abort_node.is_aborted() {
            self.aborted = true;
        }
        self.aborted
    }

    /// 現在の分割点で兄弟がcutoffしたことだけが中断理由ならmaster探索を再開する。
    /// 外部stopまたは祖先分割の中断は解除しない。
    pub(crate) fn recover_from_split_abort(&mut self, split: &Arc<AbortNode>) -> bool {
        if !self.aborted || !split.is_aborted() {
            return false;
        }
        if self
            .stop
            .as_ref()
            .is_some_and(|stop| stop.load(Ordering::Acquire))
            || split.has_aborted_ancestor()
        {
            return false;
        }
        self.aborted = false;
        true
    }
}

impl SearchStats {
    pub fn add_assign(&mut self, other: SearchStats) {
        self.final_search_nodes += other.final_search_nodes;
        self.eval_search_nodes += other.eval_search_nodes;
        self.final_search_leaf_nodes += other.final_search_leaf_nodes;
        self.eval_search_leaf_nodes += other.eval_search_leaf_nodes;
        self.tt_hits += other.tt_hits;
        self.tt_cuts += other.tt_cuts;
        self.mpc_tries += other.mpc_tries;
        self.mpc_cuts += other.mpc_cuts;
        self.mpc_high_cuts += other.mpc_high_cuts;
        self.mpc_low_cuts += other.mpc_low_cuts;
        self.stability_tries += other.stability_tries;
        self.stability_cuts += other.stability_cuts;
        self.ybwc_splits += other.ybwc_splits;
        self.ybwc_split_aborts += other.ybwc_split_aborts;
        self.ybwc_handoffs += other.ybwc_handoffs;
        self.ybwc_pool_pushes += other.ybwc_pool_pushes;
        self.ybwc_spawn_failures += other.ybwc_spawn_failures;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    const DEADLINE: Duration = Duration::from_secs(20);

    fn context<'a>(stats: &'a mut SearchStats) -> SearchContext<'a> {
        SearchContext::new(
            Arc::new(Evaluator::default()),
            Arc::new(MpcConfig::default()),
            Arc::new(TranspositionTable::new()),
            stats,
        )
    }

    #[test]
    fn abort_propagates_to_existing_and_late_children() {
        let root = AbortNode::root();
        let child = AbortNode::child(&root);
        let grandchild = AbortNode::child(&child);

        root.abort_subtree();
        assert!(root.is_aborted());
        assert!(child.is_aborted());
        assert!(grandchild.is_aborted());

        let late_child = AbortNode::child(&child);
        assert!(late_child.is_aborted(), "abort 後に登録した子も中断される");
    }

    #[test]
    fn split_abort_is_recoverable_but_ancestor_abort_is_not() {
        let mut stats = SearchStats::default();
        let mut search = context(&mut stats);
        let root = search.abort_node.clone();
        let split = AbortNode::child(&root);
        let worker = AbortNode::child(&split);

        let parent = search.push_abort_node(worker);
        split.abort_subtree();
        assert!(search.check_abort_now());
        search.restore_abort_node(parent);
        assert!(search.recover_from_split_abort(&split));
        assert!(!search.is_aborted());

        let next_split = AbortNode::child(&root);
        let next_worker = AbortNode::child(&next_split);
        let parent = search.push_abort_node(next_worker);
        root.abort_subtree();
        assert!(search.check_abort_now());
        search.restore_abort_node(parent);
        assert!(!search.recover_from_split_abort(&next_split));
    }

    /// 待機中の祖先がいれば、ワーカープールではなくそちらへ直接渡す。
    ///
    /// 実際の探索でこの経路が踏まれるのは分割点の入れ子が深いときだけで
    /// (4コアの FFO40-49 で投入試行の 4.1%、18空きの局面では 0%)、
    /// 探索経由のテストでは安定して再現できないため、ここで直接確認する。
    #[test]
    fn split_job_is_handed_to_a_waiting_ancestor() {
        let slot = Arc::new(HelperSlot::new());
        let executed = Arc::new(AtomicBool::new(false));

        // 祖先の master 役。`join_slaves` の待機窓に入っている状態を作る。
        let master = {
            let slot = slot.clone();
            thread::spawn(move || {
                let deadline = Instant::now() + DEADLINE;
                loop {
                    if let Some(job) = slot.wait_or_take_job(1, Duration::from_millis(5)) {
                        job();
                        return true;
                    }
                    if Instant::now() >= deadline {
                        return false;
                    }
                }
            })
        };

        let mut stats = SearchStats::default();
        // thread_pool は None。渡せる先は helper_chain しか無いので、
        // Ok が返ったならハンドオフ経由だったことが確定する。
        let mut search = context(&mut stats);
        search.push_helper(slot.clone());

        let deadline = Instant::now() + DEADLINE;
        loop {
            if search.can_spawn_split_job() {
                let executed = executed.clone();
                let job: DetachedJob = Box::new(move || {
                    executed.store(true, Ordering::SeqCst);
                });
                if search.spawn_split_job(job).is_ok() {
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "待機中の祖先へ仕事を渡せなかった"
            );
            thread::yield_now();
        }
        drop(search);

        assert!(master.join().unwrap(), "master が仕事を受け取らなかった");
        assert!(
            executed.load(Ordering::SeqCst),
            "渡した仕事が実行されていない"
        );
        assert_eq!(stats.ybwc_handoffs, 1);
        assert_eq!(stats.ybwc_pool_pushes, 0);
        assert_eq!(stats.ybwc_spawn_failures, 0);
    }

    /// 渡せる祖先もプールも無ければ、仕事はそのまま返る (master が自分で探索する)。
    #[test]
    fn split_job_is_returned_when_there_is_nowhere_to_put_it() {
        let mut stats = SearchStats::default();
        let mut search = context(&mut stats);

        let executed = Arc::new(AtomicBool::new(false));
        let job: DetachedJob = {
            let executed = executed.clone();
            Box::new(move || executed.store(true, Ordering::SeqCst))
        };

        assert!(!search.can_spawn_split_job());
        let returned = search.spawn_split_job(job);
        assert!(returned.is_err(), "渡せないので仕事が返るはず");

        // 返ってきた仕事は失われていない。
        returned.unwrap_err()();
        drop(search);
        assert!(executed.load(Ordering::SeqCst));
        assert_eq!(stats.ybwc_spawn_failures, 1);
        assert_eq!(stats.ybwc_handoffs, 0);
    }
}
