use wasm_bindgen::prelude::*;

// wasm console.log()を実行
// https://rustwasm.github.io/wasm-bindgen/examples/console-log.html

// First up let's take a look of binding `console.log` manually, without the
// help of `web_sys`. Here we're writing the `#[wasm_bindgen]` annotations
// manually ourselves, and the correctness of our program relies on the
// correctness of these annotations!

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);

    // The `console.log` is quite polymorphic, so we can bind it with multiple
    // signatures. Note that we need to use `js_name` to ensure we always call
    // `log` in JS.
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_u32(a: u32);

    // Multiple arguments too!
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_many(a: &str, b: &str);
}

// Next let's define a macro that's like `println!`, only it works for
// `console.log`. Note that `println!` doesn't actually work on the wasm target
// because the standard library currently just eats all output. To get
// `println!`-like behavior in your app you'll likely want a macro like this.

macro_rules! console_log {
    // Note that this is using the `log` function imported above during
    // `bare_bones`
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen]
pub fn add(a: i32, b: i32) -> i32 {
    console_log!("{}", a+b);
    a + b
}

use deft_reversi_engine::*;
use serde::{Serialize, Deserialize};

// Rust から Javascript に、structを渡す。
//https://rustwasm.github.io/docs/wasm-bindgen/reference/arbitrary-data-with-serde.html
// https://rustwasm.github.io/wasm-bindgen/reference/arbitrary-data-with-serde.html#serializing-and-deserializing-arbitrary-data-into-and-from-jsvalue-with-serde
#[derive(Serialize, Deserialize)]
struct JsBoard {
    board: Vec<Vec<i32>>,
    next_turn: i32
}

#[wasm_bindgen]
pub struct App {
    bm: BoardManager,
    level: i32,
    evaluator: Option<Evaluator>,
    t_table: Option<TranspositionTable>
}



#[wasm_bindgen]
impl App {
    #[allow(clippy::new_without_default)]
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let t_table = TranspositionTable::new();
        Self {
            bm: BoardManager::new(),
            level: 1,
            evaluator: None,
            t_table: Some(t_table)
        }
    }
    #[wasm_bindgen]
    pub fn set_evaluator(&mut self, eval_string: &str) {
        match Evaluator::read_string(eval_string) {
            Ok(e) => {
                self.evaluator = Some(e);
            }
            Err(e) => {
                console_log!("Evaluatorを読み込む際にエラーが置きました。{}",e)
            }
        }
    }


    #[wasm_bindgen]
    pub fn get_board(&self) -> Result<JsValue, serde_wasm_bindgen::Error> {
        let b = self.bm.current_board();
        let put_able = b.put_able();
        let mut js_b = JsBoard{
            board: vec![vec![0;8]; 8],
            next_turn: 0
        };
        for y in 0..8 {
            for x in 0..8 {
                let mask = 1u64 << (y * 8 + x);
                if mask & b.bit_board[Board::BLACK] != 0 {
                    js_b.board[y][x] = 1;
                } else if mask & b.bit_board[Board::WHITE] != 0{
                    js_b.board[y][x] = 2;
                } else if put_able & mask != 0 {
                    js_b.board[y][x] = 3;
                }
            }
        }

        //https://rustwasm.github.io/wasm-bindgen/reference/arbitrary-data-with-serde.html#serializing-and-deserializing-arbitrary-data-into-and-from-jsvalue-with-serde
        serde_wasm_bindgen::to_value(&js_b)
    }

    #[wasm_bindgen]
    pub fn put(&mut self, y: i32, x: i32) -> bool {
        let mut b = self.bm.current_board();
        let re = b.put_piece_from_coord(y, x);
        if re.is_err() {
            return false;
        }
        self.bm.add(b);
        true
    }


    #[wasm_bindgen]
    pub fn ai_put(&mut self) {
        let mut b = self.bm.current_board();

        let evaluator = self.evaluator.as_mut().unwrap();
        let t_table = self.t_table.as_mut().unwrap();

        let re = 
            if self.level == 0 {
                put_random_piece(&mut b)
            } else {
                let put_place_mask = solve_put_place(&b,self.level, evaluator,t_table);
                b.put_piece(put_place_mask)
            };

        if re.is_ok() {
            self.bm.add(b);
        }
    }

    #[wasm_bindgen]
    pub fn is_no_put_place(&self) -> bool {
        let b = self.bm.current_board();
        b.put_able() == 0
    }

    #[wasm_bindgen]
    pub fn is_end_game(&self) -> bool {
        let mut b = self.bm.current_board();
        let is_player_cant_put = b.put_able() == 0;
        b.next_turn ^= 1;
        let is_opponent_cant_put = b.put_able() == 0;
        is_player_cant_put && is_opponent_cant_put
    }

    #[wasm_bindgen]
    pub fn pass(&mut self){
        let mut b = self.bm.current_board();
        b.next_turn ^= 1;
        self.bm.undo();
        self.bm.add(b);
        console_log!("passed");
    }

    pub fn set_level(&mut self, l: i32){
        self.level = l;
        console_log!("set level {}", l);
    }
}

