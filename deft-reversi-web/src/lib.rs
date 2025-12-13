use std::str::FromStr;

use wasm_bindgen::prelude::*;

use deft_reversi_engine::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

// https://rustwasm.github.io/wasm-bindgen/examples/console-log.html
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);

    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_u32(a: u32);

    // Multiple arguments too!
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_many(a: &str, b: &str);
}

macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen]
pub fn add(a: i32, b: i32) -> i32 {
    console_log!("{}", a + b);
    a + b
}

#[wasm_bindgen]
struct App {
    game: Game,
    solver: Solver,
    opening_book: OpeningBook,
    rng: rand::rngs::ThreadRng,
    lv: i32,
    human_opening: Option<usize>,
}

#[derive(Serialize, Deserialize)]
pub struct StateForJS {
    black_bits_low: u32,
    black_bits_high: u32,
    white_bits_low: u32,
    white_bits_high: u32,
    legal_moves_bits_low: u32,
    legal_moves_bits_high: u32,
    flipping: String,
    next_turn: String,
    eval: Option<Vec<i32>>,
    last_move: Option<i32>,
    human_opening_next_position: Option<i32>,
    current_human_opening: Option<String>,
}

#[wasm_bindgen]
impl App {
    #[allow(clippy::new_without_default)]
    // #[wasm_bindgen(constructor)]
    pub async fn new(eval_string: &str, opening_string: &str) -> Result<Self, JsValue> {
        let evaluator = Evaluator::read_string(eval_string).map_err(|e| {
            JsValue::from_str(&format!("Evaluatorを読み込む際にエラーが起きました: {}", e))
        })?;
        let opening_book = OpeningBook::from_str(opening_string).map_err(|e| {
            JsValue::from_str(&format!("Openingを読み込む際にエラーが起きました: {}", e))
        })?;
        Ok(Self {
            game: Game::new(),
            solver: Solver::new(evaluator),
            opening_book,
            rng: rand::thread_rng(),
            lv: 1,
            human_opening: None,
        })
    }

    pub fn new_game(&mut self) {
        self.game = Game::new();
    }

    #[wasm_bindgen]
    pub fn set_ai_level(&mut self, lv: i32) {
        self.lv = lv;
    }

    #[wasm_bindgen]
    pub fn set_human_opening(&mut self, index: i32) {
        self.human_opening = Some(index as usize);
    }

    fn get_eval_score(&mut self) -> i32 {
        if !self.is_pass() && !self.is_end() {
            self.solver.solve(&self.game.current.board, self.lv).eval
        } else {
            0
        }
    }

    fn get_move_scores(&mut self, lv: i32) -> [i32; 64] {
        let mut scores = [0; 64];
        let b = &self.game.current.board;
        let legal_moves = b.moves();
        for i in 0..64 {
            let mask = 1u64 << i;
            if mask & legal_moves != 0 {
                let position = match position_bit_to_num(mask) {
                    Ok(p) => p,
                    Err(_) => {
                        console_log!("Err: position_bit_to_num failed");
                        continue;
                    }
                };
                let mut b = b.clone();
                b.put(mask);
                let result = self.solver.solve(&b, lv);
                scores[position as usize] = -result.eval;
            }
        }
        scores
    }

