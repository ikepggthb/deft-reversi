# deft-reversi-engine 外部 API 契約

## 目的

この文書は、`deft-reversi-engine` の大規模リファクタリング中に守る外部契約を定義する。

内部のファイル構成、モジュール構成、探索実装、評価実装は変更してよい。ただし、この文書で安定 API として定義した型、関数、基本挙動は、CLI/WASM など外部利用者に影響するため互換性を保つ。

## 基本方針

- 外部利用者は原則として crate root から利用できる。
- 内部モジュールの深いパスは契約に含めない。
- `Board` の bitboard 表現は当面契約に含める。既存 CLI/WASM が `Board { player, opponent }` とフィールド直接参照に依存しているため。
- 探索内部、置換表内部、move ordering 内部は安定 API に含めない。
- 既存利用箇所の都合で一時的に残す API は、安定 API ではなく互換 API として分ける。

## Crate Root で公開する安定 API

`src/lib.rs` から、少なくとも以下を利用できる状態にする。

```rust
pub use crate::board::{
    position_bit_to_num, position_bit_to_str, position_num_to_bit, position_str_to_bit, Board,
    PutPieceErr,
};
pub use crate::eval::{put_random_piece, simplest_eval, Evaluator};
pub use crate::game::{check_record, count_record, Color, Game, State};
pub use crate::search::{Solver, SolverResult, SolverType};
```

`OpeningBook` は engine の主要用途からは外れるが、定石機能として公開を維持する。

```rust
pub use crate::book::{OpeningBook, OpeningBookError};
```

## Board 契約

### 型

```rust
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Board {
    pub player: u64,
    pub opponent: u64,
}
```

### 意味

- `player` は現在手番側の石。
- `opponent` は相手側の石。
- `player & opponent == 0` を正常盤面の前提とする。
- bit index は `A1 = 0`, `B1 = 1`, ..., `H8 = 63`。
- 手を打った後の `Board` は、次の手番を `player` とするため `player` / `opponent` が入れ替わる。

### 定数

以下の座標定数は公開互換を維持する。

```rust
pub const A1: u8 = 0;
// ...
pub const H8: u8 = 63;
pub const PASS: u8 = 64;
pub const NO_COORD: u8 = 65;
```

### 関数とメソッド

```rust
impl Board {
    pub const SIZE: i32 = 8;

    pub fn new() -> Self;
    pub fn default() -> Self;
    pub fn swap(&mut self);
    pub fn swapped_board(&self) -> Board;
    pub fn clear(&mut self);

    pub fn put(&mut self, put_mask: u64) -> Result<(), PutPieceErr>;
    pub fn flip_bit(&self, x: u64) -> u64;
    pub fn put_piece_fast(&mut self, put_mask: u64);
    pub fn put_piece_fast_from_flip_bit(&mut self, put_mask: u64, flip_bit: u64);

    pub fn moves(&self) -> u64;
    pub fn opponent_moves(&self) -> u64;

    pub fn all_symmetries(&self) -> Vec<Board>;
    pub fn all_rotations(&self) -> Vec<Board>;
    pub fn get_unique_board(&self) -> Board;

    pub fn move_count(&self) -> i32;
    pub fn piece_count(&self) -> i32;
    pub fn empties_count(&self) -> i32;
}
```

`put_piece_fast` と `put_piece_fast_from_flip_bit` は合法手チェックをしない高速 API とする。呼び出し側は合法手であることを保証する。

```rust
pub enum PutPieceErr {
    NoValidPlacement,
    Unknown(String),
}
```

## 座標変換契約

```rust
pub fn position_bit_to_num(bit: u64) -> Result<u8, &'static str>;
pub fn position_num_to_bit(num: i32) -> Result<u64, &'static str>;
pub fn position_str_to_bit(s: &str) -> Result<u64, &'static str>;
pub fn position_bit_to_str(bit: u64) -> Result<String, &'static str>;
```

仕様:

- 文字列座標は `A1` から `H8`。列文字は大文字小文字を問わない。
- `position_bit_to_num` と `position_bit_to_str` は 1 bit だけ立った `u64` を受け取る。
- `position_num_to_bit` は `0..64` のみ有効。
- エラー型は当面 `&'static str` のまま維持する。

## Evaluator 契約

### 型

```rust
pub struct Evaluator {
    pub version: String,
    pub n_deta_set: i32,
    pub n_iteration: i32,
    // 評価データ本体は公開互換のため当面 public を維持する。
}
```

### 関数とメソッド

```rust
impl Evaluator {
    pub fn new() -> Self;
    pub fn default() -> Self;

    pub fn clac_features(&mut self, board: &Board);
    pub fn calc_eval(&self, board: &Board) -> i32;
    pub fn clac_features_eval(&mut self, board: &Board) -> i32;

    pub fn evaluate(&self, board: &Board, state: &EvalState) -> i32;
    pub fn evaluate_board_slow(&self, board: &Board) -> i32;
}
```

注意:

- `clac_features` / `clac_features_eval` は typo を含む既存名だが、互換性のため当面維持する。
- 将来 `calc_features` を追加してもよいが、既存名の削除は別途破壊的変更として扱う。
- 評価ファイルの読み書きは `EngineFile` 側の責務で、`Evaluator` は runtime の評価器だけを表す。

### 簡易評価

```rust
pub fn simplest_eval(board: &Board) -> i32;
pub fn put_random_piece(board: &mut Board) -> Result<(), PutPieceErr>;
```

## Solver 契約

### 型

