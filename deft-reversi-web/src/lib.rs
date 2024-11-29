use wasm_bindgen::prelude::*;

use deft_reversi_engine::*;
use serde::{Serialize, Deserialize};
use serde_wasm_bindgen::*;
use rand::Rng;

// use js_sys::{Promise, Error};
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

#[wasm_bindgen]
struct App {
    game: Game,
    solver: Solver,
    rng: rand::rngs::ThreadRng,
    first_move_played: bool
}


#[derive(Serialize, Deserialize)]
pub struct StateForJS {
    black: String,
    white: String,
    legal_moves: String,
    flipping: String,
    next_turn: String,
    eval: Option<Vec<i32>>,
    last_move: Option<i32>
}



#[wasm_bindgen]
impl App {
    #[allow(clippy::new_without_default)]
    // #[wasm_bindgen(constructor)]
    pub async fn new(eval_string: &str) -> Self {
        let evaluator = match Evaluator::read_string(eval_string) {
            Ok(e) => e,
            Err(e) => {
                console_log!("Evaluatorを読み込む際にエラーが置きました。{}",e);
                panic!();
            }
        };
        Self {
            game: Game::new(),
            solver: Solver::new(evaluator),
            rng: rand::thread_rng(),
            first_move_played: false
        }
    }

    pub fn new_game(&mut self) {
        self.game = Game::new();
        self.first_move_played = false;
    }

    #[wasm_bindgen]
    pub fn set_ai_level(&mut self, lv: i32){
        self.solver.set_ai_level(lv);
    }


    fn get_eval_score(&mut self) -> Result<i32, &'static str> {
        match self.solver.solve(self.game.get_board()) {
            Ok(d) => {
                Ok(d.eval)
            },
            Err(_) => {
                Err("Invalid position")
            }
        }
    }

    fn get_move_scores(&mut self, lv: i32) -> [i32; 64] {
        let mut scores = [0; 64];
        self.set_ai_level(lv);
        let b = self.game.get_board();
        let legal_moves =  b.put_able();
        for i in 0..64 {
            let mask = 1u64 << i;
            if mask & legal_moves != 0 {
                let position = position_bit_to_num(mask).unwrap();
                let mut b = b.clone();
                b.put_piece(mask);
                let result = 
                match self.solver.solve_no_iter(&b) {
                    Ok(r) => {
                        // console_log!("    score         : {:+}", r.eval);
                        // console_log!("    best move     : {  }", position_bit_to_str(r.best_move).unwrap());
                        // console_log!("    node          : {  }", r.searched_nodes);
                        r
                    },
                    Err(e) => {
                        if let SolverErr::NoMove = e {
                            console_log!("solve err: No move");
                            return [-100; 64];
                        }
                        return [0; 64];
                    }
                };
                
                scores[position as usize] = -result.eval;
                
            }
        }
        scores
    }

    #[wasm_bindgen]
    pub fn get_state(&mut self, eval_level: Option<i32>) -> JsValue  {
        let eval= eval_level.map(|lv| self.get_move_scores(lv).to_vec());

        let b = self.game.get_board();
        let legal_moves =  b.put_able();

        let next_turn = {
            if b.next_turn == Board::BLACK {
                "Black"
            } else {
                "White"
            }
        };

        let s = StateForJS { 
            black: format!("{:064b}", b.bit_board[Board::BLACK]),
            white: format!("{:064b}", b.bit_board[Board::WHITE]),
            legal_moves: format!("{:064b}", legal_moves),
            flipping: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            next_turn: next_turn.to_string(),
            eval,
            last_move: self.game.get_last_move()
        };

        serde_wasm_bindgen::to_value(&s).unwrap()
    }

    #[wasm_bindgen]
    pub fn get_eval_scores(&mut self) -> i32 {
        if !self.is_pass() && !self.is_end() {
            self.get_eval_score().unwrap()
        } else {
            0
        }
    }

    #[wasm_bindgen]
    pub fn put(&mut self, i: i32) {
        self.first_move_played = true;
        let position_bit = position_num_to_bit(i).unwrap();
        let position_str = position_bit_to_str(position_bit).unwrap();
        self.game.put(position_str.as_str());
    }

    #[wasm_bindgen]
    pub fn is_legal_move(&mut self, i: i32) -> bool {
        let position_bit = position_num_to_bit(i).unwrap();
        return self.game.get_board().put_able() & position_bit != 0
    }

    #[wasm_bindgen]
    pub fn is_pass(&self) -> bool {
        self.game.is_pass()
    }

    #[wasm_bindgen]
    pub fn is_end(&self) -> bool {
        self.game.is_end()
    }
    
    #[wasm_bindgen]
    pub fn pass(&mut self) {
        self.game.pass();
    }

    #[wasm_bindgen]
    pub fn undo(&mut self) {
        self.game.undo();
    }

    #[wasm_bindgen]
    pub fn redo(&mut self) {
        self.game.undo();
    }

    #[wasm_bindgen]
    pub fn ai_put(&mut self, lv: i32) {

        if !self.first_move_played {
            let first_moves = ["f5", "e6", "d3", "c4"];
            let x: i32 = self.rng.gen();
            let positon = first_moves[x as usize % first_moves.len()];
            console_log!("{}", positon);
            let put = self.game.put(positon);
            match put {
                Ok(_) => (),
                Err(e) => console_log!("{}", e)
            };
            
            self.first_move_played = true;
            return;
        }
        
        self.set_ai_level(lv);
        let b = self.game.get_board();
        let solver_result = self.solver.solve(b);
        match solver_result {
            Ok(r) => {
                let p = position_bit_to_str(r.best_move).unwrap();
                let put = self.game.put(p.as_str());
                match put {
                    Ok(_) => (),
                    Err(e) => console_log!("{}", e)
                };
                console_log!("    lv            : {}", lv);
                console_log!("    score         : {:+}", r.eval);
                console_log!("    best move     : {  }", position_bit_to_str(r.best_move).unwrap());
                console_log!("    node          : {  }", r.searched_nodes);
            },
            Err(_) => {
                console_log!("Solver Err");
            }
        }
    }

    #[wasm_bindgen]
    pub fn get_record(&self) -> String {
        self.game.record()
    }

    pub fn do_over(&mut self) {
        self.game.do_over();
    }
}