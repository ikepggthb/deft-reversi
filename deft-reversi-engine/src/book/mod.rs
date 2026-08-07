//! Egaroucid 互換の opening book。
//!
//! Egaroucid (<https://github.com/Nyanyan/Egaroucid>) の `book.egbk3` をそのまま
//! 読み書きできる定石データベース。旧形式 (`.egbk2` / `.egbk`) と Edax の
//! `book.dat` の取り込み・書き出しにも対応する。
//!
//! ```no_run
//! use deft_reversi_engine::{Board, Book};
//!
//! let book = Book::load("book.egbk3").unwrap();
//! if let Some(mv) = book.best_move(&Board::new()) {
//!     println!("book move: {} ({:+})", mv.mv, mv.value);
//! }
//! ```
//!
//! # Edax book との設計の違い
//!
//! Edax は局面ごとに「book 内の子へつながる手 = link」の配列をファイルに持つが、
//! Egaroucid は **link を保存しない**。手を知りたいときは合法手を実際に打って
//! みて、その子局面が book にあるかを引く ([`Book::moves_with_value`])。
//! そのため
//!
//! - レコードが 25 バイト固定になりファイルが小さい
//! - link の張り直しという操作が要らない
//! - パス局面を保存する必要がない (参照時にパスを挟むだけ)
//!
//! という違いがある。局面が持つのは評価値・レベル・`n_lines`・leaf だけで、
//! Edax のような評価値の上下界や勝敗数は持たない。
//!
//! ファイル形式の詳細は [`egbk`]、Edax との相互変換は [`edax`] を参照。

pub mod build;
pub mod edax;
pub mod egbk;
pub mod elem;
mod random;
mod table;

pub use edax::EDAX_BOOK_MAGIC;
pub use egbk::{EGBK_MAGIC, EGBK_VERSION};
pub use elem::{
    convert_coord_from_representative, convert_coord_to_representative, is_valid_policy,
    is_valid_score, representative_board, BookElem, BookMove, Leaf, LEVEL_UNDEFINED, MAX_N_LINES,
    MOVE_NOMOVE, MOVE_PASS, MOVE_UNDEFINED, SCORE_MAX, SCORE_UNDEFINED,
};

use crate::board::board::Board;
use crate::EngineError;
use elem::next_board;
use random::Random;
use table::{BoardMap, Entry, Generation, Generations};

/// book の手が本譜の評価値からこれ以上離れていたら book を使わない。
///
/// Egaroucid の `BOOK_LOSS_IGNORE_THRESHOLD`。book の自己矛盾に対する保険。
pub const BOOK_LOSS_IGNORE_THRESHOLD: i32 = 8;

/// [`Book::random_move`] の精度レベルの上限。Egaroucid の `BOOK_ACCURACY_LEVEL_INF`。
pub const BOOK_ACCURACY_LEVEL_INF: i32 = 10;

/// 参照した子局面 1 つ分の情報。
#[derive(Clone, Copy, Debug)]
struct ChildEntry {
    /// 親の盤面での着手座標。
    mv: u8,
    /// 実際に book から引いた子局面の正規形。
    key: Board,
    /// 子の評価値に掛ける符号。パスを挟むと +1 になる。
    sign: i8,
}

/// Egaroucid 互換の opening book。
///
/// 局面は正規形の盤面をキーにしたハッシュ表 (`table` モジュール) に持つ。
/// 走査中の訪問済み判定は各エントリの世代印で行うので、木を辿るときに
/// 局面数に比例した一時領域を確保しない。
#[derive(Clone, Debug)]
pub struct Book {
    /// キーは正規形の盤面。着手座標もこの向きで保持する。
    positions: BoardMap,
    random: Random,
    generations: Generations,
}

impl Default for Book {
    fn default() -> Self {
        Self::new()
    }
}

impl Book {
    /// 初期盤面だけを含む book を作る。Egaroucid の `reg_first_board`。
    pub fn new() -> Self {
        let mut book = Self::empty();
        book.register_first_board();
        book
    }

    /// 局面を 1 つも持たない book を作る。
    pub fn empty() -> Self {
        Self {
            positions: BoardMap::default(),
            random: Random::from_clock(),
            generations: Generations::default(),
        }
    }