```rust
#[derive(Clone, Copy)]
pub enum SolverType {
    Eval(i32, i32),  // depth, selectivity_lv
    Final(i32),      // selectivity_lv
}

pub struct SolverResult {
    pub best_move: u64,
    pub eval: i32,
    pub solver_type: SolverType,
    pub searched_nodes: u64,
    pub searched_leaf_nodes: u64,
}

pub struct Solver {
    // フィールド公開は互換 API として扱う。
}
```

### 関数とメソッド

```rust
impl SolverType {
    pub fn description(&self) -> String;
}

impl Solver {
    pub fn new(evaluator: Evaluator) -> Self;
    pub fn solve(&mut self, board: &Board, lv: i32) -> SolverResult;
}
```

仕様:

- `lv` は `1..=60` に clamp される。
- `best_move` は 1 bit だけ立った合法手 bitboard。
- 合法手がなく両者とも打てない終局盤面では、`best_move == 0` を返す。
- 合法手がなくパスが必要な盤面では、内部的に手番を入れ替えて探索し、評価値の符号を反転して返す。
- `eval` は現在手番側から見た評価値。
- `searched_nodes` と `searched_leaf_nodes` は探索統計であり、厳密な値は探索実装変更により変わりうる。

## Game 契約

### 型

```rust
pub struct Game {
    pub current: State,
}

pub struct State {
    pub board: Board,
    pub put_place: u8,
    pub turn: Color,
}

#[derive(Clone, Copy)]
pub enum Color {
    Black,
    White,
}
```

### 関数とメソッド

```rust
impl Color {
    pub fn opponent(&self) -> Color;
    pub fn get_char(&self) -> char;
    pub fn get_str(&self) -> &str;
}

pub fn count_record(record: &str) -> Result<usize, &'static str>;
pub fn check_record(record: &str) -> Result<(), &str>;

impl Game {
    pub fn new() -> Self;
    pub fn from_record(record: &str) -> Result<Self, &'static str>;
    pub fn undo(&mut self) -> Result<(), &'static str>;
    pub fn redo(&mut self) -> Result<(), &'static str>;
    pub fn put(&mut self, position: &str) -> Result<(), &'static str>;
    pub fn is_pass(&self) -> bool;
    pub fn pass(&mut self);
    pub fn is_end(&self) -> bool;
    pub fn record(&self) -> String;
    pub fn get_last_move(&self) -> Option<i32>;
}
```

仕様:

- `Game::new()` は初期局面、黒番で開始する。
- `Game::put()` は座標文字列を受け取り、成功時に手番を進める。
- `Game::pass()` は pass 可能な局面でのみ手番を進める。
- `record()` は pass を含めず、実着手だけを座標文字列連結で返す。

## OpeningBook 契約

```rust
pub struct OpeningBook {
    pub opening_names: Vec<String>,
}

pub enum OpeningBookError {
    ParseError(String),
    InvalidOpeningData(String),
    IoError(std::io::Error),
}

impl OpeningBook {
    pub fn new() -> Self;
    pub fn from_file(file_path: &str) -> Result<Self, OpeningBookError>;
    pub fn name_str_from_board(&self, board: &Board) -> Option<&str>;
    pub fn name_from_board(&self, board: &Board) -> Option<usize>;
    pub fn reachable_name_string(&self, board: &Board) -> Option<Vec<String>>;
    pub fn reachable_name(&self, board: &Board) -> Option<Vec<usize>>;
    pub fn opening_move(&self, board: &Board, name_index: usize) -> Result<Option<u64>, PutPieceErr>;
    pub fn opening_move_from_string(
        &self,
        board: &Board,
        name: &String,
    ) -> Result<Option<u64>, PutPieceErr>;
}
```

`OpeningBook` は `Default` と `FromStr` も維持する。

## 互換 API

以下は既存 CLI などが使っているため一時的に残すが、安定 API ではない。

```rust
pub use crate::search::{MoveIterator, TranspositionTable};
```

理由:

- `MoveIterator` は `deft-reversi-cli` の perft/self-play が利用している。
- `TranspositionTable` は旧 `lib.rs` で re-export されていた。

以下の直接アクセスは将来的に削除候補とする。

```rust
solver.search.t_table.set_old();
```

代替 API として、必要なら次を追加する。

```rust
impl Solver {
    pub fn age_transposition_table(&mut self);
}
```

または `solve()` 内で置換表 aging を完結させ、外部から触らせない。

## 非契約 API

以下は外部契約に含めない。

- `eval_search`, `final_search`, `cut`, `mpc` の探索内部関数
- `SearchEngine` と `SearchStats` のフィールド詳細
- `MoveBoard`, `MoveIteratorParity`, move ordering 内部関数
- `TranspositionTable::add`, `get`, `hash_board` など置換表内部操作
- 評価特徴量の内部配列や `evaluator_const` の詳細
- `learn` 配下の学習 API

リファクタリング中に必要であれば `pub(crate)` に落とす。

## 破壊的変更として扱うもの

以下は外部利用者に影響するため、通常の内部リファクタリングでは行わない。

- `Board` の `player` / `opponent` フィールドを private にする。
- bit index と座標対応を変更する。
- `Board::put_piece_fast` の手番入れ替え挙動を変更する。
- `Solver::solve` の `eval` の視点を変更する。
- `Evaluator::read_string` / `read_file` が既存評価 JSON を読めなくなる。
- `Game::record()` の文字列形式を変更する。
- crate root から `Board`, `Evaluator`, `Solver`, `Game` を直接 import できなくする。
