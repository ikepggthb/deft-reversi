use wasm_bindgen::prelude::*;

use deft_reversi_engine::*;
use serde::{Serialize, Deserialize};
use serde_wasm_bindgen::*;

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
    game_state: Game,
}


#[derive(Serialize, Deserialize)]
pub struct StateForJS {
    black: String,
    white: String,
    legal_moves: String,
    flipping: String,
    next_turn: String,
    eval: Option<Vec<i32>>
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
            game_state: Game::new(evaluator)
        }
    }

    #[wasm_bindgen]
    pub fn set_ai_level(&mut self, lv: i32){
        self.game_state.set_ai_level(lv);
    }

    #[wasm_bindgen]
    pub fn get_state(&mut self, eval_level: Option<i32>) -> JsValue  {

        let eval= match eval_level {
            Some(lv) => {
                Some(self.game_state.get_move_scores(lv).to_vec())
            },
            None => {
                None
            }
        };


        let b = self.game_state.get_board();
        let legal_moves =  b.put_able();

        let s = StateForJS { 
            black: format!("{:064b}", b.bit_board[Board::BLACK]),
            white: format!("{:064b}", b.bit_board[Board::WHITE]),
            legal_moves: format!("{:064b}", legal_moves),
            flipping: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            next_turn: "Black".to_string(),
            eval
        };

        serde_wasm_bindgen::to_value(&s).unwrap()
    }

    #[wasm_bindgen]
    pub fn get_eval_scores(&mut self) -> i32 {
        if !self.is_pass() && !self.is_end() {
            self.game_state.get_eval_score().unwrap()
        } else {
            0
        }
    }

    #[wasm_bindgen]
    pub fn put(&mut self, i: i32) {
        let position_bit = position_num_to_bit(i).unwrap();
        let position_str = position_bit_to_str(position_bit).unwrap();
        self.game_state.put(position_str.as_str());
    }

    #[wasm_bindgen]
    pub fn is_legal_move(&mut self, i: i32) -> bool {
        let position_bit = position_num_to_bit(i).unwrap();
        return self.game_state.get_board().put_able() & position_bit != 0
    }

    #[wasm_bindgen]
    pub fn is_pass(&self) -> bool {
        self.game_state.is_pass()
    }

    #[wasm_bindgen]
    pub fn is_end(&self) -> bool {
        self.game_state.is_end()
    }
    
    #[wasm_bindgen]
    pub fn pass(&mut self) {
        self.game_state.pass();
    }
    
    #[wasm_bindgen]
    pub fn ai_put(&mut self) {
        console_log!("ai put");
    
        self.game_state.ai_put();
    }
}