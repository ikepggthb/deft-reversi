use crate::eval::Evaluator;
use crate::search::mpc::MpcConfig;
use crate::t_table::TranspositionTable;
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
}

/// 評価探索で共有する不変資源と探索設定。
///
/// board や alpha/beta/depth のようなノードごとに変わる状態は持たない。
pub struct SearchContext<'a> {
    pub evaluator: Arc<Evaluator>,
    pub mpc_config: Arc<MpcConfig>,
    pub tt: Arc<TranspositionTable>,
    pub stats: &'a mut SearchStats,
    pub selectivity_lv: i32,
    pub final_search_empties: i32,
}

impl<'a> SearchContext<'a> {
    pub fn new(
        evaluator: Arc<Evaluator>,
        mpc_config: Arc<MpcConfig>,
        tt: Arc<TranspositionTable>,
        stats: &'a mut SearchStats,
    ) -> Self {
        Self {
            evaluator,
            mpc_config,
            tt,
            stats,
            selectivity_lv: NO_MPC_SELECTIVITY_LV,
            final_search_empties: 0,
        }
    }
}