    #[wasm_bindgen]
    pub fn get_state(&mut self, eval_level: Option<i32>) -> Result<JsValue, JsValue> {
        let eval = eval_level.map(|lv| self.get_move_scores(lv).to_vec());

        let b = &self.game.current.board;
        let legal_moves = b.moves();

        let next_turn = self.game.current.turn.get_str();

        let (black, white) = {
            match self.game.current.turn {
                Color::Black => (b.player, b.opponent),
                Color::White => (b.opponent, b.player),
            }
        };

        let black_bits_low = (black & 0xffff_ffff) as u32;
        let black_bits_high = (black >> 32) as u32;
        let white_bits_low = (white & 0xffff_ffff) as u32;
        let white_bits_high = (white >> 32) as u32;
        let legal_moves_bits_low = (legal_moves & 0xffff_ffff) as u32;
        let legal_moves_bits_high = (legal_moves >> 32) as u32;

        let human_opening_next_position = {
            if let Some(name_index) = self.human_opening {
                if self.game.current.board == Board::new() {
                    Some(37) // f5
                } else if let Ok(Some(p)) = self
                    .opening_book
                    .opening_move(&self.game.current.board, name_index)
                {
                    match position_bit_to_num(p) {
                        Ok(p) => Some(p as i32),
                        Err(_) => {
                            console_log!("Err: human_opening_next_position can not get");
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        let current_human_opening = self
            .opening_book
            .name_str_from_board(&self.game.current.board)
            .map(|s| s.to_string());

        let s = StateForJS {
            black_bits_low,
            black_bits_high,
            white_bits_low,
            white_bits_high,
            legal_moves_bits_low,
            legal_moves_bits_high,
            flipping: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            next_turn: next_turn.to_string(),
            eval,
            last_move: self.game.get_last_move(),
            human_opening_next_position,
            current_human_opening,
        };

        serde_wasm_bindgen::to_value(&s)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    }

    #[wasm_bindgen]
    pub fn put(&mut self, i: i32) -> Result<(), JsValue> {
        let position_bit = position_num_to_bit(i)
            .map_err(|e| JsValue::from_str(&format!("invalid position: {}", e)))?;
        let position_str = position_bit_to_str(position_bit)
            .map_err(|e| JsValue::from_str(&format!("invalid position: {}", e)))?;
        self.game
            .put(position_str.as_str())
            .map_err(|e| JsValue::from_str(&format!("put error: {}", e)))?;
        if let Some(s) = self
            .opening_book
            .name_str_from_board(&self.game.current.board)
        {
            console_log!("opening: {}", s);
        }
        Ok(())
    }

    #[wasm_bindgen]
    pub fn is_legal_move(&mut self, i: i32) -> bool {
        match position_num_to_bit(i) {
            Ok(position_bit) => self.game.current.board.moves() & position_bit != 0,
            Err(_) => false,
        }
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
    pub fn undo(&mut self) -> bool {
        self.game.undo().is_ok()
    }

    #[wasm_bindgen]
    pub fn redo(&mut self) -> bool {
        self.game.redo().is_ok()
    }

    #[wasm_bindgen]
    pub fn ai_put(&mut self, lv: i32) -> Result<(), JsValue> {
        if self.game.current.board == Board::new() {
            const FIRST_MOVES: [&str; 4] = ["f5", "e6", "d3", "c4"];
            let i = self.rng.gen_range(0..FIRST_MOVES.len());
            let positon = FIRST_MOVES[i];
            console_log!("{}", positon);
            self.game
                .put(positon)
                .map_err(|e| JsValue::from_str(&format!("put error: {}", e)))?;
            return Ok(());
        }

        if let Some(opening_name_index) = self.human_opening {
            if let Some(n) = self.opening_book.opening_names.get(opening_name_index) {
                console_log!("human opening name: {}", n);
            } else {
                console_log!("Err: invalid human opening index");
            };
            if let Ok(Some(p)) = self
                .opening_book
                .opening_move(&self.game.current.board, opening_name_index)
            {
                console_log!("ai: human opening move");
                let position_str = position_bit_to_str(p)
                    .map_err(|e| JsValue::from_str(&format!("invalid position: {}", e)))?;
                self.game
                    .put(&position_str)
                    .map_err(|e| JsValue::from_str(&format!("put error: {}", e)))?;
                return Ok(());
            }
        }

        let empties = self.game.current.board.empties_count();
        let r = self.solver.solve(&self.game.current.board, lv);
        let p = position_bit_to_str(r.best_move)
            .map_err(|e| JsValue::from_str(&format!("invalid position: {}", e)))?;
        self.game
            .put(p.as_str())
            .map_err(|e| JsValue::from_str(&format!("put error: {}", e)))?;
        console_log!("    solver type   : {  }", r.solver_type.description());
        console_log!("    score         : {:+}", r.eval);
        console_log!("    empty squares : {  }", empties);
        console_log!("    best move     : {  }", p);
        console_log!("    node          : {  }", r.searched_nodes);
        Ok(())
    }

    #[wasm_bindgen]
    pub fn get_record(&self) -> String {
        self.game.record()
    }
}
