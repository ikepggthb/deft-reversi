mod eval_simple;
mod board;
mod bit;
mod t_table;
mod search;
mod perfect_search;
mod eval_search;
mod solver;
mod game;
mod console;
mod eval;
// mod learn;
mod ffo_test;
mod eval_for_learn;
mod mpc;

// mod game;
// ---


use ffo_test::*;
// use learn::*;
use eval::*;

// use console::*;


fn main () {
    // npc_perfect_learn();
    // npc_learn(10);
    // learning();
    ffo_test();
    // console_game();
}