    /// 局面数の見当がついているときに、あらかじめ領域を確保して作る。
    /// 読み込み時の再ハッシュを避けるために使う。
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            positions: BoardMap::with_capacity_and_hasher(capacity, Default::default()),
            random: Random::from_clock(),
            generations: Generations::default(),
        }
    }

    // ---- 内部アクセサ ----

    /// 正規形の盤面をキーに局面データを引く。
    #[inline]
    pub(super) fn elem(&self, key: &Board) -> Option<&BookElem> {
        self.positions.get(key).map(|entry| &entry.elem)
    }

    /// 正規形の盤面をキーに局面データを書き換え可能な参照で引く。
    #[inline]
    pub(super) fn elem_mut(&mut self, key: &Board) -> Option<&mut BookElem> {
        self.positions.get_mut(key).map(|entry| &mut entry.elem)
    }

    /// 正規形の盤面をキーに削除する。
    pub(super) fn remove_representative(&mut self, key: &Board) -> bool {
        self.positions.remove(key).is_some()
    }

    /// 走査を 1 つ始める。全エントリが未訪問に戻る。
    pub(super) fn begin_traversal(&mut self) -> Generation {
        self.generations.next(&mut self.positions)
    }

    /// この走査で `key` を初めて訪れたなら `true`。
    #[inline]
    pub(super) fn mark_visited(&mut self, key: &Board, generation: Generation) -> bool {
        match self.positions.get_mut(key) {
            Some(entry) => entry.mark_visited(generation),
            None => false,
        }
    }

    /// 印の付いていない局面をすべて削除する。削除した数を返す。
    pub(super) fn retain_visited(&mut self, generation: Generation) -> usize {
        let before = self.positions.len();
        self.positions
            .retain(|_, entry| entry.is_visited(generation));
        before - self.positions.len()
    }

    /// 条件を満たさない局面を削除する。削除した数を返す。
    pub(super) fn retain_keys(&mut self, mut keep: impl FnMut(&Board) -> bool) -> usize {
        let before = self.positions.len();
        self.positions.retain(|board, _| keep(board));
        before - self.positions.len()
    }

    /// 初期盤面を value 0 / leaf D3 で登録する。Egaroucid の `reg_first_board`。
    pub fn register_first_board(&mut self) {
        let elem = BookElem {
            value: 0,
            level: LEVEL_UNDEFINED,
            leaf: Leaf {
                value: 0,
                mv: 19,
                level: LEVEL_UNDEFINED,
            },
            n_lines: 0,
        };
        self.positions
            .insert(Board::new().unique_board(), Entry::new(elem));
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// 正規形の盤面と局面データを走査する。順序は内部の表の並び順で、
    /// 辞書順ではない (Egaroucid も同じ)。
    pub fn iter(&self) -> impl Iterator<Item = (&Board, &BookElem)> {
        self.positions
            .iter()
            .map(|(board, entry)| (board, &entry.elem))
    }

    /// 正規形の盤面を走査する。順序は [`Book::iter`] と同じ。
    pub fn boards(&self) -> impl Iterator<Item = &Board> {
        self.positions.keys()
    }

    /// ランダム手選択の乱数種を固定する (テスト・再現用)。
    pub fn set_random_seed(&mut self, seed: u64) {
        self.random = Random::new(seed);
    }

    /// 盤面 (対称形を含む) が登録されているか。Egaroucid の `contain`。
    pub fn contains(&self, board: &Board) -> bool {
        self.positions.contains_key(&board.unique_board())
    }

    /// 正規形の盤面をそのままキーとして引く。Egaroucid の `contain_representative`。
    pub fn contains_representative(&self, board: &Board) -> bool {
        self.positions.contains_key(board)
    }

    /// 盤面 (対称形を含む) の局面データを、**`board` の向きに直した leaf 座標で**
    /// 返す。Egaroucid の `get`。
    pub fn get(&self, board: &Board) -> Option<BookElem> {
        let (rep, idx) = representative_board(board);
        let mut elem = *self.elem(&rep)?;
        if elem.leaf.is_move() {
            elem.leaf.mv = convert_coord_from_representative(elem.leaf.mv as u8, idx) as i8;
        }
        Some(elem)
    }

    /// 正規形の盤面をキーに、座標変換せずそのまま引く。
    pub fn get_representative(&self, board: &Board) -> Option<&BookElem> {
        self.elem(board)
    }

    /// 初期盤面の局面データ。
    pub fn root(&self) -> Option<BookElem> {
        self.get(&Board::new())
    }

    /// 局面を登録する。leaf 座標は正規形の向きに移して保存する。
    ///
    /// Egaroucid の `reg` / `register_symmetric_book`。既存なら上書きし、
    /// 新規に追加したときだけ `true` を返す。
    pub fn register(&mut self, board: &Board, mut elem: BookElem) -> bool {
        let (rep, idx) = representative_board(board);
        if elem.leaf.mv != MOVE_UNDEFINED && elem.leaf.is_move() {
            elem.leaf.mv = convert_coord_to_representative(elem.leaf.mv as u8, idx) as i8;
        }
        self.positions.insert(rep, Entry::new(elem)).is_none()
    }

    /// 正規形の盤面をキーに、座標変換せず登録する。
    pub fn register_representative(&mut self, board: Board, elem: BookElem) -> bool {
        debug_assert_eq!(board, board.unique_board());
        self.positions.insert(board, Entry::new(elem)).is_none()
    }

    /// 局面を削除する。Egaroucid の `delete_elem`。
    pub fn remove(&mut self, board: &Board) -> bool {
        self.remove_representative(&board.unique_board())
    }

    /// 全局面を消して初期盤面だけに戻す。Egaroucid の `delete_all`。
    pub fn clear(&mut self) {
        self.positions.clear();
        self.register_first_board();
    }

    /// 局面を取り込む。既にある場合は **level が同じか高い方の値を採用**する。
    ///
    /// Egaroucid の `merge`。value と leaf はそれぞれ独立に判定する。
    pub fn merge_elem(&mut self, board: &Board, elem: BookElem) -> bool {
        let (key, idx) = representative_board(board);
        let mut elem = elem;
        if elem.leaf.is_move() {
            elem.leaf.mv = convert_coord_to_representative(elem.leaf.mv as u8, idx) as i8;
        }
        self.merge_representative(key, elem)
    }

    /// 正規化済みの盤面と、正規形の向きの leaf 座標を持つ局面を取り込む。
    ///
    /// 読み込み時のホットパス。正規化もハッシュ表の探索も 1 回で済ませる。
    pub(super) fn merge_representative(&mut self, key: Board, elem: BookElem) -> bool {
        use std::collections::hash_map::Entry as MapEntry;
        match self.positions.entry(key) {
            MapEntry::Vacant(slot) => {
                slot.insert(Entry::new(elem));
                true
            }
            MapEntry::Occupied(mut slot) => {
                let current = &mut slot.get_mut().elem;
                if elem.value != SCORE_UNDEFINED && current.level <= elem.level {
                    current.value = elem.value;
                    current.level = elem.level;
                }
                if elem.leaf.value != SCORE_UNDEFINED && current.leaf.level <= elem.leaf.level {
                    current.leaf = elem.leaf;
                }
                false
            }
        }
    }

    /// 別の book の局面をすべて取り込む。追加された局面数を返す。
    pub fn merge(&mut self, other: &Book) -> usize {
        let mut added = 0;
        for (board, elem) in other.iter() {
            if self.merge_elem(board, *elem) {
                added += 1;
            }
        }
        added
    }

    /// 評価値を書き換える (無ければ登録する)。Egaroucid の `change`。
    ///
    /// 終局局面やパス局面では、Egaroucid と同じく符号を合わせて登録先を選ぶ。
    pub fn change(&mut self, board: &Board, value: i8, level: i8) {
        if !is_valid_score(value) {
            return;
        }
        let mut board = *board;
        let mut value = value;

        if board.moves() == 0 && board.opponent_moves() == 0 {
            // 終局。すでにどちらかの向きで登録されていればそれを更新する。
            if self.contains(&board) {
                self.update_value(&board, value, level);
                return;
            }
            let passed = board.passed();
            if self.contains(&passed) {
                self.update_value(&passed, -value, level);
                return;
            }
        } else if board.moves() == 0 {
            // パス。手番側の視点に合わせる。
            board = board.passed();
            value = -value;
        }

        if self.contains(&board) {
            self.update_value(&board, value, level);
        } else {
            self.register(&board, BookElem::new(value, level));
        }
    }

    fn update_value(&mut self, board: &Board, value: i8, level: i8) {
        if let Some(elem) = self.elem_mut(&board.unique_board()) {
            elem.value = value;
            elem.level = level;
        }
    }

    /// leaf を差し替える。Egaroucid の `add_leaf`。
    pub fn set_leaf(&mut self, board: &Board, leaf: Leaf) {
        let (rep, idx) = representative_board(board);
        let mut leaf = leaf;
        if leaf.is_move() {
            leaf.mv = convert_coord_to_representative(leaf.mv as u8, idx) as i8;
        }
        self.positions
            .entry(rep)
            .or_insert_with(|| Entry::new(BookElem::default()))
            .elem
            .leaf = leaf;
    }

    // ---- 手の参照 (link を持たないので子局面を引いて導出する) ----

    /// `board` から 1 手進めた先が book にある手を 1 つずつ渡す。
    ///
    /// Egaroucid の `get_all_moves_with_value` と同じ分岐でパス・終局を扱う。
    /// 呼び出し側が Vec を必要としない場合に割り当てを避けられるよう、
    /// コールバック形式にしてある。
    fn for_each_child(&self, board: &Board, mut f: impl FnMut(ChildEntry)) {
        let mut legal = board.moves();
        while legal != 0 {
            let mv = legal.trailing_zeros() as u8;
            legal &= legal - 1;
            let child = board.make_move(1u64 << mv);

            let child_legal = child.moves();
            if child_legal == 0 && child.opponent_moves() == 0 {
                // 終局。どちらの向きで登録されているか分からないので両方見る。
                let key = child.unique_board();
                if self.contains_representative(&key) {
                    f(ChildEntry { mv, key, sign: -1 });
                    continue;
                }
                let passed = child.passed().unique_board();
                if self.contains_representative(&passed) {
                    f(ChildEntry {
                        mv,
                        key: passed,
                        sign: 1,
                    });
                }
            } else if child_legal == 0 {
                // 相手がパスするので手番が戻る = 符号は反転しない。
                let key = child.passed().unique_board();
                if self.contains_representative(&key) {
                    f(ChildEntry { mv, key, sign: 1 });
                }
            } else {
                let key = child.unique_board();
                if self.contains_representative(&key) {
                    f(ChildEntry { mv, key, sign: -1 });
                }
            }
        }
    }

    /// 子局面が book に登録されている手のビットマスク。Vec を作らない。
    pub fn registered_moves_mask(&self, board: &Board) -> u64 {
        let mut mask = 0u64;
        self.for_each_child(board, |entry| mask |= 1u64 << entry.mv);
        mask
    }

    /// 子局面の評価値 (親から見た符号) を 1 つずつ渡す。割り当てなし。
    pub(super) fn for_each_child_value(&self, board: &Board, mut f: impl FnMut(i8)) {
        self.for_each_child(board, |entry| {
            if let Some(elem) = self.elem(&entry.key) {
                f((entry.sign as i32 * elem.value as i32).clamp(-127, 127) as i8);
            }
        });
    }

    /// 子局面が book に登録されている手が 1 つでもあるか。見つけ次第打ち切る。
    pub fn has_registered_move(&self, board: &Board) -> bool {
        let mut legal = board.moves();
        while legal != 0 {
            let mv = legal.trailing_zeros() as u8;
            legal &= legal - 1;
            let child = board.make_move(1u64 << mv);
            let child_legal = child.moves();
            if child_legal == 0 {
                if self.contains(&child.passed()) {
                    return true;
                }
                if child.opponent_moves() == 0 && self.contains(&child) {
                    return true;
                }
            } else if self.contains(&child) {
                return true;
            }
        }
        false
    }

    /// 子局面のキーと符号の一覧。negamax 用。
    pub(super) fn child_entries_for_negamax(&self, board: &Board) -> Vec<(Board, i32)> {
        let mut out = Vec::new();
        self.for_each_child(board, |entry| out.push((entry.key, entry.sign as i32)));
        out
    }

    /// book に登録されている手とその評価値を、`board` の向きで返す。
    ///
    /// Egaroucid の `get_all_moves_with_value`。link を保存しない代わりに、
    /// 合法手を打った先が book にあるかをその都度引く。
    pub fn moves_with_value(&self, board: &Board) -> Vec<BookMove> {
        let mut out = Vec::new();
        self.for_each_child(board, |entry| {
            if let Some(elem) = self.elem(&entry.key) {
                out.push(BookMove {
                    mv: entry.mv,
                    value: (entry.sign as i32 * elem.value as i32).clamp(-127, 127) as i8,
                });
            }
        });
        out
    }

    /// 評価値が最大の手をすべて返す。Egaroucid の `get_all_best_moves`。
    pub fn best_moves(&self, board: &Board) -> Vec<u8> {
        let moves = self.moves_with_value(board);
        let Some(max) = moves.iter().map(|m| m.value).max() else {
            return Vec::new();
        };
        moves
            .into_iter()
            .filter(|m| m.value == max)
            .map(|m| m.mv)
            .collect()
    }

    /// book の最善手。Egaroucid の `get_specified_best_move` を全合法手に適用した形。
    pub fn best_move(&self, board: &Board) -> Option<BookMove> {
        self.best_move_from(board, !0u64)
    }

    /// 着手を `use_legal` に絞った最善手。Egaroucid の `get_specified_best_move`。
    pub fn best_move_from(&self, board: &Board, use_legal: u64) -> Option<BookMove> {
        self.moves_with_value(board)
            .into_iter()
            .filter(|m| use_legal & (1u64 << m.mv) != 0)
            .max_by_key(|m| m.value)
    }

    /// 精度レベル付きでランダムに 1 手選ぶ。Egaroucid の `get_random`。
    ///
    /// `acc_level` は 0 が最強、[`BOOK_ACCURACY_LEVEL_INF`] が最も緩い。
    /// 最善の子の値がこの局面自身の値より [`BOOK_LOSS_IGNORE_THRESHOLD`] 以上
    /// 悪いときは、book が自己矛盾しているとみなして `None` を返す。
    pub fn random_move(&mut self, board: &Board, acc_level: i32) -> Option<BookMove> {
        self.random_move_from(board, acc_level, !0u64)
    }

    /// 着手を `use_legal` に絞った [`Book::random_move`]。
    pub fn random_move_from(
        &mut self,
        board: &Board,
        acc_level: i32,
        use_legal: u64,
    ) -> Option<BookMove> {
        // get_random は moves_with_value と違い、終局でもパスを 1 回だけ挟む。
        let mut moves: Vec<BookMove> = Vec::new();
        let mut legal = board.moves() & use_legal;
        while legal != 0 {
            let mv = legal.trailing_zeros() as u8;
            legal &= legal - 1;
            let mut child = board.make_move(1u64 << mv);
            let mut sign = -1i32;
            if child.moves() == 0 {
                sign = 1;
                child = child.passed();
            }
            if let Some(elem) = self.elem(&child.unique_board()) {
                moves.push(BookMove {
                    mv,
                    value: (sign * elem.value as i32).clamp(-127, 127) as i8,
                });
            }
        }

        let best = moves.iter().map(|m| m.value as i32).max()?;
        let own_value = self.get(board).map(|e| e.value as i32).unwrap_or(0);
        if best < own_value - BOOK_LOSS_IGNORE_THRESHOLD {
            return None;
        }

        // Egaroucid と同じ重み付け。値が最善に近いほど重い。
        let acceptable_min = best as f64 - 2.0 * acc_level as f64 - 0.5;
        let exponent = (BOOK_ACCURACY_LEVEL_INF - acc_level).max(0) as f64;
        let mut weights: Vec<f64> = moves
            .iter()
            .map(|m| {
                let v = m.value as f64;
                if v < acceptable_min {
                    0.0
                } else {
                    (((v - best as f64).exp() + 1.5) / 3.0).powf(exponent)
                }
            })
            .collect();

        let sum: f64 = weights.iter().sum();
        if sum <= 0.0 {
            // 全部 0 になったら最善手を返す (Egaroucid も最後の要素に落ちる)。
            return moves.into_iter().max_by_key(|m| m.value);
        }
        for w in weights.iter_mut() {
            *w /= sum;
        }

        let rnd = self.random.next_f64();
        let mut acc = 0.0;
        for (m, w) in moves.iter().zip(weights.iter()) {
            acc += w;
            if acc >= rnd {
                return Some(*m);
            }
        }
        moves.last().copied()
    }

    /// `board` から book を辿って得られる手順。
    pub fn line(&mut self, board: &Board, acc_level: i32) -> Vec<u8> {
        let mut line = Vec::new();
        let mut board = *board;
        loop {
            if board.moves() == 0 {
                if board.opponent_moves() == 0 {
                    break;
                }
                board = board.passed();
                continue;
            }
            let Some(mv) = self.random_move(&board, acc_level) else {
                break;
            };
            let Some(next) = next_board(&board, mv.mv as i8) else {
                break;
            };
            line.push(mv.mv);
            board = next;
            if line.len() >= 60 {
                break;
            }
        }
        line
    }

    /// book ファイルを拡張子から判別して読み込む。
    ///
    /// `.egbk3` / `.egbk2` / `.egbk` / Edax の `.dat` に対応する。拡張子が
    /// 判別できない場合は中身のマジックで判定する。
    pub fn load(path: &str) -> Result<Self, EngineError> {
        // 大きくなりうる .egbk3 だけはストリームで読み、ファイル全体を
        // メモリに載せないようにする。他の形式は元々小さい。
        if let Some(result) = Self::load_egbk3_streaming(path) {
            return result;
        }
        Self::from_bytes(&std::fs::read(path)?)
    }

    /// バイト列の先頭を見て形式を判別し読み込む。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EngineError> {
        if bytes.starts_with(egbk::EGBK_MAGIC) {
            return Self::from_egbk_bytes(bytes);
        }
        if bytes.len() >= 8 && &bytes[0..8] == edax::EDAX_BOOK_MAGIC {
            return Self::from_edax_bytes(bytes);
        }
        // マジックの無い最初期の .egbk。
        Self::from_egbk1_bytes(bytes, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D3: u8 = 19;

    /// 初期盤面 -> D3 -> (最初の合法手) の 3 局面を持つ book。
    fn sample_book() -> Book {
        let mut book = Book::empty();
        let root = Board::new();
        let child = root.make_move(1u64 << D3);
        let grandchild = child.make_move(1u64 << child.moves().trailing_zeros());

        book.register(&root, BookElem::new(2, 10));
        book.register(&child, BookElem::new(-2, 10));
        book.register(&grandchild, BookElem::new(3, 10));
        book.set_random_seed(1);
        book
    }

    #[test]
    fn new_book_contains_only_the_initial_position() {
        let book = Book::new();
        assert_eq!(book.len(), 1);
        assert!(book.contains(&Board::new()));
        let root = book.root().unwrap();
        assert_eq!(root.value, 0);
        assert!(root.leaf.is_move());
    }

    #[test]
    fn contains_finds_every_symmetric_board() {
        let book = sample_book();
        let child = Board::new().make_move(1u64 << D3);
        for sym in child.all_symmetries() {
            assert!(book.contains(&sym), "missing symmetry of D3");
        }
    }

    #[test]
    fn get_returns_the_leaf_move_in_the_queried_orientation() {
        let mut book = Book::empty();
        let board = Board::new().make_move(1u64 << D3);
        let leaf_move = board.moves().trailing_zeros() as i8;
        book.register(
            &board,
            BookElem {
                value: 1,
                level: 5,
                leaf: Leaf {
                    value: 1,
                    mv: leaf_move,
                    level: 5,
                },
                n_lines: 0,
            },
        );

        // どの対称形で引いても、その向きの合法手として返る。
        for sym in board.all_symmetries() {
            let elem = book.get(&sym).unwrap();
            assert!(elem.leaf.is_move());
            assert_ne!(
                sym.moves() & (1u64 << elem.leaf.mv),
                0,
                "leaf move {} is not legal on this symmetry",
                elem.leaf.mv
            );
        }
    }

    #[test]
    fn moves_with_value_derives_moves_from_child_positions() {
        let book = sample_book();
        let moves = book.moves_with_value(&Board::new());

        // 初期盤面の 4 手はすべて同じ正規形に落ちるので 4 手とも返る。
        assert_eq!(moves.len(), 4);
        for m in &moves {
            assert_ne!(Board::new().moves() & (1u64 << m.mv), 0);
            // 子の value = -2 が符号反転して +2 になる。
            assert_eq!(m.value, 2);
        }
    }

    #[test]
    fn moves_with_value_returns_nothing_when_no_child_is_registered() {
        let mut book = Book::empty();
        book.register(&Board::new(), BookElem::new(0, 1));
        assert!(book.moves_with_value(&Board::new()).is_empty());
        assert!(book.best_move(&Board::new()).is_none());
    }

    #[test]
    fn moves_with_value_is_orientation_consistent() {
        let book = sample_book();
        for sym in Board::new().all_symmetries() {
            let moves = book.moves_with_value(&sym);
            assert_eq!(moves.len(), 4);
            for m in moves {
                assert_ne!(sym.moves() & (1u64 << m.mv), 0);
            }
        }
    }

    #[test]
    fn best_moves_returns_every_move_sharing_the_maximum() {
        let book = sample_book();
        assert_eq!(book.best_moves(&Board::new()).len(), 4);
        assert_eq!(book.best_move(&Board::new()).unwrap().value, 2);
    }

    #[test]
    fn random_move_rejects_positions_whose_children_are_much_worse() {
        let mut book = sample_book();
        // 自分の値だけ大きく引き上げると、子との差が閾値を超えて None になる。
        book.change(&Board::new(), 2 + BOOK_LOSS_IGNORE_THRESHOLD as i8 + 1, 10);
        assert!(book.random_move(&Board::new(), 0).is_none());
    }

    #[test]
    fn random_move_only_returns_legal_registered_moves() {
        let mut book = sample_book();
        for _ in 0..50 {
            let mv = book.random_move(&Board::new(), 0).unwrap();
            assert_ne!(Board::new().moves() & (1u64 << mv.mv), 0);
        }
    }

    #[test]
    fn random_move_from_respects_the_move_mask() {
        let mut book = sample_book();
        let mask = 1u64 << D3;
        for _ in 0..20 {
            let mv = book.random_move_from(&Board::new(), 0, mask).unwrap();
            assert_eq!(mv.mv, D3);
        }
    }

    #[test]
    fn merge_elem_prefers_the_higher_level() {
        let mut book = Book::empty();
        let board = Board::new();
        book.register(&board, BookElem::new(1, 5));

        // level が低い情報は無視される。
        book.merge_elem(&board, BookElem::new(9, 3));
        assert_eq!(book.get(&board).unwrap().value, 1);

        // level が同じか高ければ採用される。
        book.merge_elem(&board, BookElem::new(7, 5));
        assert_eq!(book.get(&board).unwrap().value, 7);
        book.merge_elem(&board, BookElem::new(8, 20));
        assert_eq!(book.get(&board).unwrap().value, 8);
        assert_eq!(book.get(&board).unwrap().level, 20);
    }

    #[test]
    fn change_flips_the_sign_on_a_pass_position() {
        let mut book = Book::empty();
        // パス局面を与えると、手番を進めた側で登録される。
        let mut board = Board::new().make_move(1u64 << D3);
        while board.moves() != 0 {
            board = board.make_move(1u64 << board.moves().trailing_zeros());
            if board.moves() == 0 && board.opponent_moves() != 0 {
                break;
            }
        }
        if board.moves() == 0 && board.opponent_moves() != 0 {
            book.change(&board, 5, 3);
            assert!(book.contains(&board.passed()));
            assert_eq!(book.get(&board.passed()).unwrap().value, -5);
        }
    }

    #[test]
    fn remove_and_clear_behave_as_expected() {
        let mut book = sample_book();
        let child = Board::new().make_move(1u64 << D3);
        assert!(book.remove(&child));
        assert!(!book.remove(&child));
        assert_eq!(book.len(), 2);

        book.clear();
        assert_eq!(book.len(), 1);
        assert!(book.contains(&Board::new()));
    }

    #[test]
    fn line_follows_the_book_until_it_runs_out() {
        let mut book = sample_book();
        let line = book.line(&Board::new(), 0);
        assert_eq!(line.len(), 2);
        assert_ne!(Board::new().moves() & (1u64 << line[0]), 0);
    }
}
