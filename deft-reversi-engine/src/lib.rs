pub mod board;
pub mod eval_simple;
pub mod perfect_search;
pub mod eval_search;
pub mod solver;
pub mod game;
mod bit;
pub mod cut_off;
mod t_table;
mod eval;
mod mpc;
mod human_book;
mod count_last_flip;
mod get_moves;
mod flip;
mod move_list;
// ---

pub use board::*;
pub use eval_simple::*;
pub use solver::*;
pub use game::*;
pub use eval::*;
pub use t_table::*;
pub use human_book::*;
pub use cut_off::*;
pub use mpc::{SELECTIVITY, SELECTIVITY_LV_MAX, N_SELECTIVITY_LV, NO_MPC};
pub use move_list::*;


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