fn solve_put_place(
    b                     : &Board,
    level                 : i32,
    evaluator             : &mut Evaluator,
    t_table               : &mut TranspositionTable,
) -> u64
{
    let n_empties = b.empties_count();
    let switch_winning_solver = 0;
    let (eval_solver_lv, switch_perfect_solver, selectivity_lv)  = 
        match level {
            1..=10 => {
                (level, level*2, 0)
            },
            11..=12 => {
                match n_empties {
                    0..=20 => (level, 22, 0),
                    21 => (level, 22, 3),
                    22 => (level, 22, 3),
                    44..=60 => (10, 22, 0),
                    _ => (level, 22, 4),
                }
            },
            13..=14=> {
                match n_empties {
                    0..=20 => ( level, 22, 0),
                    21 => ( level, 22, 2),
                    22 => ( level, 22, 2),
                    44..=60 => (12, 22, 1),
                    _ => ( level, 22, 4)
                }                
            },
            15..=16 => {
                match n_empties {
                    0..=20 => (level, 24, 0),
                    21 => (level, 24, 1),
                    22 => (level, 24, 1),
                    23 => (level, 24, 3),
                    24 => (level, 24, 3),
                    44..=54 => (level - 3, 24, 1),
                    55..=60 => (level - 4, 24, 1), 
                    _ => (level, 24, 4)
                }
            },
            17..=18 => {
                 match n_empties {
                    0..=20 => (level, 24, 0),
                    21..=22 => (level, 24, 2),
                    23..=24 => (level, 24, 2),
                    44..=54 => (level - 3, 24, 1),
                    55..=60 => (level - 4, 24, 1), 
                    _ => (level, 24, 4)
                }
            },
            19..=24 => {
                match n_empties {
                    0..=22 => (level, 26, 0),
                    23 => (level, 26, 1),
                    24 => (level, 26, 1),
                    25 => (level, 26, 3),
                    26 => (level, 26, 3),
                    44..=54 => (level - 4, 26, 1),
                    55..=60 => (level - 5, 26, 1),                    
                    _ => (level, 26, 4),
                }
            },
            _ => (level, level*2, 0)
        };

    let solver_result = 
        if n_empties <= switch_perfect_solver {
            console_log!("perfect solver");
            console_log!("n_empties: {}", b.empties_count());
            console_log!("selectivity: {}", selectivity_lv);
            perfect_solver(b, false, selectivity_lv,t_table, evaluator)
        } else if n_empties <= switch_winning_solver {
            console_log!("winning solver");
            winning_solver(b, false,t_table, evaluator)
        } else {
            console_log!("eval solver");
            console_log!("move count: {}", b.move_count()+1);
            console_log!("n_empties: {}", b.empties_count());
            console_log!("Lv: {}", eval_solver_lv);
            console_log!("selectivity: {}", selectivity_lv);
            eval_solver(b, eval_solver_lv, selectivity_lv, false,t_table, evaluator)
        };
    match solver_result {
        Ok(solve_result) => {
            console_log!("評価値: {}", solve_result.eval);
            console_log!("-----------------");
            solve_result.best_move
        },
        Err(_) => {
            console_log!("Err: solver is stoped");
            panic!()
        }
    }
}