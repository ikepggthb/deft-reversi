pub mod board;
pub mod eval_simple;
pub mod perfect_search;
pub mod eval_search;
pub mod solver;
pub mod game;
mod bit;
mod search;
mod t_table;
mod eval;
mod mpc;
mod human_book;
mod ffo_test;
mod count_last_flip;
// ---

pub use board::*;
pub use eval_simple::*;
pub use solver::*;
pub use game::*;
pub use eval::*;
pub use t_table::*;
pub use human_book::*;


#[cfg(test)]
mod tests {
    use super::*;
    pub use ffo_test::*;
    
    #[test]
    fn run () {
        // npc_perfect_learn();
        // npc_learn(10);
        // learning();
        ffo_test();
        // console_game();
    }
}