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
// ---

pub use board::*;
pub use eval_simple::*;
pub use solver::*;
pub use game::*;
pub use eval::*;
pub use t_table::*;



