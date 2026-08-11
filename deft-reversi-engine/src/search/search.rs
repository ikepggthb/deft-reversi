use crate::eval::Evaluator;
use crate::search::mpc::MpcConfig;
use crate::t_table::TranspositionTable;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const NO_MPC_SELECTIVITY_LV: i32 = 6;

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

    pub fn with_pv_tt(mut self, pv_tt: Arc<TranspositionTable>, root_empties: i32) -> Self {
        self.pv_tt = Some(pv_tt);
        self.pv_tt_min_empties = (root_empties - 4).max(0);
        self
    }

    #[inline(always)]
    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    #[inline(always)]
    pub fn check_abort(&mut self) -> bool {
        self.check_abort_now()
    }

    #[inline(always)]
    pub(crate) fn check_abort_now(&mut self) -> bool {
        if self.aborted {
            return true;
        }
        if self
            .stop
            .as_ref()
            .is_some_and(|stop| stop.load(Ordering::Acquire))
        {
            self.aborted = true;
        }
        self.aborted
    }
}
