use crate::eval::Evaluator;
use crate::search::mpc::MpcConfig;
use crate::search::thread_pool::{DetachedJob, HelperSlot, ThreadPool};
use crate::t_table::TranspositionTable;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const NO_MPC_SELECTIVITY_LV: i32 = 6;
const STOP_CHECK_NODE_MASK: u64 = 0x01ff;

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
    pub searchings: Vec<Arc<AtomicBool>>,
    /// 祖先の分割点の受け口。末尾が最も深い。
    ///
    /// 分割時にここを末尾から辿って待機中の master を探し、
    /// 見つかればその master へ仕事を直接渡す。
    pub(crate) helper_chain: Vec<Arc<HelperSlot>>,
    pub(crate) stop: Option<Arc<AtomicBool>>,
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
            searchings: Vec::new(),
            helper_chain: Vec::new(),
            stop: None,
            aborted: false,
        }
    }

    pub fn with_stop(mut self, stop: Option<Arc<AtomicBool>>) -> Self {
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

    pub fn with_searchings(mut self, searchings: Vec<Arc<AtomicBool>>) -> Self {
        self.searchings = searchings;
        self
    }

    pub(crate) fn with_helper_chain(mut self, helper_chain: Vec<Arc<HelperSlot>>) -> Self {
        self.helper_chain = helper_chain;
        self
    }

    /// 今この瞬間に仕事を渡せる先がありそうか。ロックを取らない概算。
    ///
    /// 探索ループの中で毎回呼ぶため、ここで弾ければジョブの構築自体を省ける。
    #[inline(always)]
    pub(crate) fn can_spawn_split_job(&self) -> bool {
        self.helper_chain.iter().any(|slot| slot.is_waiting())
            || self
                .thread_pool
                .as_deref()
                .is_some_and(|pool| pool.has_queue_room())
    }

    /// 分割点の仕事を、待機中の祖先か、居なければワーカープールへ渡す。
    ///
    /// 近い祖先から順に試す。どこにも渡せなければ仕事をそのまま返し、
    /// 呼び出し側 (master) がその手を自分で探索する。
    pub(crate) fn spawn_split_job(&mut self, mut job: DetachedJob) -> Result<(), DetachedJob> {
        for slot in self.helper_chain.iter().rev() {
            match slot.try_offer(job) {
                Ok(()) => {
                    self.stats.ybwc_handoffs += 1;
                    return Ok(());
                }
                Err(returned) => job = returned,
            }
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
        if self.aborted {
            return true;
        }
        let nodes = self.stats.eval_search_nodes + self.stats.final_search_nodes;
        if nodes & STOP_CHECK_NODE_MASK != 0 {
            return false;
        }
        self.check_abort_now()
    }

    /// ノード間引きをせず、stopとYBWC祖先の中断状態を直ちに確認する。
    /// タスク回収直後など、子が中断を報告した箇所で使う。
    #[inline(always)]
    pub(crate) fn check_abort_now(&mut self) -> bool {
        if self.aborted {
            return true;
        }
        if self
            .stop
            .as_ref()
            .is_some_and(|stop| stop.load(Ordering::Relaxed))
            || self
                .searchings
                .iter()
                .any(|searching| !searching.load(Ordering::Relaxed))
        {
            self.aborted = true;
        }
        self.aborted
    }

    /// 現在の分割点で兄弟がcutoffしたことだけが中断理由ならmaster探索を再開する。
    /// 外部stopまたは祖先分割の中断は解除しない。
    pub(crate) fn recover_from_split_abort(&mut self, split: &Arc<AtomicBool>) -> bool {
        if !self.aborted || split.load(Ordering::Acquire) {
            return false;
        }
        if self
            .stop
            .as_ref()
            .is_some_and(|stop| stop.load(Ordering::Acquire))
            || self
                .searchings
                .iter()
                .any(|searching| !searching.load(Ordering::Acquire))
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
