pub mod count_last_flip;
mod leaf;
// pub mod move_iterator;
// pub mod move_list;
pub mod negaalpha;
pub mod nws;
pub mod pvs;

pub mod solve_score {
    pub(crate) use crate::search::final_search::leaf::{
        final_parity, solve_score_1_empties, solve_score_2_empties, solve_score_3_empties,
        solve_score_4_empties,
    };
    pub use crate::search::solve_score::solve_score;
}

pub(crate) use nws::nws_final;
pub(crate) use pvs::pvs_final;
pub(crate) use solve_score::solve_score;
