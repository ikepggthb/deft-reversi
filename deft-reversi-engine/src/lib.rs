pub mod board;
pub mod eval_simple;
pub mod perfect_search;
pub mod eval_search;
pub mod solver;
pub mod game;
mod bit;
pub mod search;
mod t_table;
mod eval;
mod mpc;
mod human_book;
mod count_last_flip;
// ---

pub use board::*;
pub use eval_simple::*;
pub use solver::*;
pub use game::*;
pub use eval::*;
pub use t_table::*;
pub use human_book::*;
pub use search::*;
pub use mpc::{SELECTIVITY, SELECTIVITY_LV_MAX, N_SELECTIVITY_LV, NO_MPC};


#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn run () {
        // npc_perfect_learn();
        // npc_learn(10);
        // learning();
        // console_game();
    }
}