//! book の育成と整理。Egaroucid の leaf 探索・negamax・reduce に対応する。
//!
//! Egaroucid は link を保存しないので、book を広げる操作は
//!
//! 1. 各局面について「まだ book に無い手」の中の最善手を探索して leaf に記録する
//!    ([`Book::search_leaf`])
//! 2. leaf の手の子局面を book に登録する ([`Book::expand_leaves`])
//! 3. [`Book::negamax`] で評価値を親へ伝播させる
//!
//! という形になる。[`Book::expand`] がこの 3 つを繰り返す。
//!
//! Egaroucid の engine には book を広げるループが無く (GUI 側にある)、
//! [`Book::upgrade_better_leaves`] のような整理の操作だけがある。
//! [`Book::expand_leaves`] / [`Book::expand`] は Egaroucid のデータモデルに
//! 沿って足したこのエンジン側の育成 API。

use super::elem::{
    clamp_score, is_valid_score, next_board, BookElem, Leaf, LEVEL_UNDEFINED, MAX_N_LINES,
    MOVE_NOMOVE, MOVE_UNDEFINED, SCORE_UNDEFINED,
};
use super::Book;
use crate::board::board::Board;
use crate::board::position::position_str_to_num;
use crate::search::{solver_type_for_level, Solver, SolverType};
use crate::EngineError;
use std::collections::{BTreeMap, BTreeSet};

impl Book {
    /// 局面を探索して book に登録する。
    ///
    /// 既に登録済みなら level が同じか高いときだけ上書きする ([`Book::merge_elem`])。
    /// 新規に追加したときだけ `true` を返す。
    pub fn add_board(&mut self, board: &Board, level: i32, solver: &Solver) -> bool {
        let mut board = *board;
        if board.moves() == 0 {
            if board.opponent_moves() == 0 {
                return false;
            }
            board = board.passed();
        }
        let result = solver.solve(&board, level);
        self.merge_elem(
            &board,
            BookElem {
                value: clamp_score(result.score),
                level: level.clamp(0, i8::MAX as i32) as i8,
                leaf: Leaf::default(),
                n_lines: 1,
            },
        )
    }

    /// 棋譜 (`"F5D6C3"` のような手順文字列) の全局面を book に登録する。
    ///
    /// `max_moves` 手目まで登録する。追加した局面数を返す。
    pub fn add_record(
        &mut self,
        record: &str,
        max_moves: usize,
        level: i32,
        solver: &Solver,
    ) -> Result<usize, EngineError> {
        if record.len() % 2 != 0 {
            return Err(EngineError::InvalidRecord {
                reason: "record length must be even".to_string(),
                offset: record.len(),
            });
        }

        let mut boards = vec![Board::new()];
        let mut board = Board::new();
        for (i, chunk) in record.as_bytes().chunks(2).enumerate() {
            if i >= max_moves {
                break;
            }
            let move_str = std::str::from_utf8(chunk).map_err(|_| EngineError::InvalidRecord {
                reason: "record is not UTF-8".to_string(),
                offset: i * 2,
            })?;
            if board.moves() == 0 {
                if board.opponent_moves() == 0 {
                    break;
                }
                board = board.passed();
            }
            let mv = position_str_to_num(move_str)?;
            if board.moves() & (1u64 << mv) == 0 {
                return Err(EngineError::InvalidRecord {
                    reason: format!("illegal move: {move_str}"),
                    offset: i * 2,
                });
            }
            board = board.make_move(1u64 << mv);
            boards.push(board);
        }

        let mut added = 0;
        for board in &boards {
            if self.add_board(board, level, solver) {
                added += 1;
            }
        }
        Ok(added)
    }

    // ---- leaf ----

    /// `board` の「まだ book に無い手」の中の最善手を探索し leaf に記録する。
    ///
    /// Egaroucid の `search_leaf`。全ての手が既に book にあるときは leaf を
    /// [`MOVE_NOMOVE`] にする。
    pub fn search_leaf(&mut self, board: &Board, level: i32, solver: &Solver) {
        let mut remaining = board.moves();
        for m in self.moves_with_value(board) {
            remaining &= !(1u64 << m.mv);
        }

        let leaf = if remaining == 0 {
            // 全ての手が book にある。
            Leaf {
                value: SCORE_UNDEFINED,
                mv: MOVE_NOMOVE,
                level: level.clamp(0, i8::MAX as i32) as i8,
            }
        } else {
            let result = solver.solve_with_moves(board, level, remaining);
            match result.best_move {
                Some(mv) => Leaf {
                    value: clamp_score(result.score),
                    mv: mv as i8,
                    level: level.clamp(0, i8::MAX as i32) as i8,
                },
                None => Leaf {
                    value: SCORE_UNDEFINED,
                    mv: MOVE_UNDEFINED,
                    level: LEVEL_UNDEFINED,
                },
            }
        };
        self.set_leaf(board, leaf);
    }

    /// leaf が古くなった局面 (leaf の手の先が book に入ってしまった、座標が
    /// 壊れている) の leaf を探索し直す。Egaroucid の `check_add_leaf_all_search`。
    ///
    /// 探索し直した局面数を返す。
    pub fn refresh_leaves(&mut self, level: i32, solver: &Solver) -> usize {
        let targets: Vec<Board> = self
            .positions
            .iter()
            .filter(|(board, elem)| self.leaf_needs_rewrite(board, &elem.leaf))
            .map(|(board, _)| *board)
            .collect();

        let n = targets.len();
        for board in targets {
            self.search_leaf(&board, level, solver);
        }
        n
    }

    /// 全局面の leaf を無条件に探索し直す。Egaroucid の `recalculate_leaf_all`。
    pub fn recalculate_leaves(&mut self, level: i32, solver: &Solver) -> usize {
        let targets: Vec<Board> = self.positions.keys().copied().collect();
        let n = targets.len();
        for board in targets {
            self.search_leaf(&board, level, solver);
        }
        n
    }

    /// leaf が使えない状態になっている局面の leaf を未設定に戻す。
    ///
    /// Egaroucid の `check_add_leaf_all_undefined`。探索は行わない。
    pub fn invalidate_stale_leaves(&mut self) -> usize {
        let targets: Vec<Board> = self
            .positions
            .iter()
            .filter(|(board, elem)| self.leaf_needs_rewrite(board, &elem.leaf))
            .map(|(board, _)| *board)
            .collect();

        let n = targets.len();
        for board in targets {
            if let Some(elem) = self.positions.get_mut(&board) {
                elem.leaf = Leaf::default();
            }
        }
        n
    }

    /// leaf を書き直す必要があるか。座標が壊れている、非合法、あるいは
    /// その手の先が既に book に入っている場合。
    fn leaf_needs_rewrite(&self, board: &Board, leaf: &Leaf) -> bool {
        if leaf.mv == MOVE_NOMOVE {
            // 「もう手が無い」印。子が消えて手が復活していないか確かめる。
            let mut remaining = board.moves();
            for m in self.moves_with_value(board) {
                remaining &= !(1u64 << m.mv);
            }
            return remaining != 0;
        }
        if !leaf.is_move() || board.moves() & (1u64 << leaf.mv) == 0 {
            return true;
        }
        // leaf の手の先が book に入ったら leaf ではなくなる。
        match next_board(board, leaf.mv) {
            Some(child) => {
                if child.moves() == 0 && child.opponent_moves() != 0 {
                    self.contains(&child.passed())
                } else {
                    self.contains(&child)
                }
            }
            None => true,
        }
    }

    /// leaf が book 内のどの手より良い局面について、その手の先を book に登録する。
    ///
    /// Egaroucid の `upgrade_better_leaves`。これが Egaroucid 流の book の
    /// 広げ方で、登録した局面数を返す。
    pub fn upgrade_better_leaves(&mut self) -> usize {
        self.upgrade_better_leaves_from(&Board::new())
    }

    /// `board` を根として [`Book::upgrade_better_leaves`] を行う。
    pub fn upgrade_better_leaves_from(&mut self, board: &Board) -> usize {
        let mut upgraded = 0;
        let mut visited = BTreeSet::new();
        let mut stack = vec![board.unique_board()];

        while let Some(key) = stack.pop() {
            if !visited.insert(key) {
                continue;
            }
            let Some(elem) = self.positions.get(&key).copied() else {
                continue;
            };

            let links = self.moves_with_value(&key);
            if !links.is_empty() && elem.leaf.is_move() && key.moves() & (1u64 << elem.leaf.mv) != 0
            {
                let link_max = links.iter().map(|l| l.value).max().unwrap_or(i8::MIN);
                if is_valid_score(elem.leaf.value) && elem.leaf.value > link_max {
                    if let Some(child) = next_board(&key, elem.leaf.mv) {
                        self.merge_elem(
                            &child,
                            BookElem {
                                value: clamp_score(-(elem.leaf.value as i32)),
                                level: elem.leaf.level,
                                leaf: Leaf::default(),
                                n_lines: 1,
                            },
                        );
                        upgraded += 1;
                        stack.push(child.unique_board());
                    }
                }
            }

            for link in self.moves_with_value(&key) {
                if let Some(child) = next_board(&key, link.mv as i8) {
                    stack.push(child.unique_board());
                }
            }
        }
        upgraded
    }

    /// leaf の手の先を book に登録して 1 段広げる。
    ///
    /// `max_error` はその局面の評価値と leaf の値の差の許容量。0 なら leaf が
    /// 最善のときだけ広げる。追加した局面数を返す。
    ///
    /// Egaroucid では book を広げる操作は GUI 側にあり engine には無い。leaf に
    /// 「まだ book に無い手の中の最善手」が入っているという Egaroucid の設計を
    /// そのまま使った、このエンジン側の育成 API。
    pub fn expand_leaves(&mut self, max_error: i32) -> usize {
        let targets: Vec<(Board, Leaf, i8)> = self
            .positions
            .iter()
            .filter(|(_, elem)| elem.leaf.is_move() && is_valid_score(elem.leaf.value))
            .map(|(board, elem)| (*board, elem.leaf, elem.value))
            .collect();

        let mut added = 0;
        for (board, leaf, value) in targets {
            if is_valid_score(value) && (value as i32 - leaf.value as i32) > max_error {
                continue;
            }
            let Some(child) = next_board(&board, leaf.mv) else {
                continue;
            };
            // パスが挟まる場合は手番が戻るので符号は反転しない。
            let (child, child_value) = if child.moves() == 0 && child.opponent_moves() != 0 {
                (child.passed(), leaf.value as i32)
            } else {
                (child, -(leaf.value as i32))
            };
            if self.merge_elem(
                &child,
                BookElem {
                    value: clamp_score(child_value),
                    level: leaf.level,
                    leaf: Leaf::default(),
                    n_lines: 1,
                },
            ) {
                added += 1;
            }
        }
        added
    }

    /// book を広げる。leaf の探索 → 展開 → negamax を `rounds` 回繰り返す。
    ///
    /// 追加した局面数を返す。`rounds` が 0 なら変化が無くなるまで繰り返す。
    pub fn expand(&mut self, rounds: usize, max_error: i32, level: i32, solver: &Solver) -> usize {
        let mut total = 0;
        let mut round = 0;
        loop {
            self.refresh_leaves(level, solver);
            let added = self.expand_leaves(max_error);
            self.upgrade_better_leaves();
            self.negamax(false);
            total += added;
            round += 1;
            if added == 0 || (rounds != 0 && round >= rounds) {
                break;
            }
        }
        // 展開した手の leaf は古くなっているので、保存前に探索し直す。
        if total > 0 {
            self.refresh_leaves(level, solver);
        }
        total
    }

    // ---- 値の伝播 ----

    /// 子局面から評価値を伝播させる。Egaroucid の `negamax_book`。
    ///
    /// `edax_compliant` が真なら leaf の値も候補に入れる (Edax と同じ扱い)。
    /// 値を書き換えた局面数を返す。
    pub fn negamax(&mut self, edax_compliant: bool) -> usize {
        self.negamax_from(&Board::new(), edax_compliant)
    }

    /// `board` を根として評価値を伝播させる。
    pub fn negamax_from(&mut self, board: &Board, edax_compliant: bool) -> usize {
        let mut n_fix = 0;
        for key in self.postorder(board.unique_board()) {
            n_fix += usize::from(self.negamax_one(key, edax_compliant));
        }
        n_fix
    }

    /// `n_lines` だけを計算し直す。Egaroucid の `recalculate_n_lines`。
    pub fn recalculate_n_lines(&mut self) {
        for key in self.postorder(Board::new().unique_board()) {
            let n_lines = self.subtree_n_lines(&key);
            if let Some(elem) = self.positions.get_mut(&key) {
                elem.n_lines = n_lines;
            }
        }
    }

    /// negamax と leaf の整合を取る。Egaroucid の `fix`。
    pub fn fix(&mut self, edax_compliant: bool) -> usize {
        let n_fix = self.negamax(edax_compliant);
        self.invalidate_stale_leaves();
        n_fix
    }

    /// negamax で辿る子局面。パス局面はここで畳む。
    fn successors(&self, board: &Board) -> Vec<(Board, i32)> {
        if board.moves() == 0 {
            if board.opponent_moves() == 0 {
                return Vec::new();
            }
            let passed = board.passed().unique_board();
            return if self.positions.contains_key(&passed) {
                vec![(passed, -1)]
            } else {
                Vec::new()
            };
        }
        self.child_entries_for_negamax(board)
    }

    /// `root` から辿れる局面を後行順 (子が先) に並べる。
    ///
    /// リバーシの局面グラフは石が増える一方なので閉路は無い。
    fn postorder(&self, root: Board) -> Vec<Board> {
        let mut order = Vec::new();
        let mut visited = BTreeSet::new();
        let mut stack = vec![(root, false)];

        while let Some((board, expanded)) = stack.pop() {
            if expanded {
                order.push(board);
                continue;
            }
            if !visited.insert(board) {
                continue;
            }
            if !self.positions.contains_key(&board) {
                continue;
            }
            stack.push((board, true));
            for (child, _) in self.successors(&board) {
                if !visited.contains(&child) {
                    stack.push((child, false));
                }
            }
        }
        order
    }

    /// 1 局面分の negamax。値を書き換えたら `true`。
    fn negamax_one(&mut self, key: Board, edax_compliant: bool) -> bool {
        let Some(elem) = self.positions.get(&key).copied() else {
            return false;
        };
        // 終局局面は子を持たないので、登録されている値をそのまま残す。
        if key.moves() == 0 && key.opponent_moves() == 0 {
            return false;
        }

        let mut best: Option<i8> = if edax_compliant && is_valid_score(elem.leaf.value) {
            Some(elem.leaf.value)
        } else {
            None
        };
        let mut n_lines: u64 = 1;

        for (child_key, sign) in self.successors(&key) {
            let Some(child) = self.positions.get(&child_key) else {
                continue;
            };
            if !child.has_value() {
                continue;
            }
            let value = clamp_score(sign * child.value as i32);
            if best.is_none_or(|b| b < value) {
                best = Some(value);
            }
            n_lines += child.n_lines as u64;
        }

        let n_lines = n_lines.min(MAX_N_LINES as u64) as u32;
        let position = self
            .positions
            .get_mut(&key)
            .expect("position exists during negamax");
        position.n_lines = n_lines;
        match best {
            Some(value) if value != position.value => {
                position.value = value;
                true
            }
            _ => false,
        }
    }

    fn subtree_n_lines(&self, key: &Board) -> u32 {
        let mut n_lines: u64 = 1;
        for (child_key, _) in self.successors(key) {
            if let Some(child) = self.positions.get(&child_key) {
                n_lines += child.n_lines as u64;
            }
        }
        n_lines.min(MAX_N_LINES as u64) as u32
    }

    // ---- 整理 ----

    /// 最善手から離れた変化を削る。Egaroucid の `reduce_book`。
    ///
    /// `max_depth` 手まで、1 手あたり `max_error_per_move` 以内、かつ 1 本の
    /// 変化を通しての誤差合計が `max_line_error` 以内の局面だけを残す。
    /// 削除した局面数を返す。
    pub fn reduce(
        &mut self,
        max_depth: usize,
        max_error_per_move: i32,
        max_line_error: i32,
    ) -> usize {
        self.reduce_from(&Board::new(), max_depth, max_error_per_move, max_line_error)
    }

    /// `board` を根として [`Book::reduce`] を行う。
    pub fn reduce_from(
        &mut self,
        board: &Board,
        max_depth: usize,
        max_error_per_move: i32,
        max_line_error: i32,
    ) -> usize {
        let root = board.unique_board();
        if !self.positions.contains_key(&root) {
            return 0;
        }

        // 残す局面を「まだ許される誤差」つきで幅優先に集める。
        let mut keep: BTreeMap<Board, i32> = BTreeMap::new();
        keep.insert(root, max_line_error);
        let mut frontier: BTreeMap<Board, i32> = BTreeMap::new();
        frontier.insert(root, max_line_error);

        for _ in 0..max_depth {
            if frontier.is_empty() {
                break;
            }
            let mut next: BTreeMap<Board, i32> = BTreeMap::new();
            for (board, remaining_error) in &frontier {
                let Some(elem) = self.positions.get(board) else {
                    continue;
                };
                let value = elem.value as i32;
                for link in self.moves_with_value(board) {
                    let link_error = value - link.value as i32;
                    if link_error > max_error_per_move || link_error > *remaining_error {
                        continue;
                    }
                    let Some(child) = next_board(board, link.mv as i8) else {
                        continue;
                    };
                    let child_key = child.unique_board();
                    let new_error = remaining_error - link_error;
                    let entry = next.entry(child_key).or_insert(new_error);
                    if *entry < new_error {
                        *entry = new_error;
                    }
                }
            }
            for (board, error) in &next {
                let entry = keep.entry(*board).or_insert(*error);
                if *entry < *error {
                    *entry = *error;
                }
            }
            frontier = next;
        }

        // 消える子を指していた手を leaf に退避させる。
        self.demote_links_to_leaf(&keep);

        let to_delete: Vec<Board> = self
            .positions
            .keys()
            .filter(|board| !keep.contains_key(*board))
            .copied()
            .collect();
        let n = to_delete.len();
        for board in to_delete {
            self.positions.remove(&board);
        }
        n
    }

    /// 削除予定の子を指す手のうち最善のものを leaf に移す。
    /// Egaroucid の `reduce_book_update_leaves`。
    fn demote_links_to_leaf(&mut self, keep: &BTreeMap<Board, i32>) {
        let kept: Vec<Board> = keep.keys().copied().collect();
        for board in kept {
            let Some(elem) = self.positions.get(&board).copied() else {
                continue;
            };
            let mut leaf = elem.leaf;
            let mut updated = false;

            for link in self.moves_with_value(&board) {
                let Some(child) = next_board(&board, link.mv as i8) else {
                    continue;
                };
                let child_key = child.unique_board();
                let passed_key = child.passed().unique_board();
                let will_be_deleted =
                    !keep.contains_key(&child_key) && !keep.contains_key(&passed_key);
                if !will_be_deleted {
                    continue;
                }
                if !is_valid_score(leaf.value) || leaf.value < link.value {
                    leaf = Leaf {
                        value: link.value,
                        mv: link.mv as i8,
                        level: self
                            .positions
                            .get(&child_key)
                            .or_else(|| self.positions.get(&passed_key))
                            .map(|e| e.level)
                            .unwrap_or(LEVEL_UNDEFINED),
                    };
                    updated = true;
                }
            }

            if updated {
                if let Some(elem) = self.positions.get_mut(&board) {
                    elem.leaf = leaf;
                }
            }
        }
    }

    /// 子を 1 つも持たず、値が中盤探索由来の局面を削除する。
    ///
    /// Egaroucid の `delete_terminal_midsearch`。終盤完全読みまで届いていない
    /// 「行き止まり」を消して、次の育成でやり直せるようにする。
    pub fn delete_terminal_midsearch(&mut self) -> usize {
        let targets: Vec<Board> = self
            .positions
            .iter()
            .filter(|(board, elem)| {
                if board.moves() == 0 {
                    return false;
                }
                if !self.moves_with_value(board).is_empty() {
                    return false;
                }
                // level に対して終盤完全読みが届いていなければ中盤由来。
                !matches!(
                    solver_type_for_level(board.empties_count() as i32, elem.level as i32),
                    SolverType::Final(_)
                )
            })
            .map(|(board, _)| *board)
            .collect();

        let n = targets.len();
        for board in targets {
            self.positions.remove(&board);
        }
        n
    }

    /// 初期盤面から辿れない局面を削除する。削除した局面数を返す。
    pub fn remove_unreachable(&mut self) -> usize {
        let reachable: BTreeSet<Board> = self
            .postorder(Board::new().unique_board())
            .into_iter()
            .collect();
        let to_delete: Vec<Board> = self
            .positions
            .keys()
            .filter(|board| !reachable.contains(*board))
            .copied()
            .collect();
        let n = to_delete.len();
        for board in to_delete {
            self.positions.remove(&board);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;
    use crate::search::SolverOptions;
    use std::sync::Arc;

    const D3: u8 = 19;

    fn test_solver() -> Solver {
        Solver::with_options(Arc::new(Evaluator::default()), SolverOptions::default())
    }

    fn seeded_book() -> Book {
        let mut book = Book::empty();
        book.set_random_seed(3);
        book
    }

    #[test]
    fn add_board_registers_a_searched_position() {
        let solver = test_solver();
        let mut book = seeded_book();

        assert!(book.add_board(&Board::new(), 2, &solver));
        assert!(book.contains(&Board::new()));
        assert!(book.root().unwrap().has_value());
        // 2 回目は追加ではなく更新。
        assert!(!book.add_board(&Board::new(), 2, &solver));
        assert_eq!(book.len(), 1);
    }

    #[test]
    fn add_record_registers_every_position_of_the_line() {
        let solver = test_solver();
        let mut book = seeded_book();

        let added = book.add_record("F5D6C3", 60, 2, &solver).unwrap();
        assert_eq!(added, 4);
        assert!(book.contains(&Board::new()));

        // 親から子が手として見えるようになる。
        assert!(!book.moves_with_value(&Board::new()).is_empty());
    }

    #[test]
    fn add_record_rejects_illegal_records() {
        let solver = test_solver();
        let mut book = seeded_book();
        assert!(book.add_record("A1", 60, 2, &solver).is_err());
        assert!(book.add_record("F5D", 60, 2, &solver).is_err());
    }

    #[test]
    fn search_leaf_only_considers_moves_that_are_not_in_the_book() {
        let solver = test_solver();
        let mut book = seeded_book();
        book.add_board(&Board::new(), 2, &solver);

        // まだ子が無いので leaf は合法手のどれか。
        book.search_leaf(&Board::new(), 2, &solver);
        let leaf = book.root().unwrap().leaf;
        assert!(leaf.is_move());
        assert_ne!(Board::new().moves() & (1u64 << leaf.mv), 0);

        // 4 手すべて登録すると (対称形なので子は 1 つ) leaf は NOMOVE になる。
        let child = Board::new().make_move(1u64 << D3);
        book.add_board(&child, 2, &solver);
        book.search_leaf(&Board::new(), 2, &solver);
        assert_eq!(book.root().unwrap().leaf.mv, MOVE_NOMOVE);
    }

    #[test]
    fn search_leaf_result_is_stored_in_the_representative_orientation() {
        let solver = test_solver();
        let mut book = seeded_book();
        let board = Board::new().make_move(1u64 << D3);
        book.add_board(&board, 2, &solver);
        book.search_leaf(&board, 2, &solver);

        // どの対称形から引いても、その向きの合法手として返る。
        for sym in board.all_symmetries() {
            let leaf = book.get(&sym).unwrap().leaf;
            assert!(leaf.is_move());
            assert_ne!(sym.moves() & (1u64 << leaf.mv), 0);
        }
    }

    #[test]
    fn invalidate_stale_leaves_clears_leaves_whose_child_got_registered() {
        let solver = test_solver();
        let mut book = seeded_book();
        book.add_board(&Board::new(), 2, &solver);
        book.search_leaf(&Board::new(), 2, &solver);
        let leaf_move = book.root().unwrap().leaf.mv;

        // leaf の手の先を登録すると leaf は無効になる。
        let child = Board::new().make_move(1u64 << leaf_move);
        book.add_board(&child, 2, &solver);
        assert!(book.invalidate_stale_leaves() >= 1);
        assert!(!book.root().unwrap().leaf.is_move());
    }

    #[test]
    fn negamax_propagates_the_child_value_to_the_parent() {
        let mut book = Book::empty();
        let child = Board::new().make_move(1u64 << D3);
        book.register(&Board::new(), BookElem::new(0, 10));
        book.register(&child, BookElem::new(-5, 10));

        assert_eq!(book.negamax(false), 1);
        assert_eq!(book.root().unwrap().value, 5);
        // n_lines は 1 + 各 link の子の n_lines。初期盤面の 4 手は同じ正規形の
        // 子に落ちるので、その子が 4 回数えられる (Egaroucid も同じ)。
        assert_eq!(
            book.root().unwrap().n_lines,
            1 + 4 * book.get(&child).unwrap().n_lines
        );
    }

    #[test]
    fn negamax_ignores_the_leaf_unless_edax_compliant() {
        let mut book = Book::empty();
        let child = Board::new().make_move(1u64 << D3);
        book.register(&child, BookElem::new(-1, 10));
        book.register(
            &Board::new(),
            BookElem {
                value: 0,
                level: 10,
                leaf: Leaf {
                    value: 20,
                    mv: 26,
                    level: 10,
                },
                n_lines: 0,
            },
        );

        book.negamax(false);
        assert_eq!(book.root().unwrap().value, 1, "leaf must be ignored");

        book.negamax(true);
        assert_eq!(book.root().unwrap().value, 20, "leaf must win in edax mode");
    }

    #[test]
    fn negamax_handles_a_pass_between_two_book_positions() {
        // パスを挟む親子を作り、符号が 1 回だけ反転することを確かめる。
        let mut book = Book::empty();
        let mut board = Board::new();
        let mut found = None;
        for _ in 0..50 {
            let legal = board.moves();
            if legal == 0 {
                break;
            }
            let mut bits = legal;
            while bits != 0 {
                let mv = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                let child = board.make_move(1u64 << mv);
                if child.moves() == 0 && child.opponent_moves() != 0 {
                    found = Some((board, child.passed()));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
            board = board.make_move(1u64 << legal.trailing_zeros());
        }
        let Some((parent, grandchild)) = found else {
            return;
        };

        book.register(&parent, BookElem::new(0, 5));
        book.register(&grandchild, BookElem::new(7, 5));
        book.negamax_from(&parent, false);
        // パスで手番が戻るので符号は反転しない。
        assert_eq!(book.get(&parent).unwrap().value, 7);
    }

    #[test]
    fn upgrade_better_leaves_registers_the_leaf_child() {
        // Egaroucid と同じく、既に link を持つ局面だけが対象になる。
        // D3 の局面には 3 通りの子があるので、1 つだけ登録しておく。
        let mut book = Book::empty();
        let parent = Board::new().make_move(1u64 << D3);
        let mut legal = parent.moves();
        let registered_move = legal.trailing_zeros() as u8;
        legal &= legal - 1;
        let leaf_move = legal.trailing_zeros() as u8;

        book.register(
            &parent.make_move(1u64 << registered_move),
            BookElem::new(0, 10),
        );
        book.register(
            &parent,
            BookElem {
                value: 0,
                level: 10,
                leaf: Leaf {
                    value: 4,
                    mv: leaf_move as i8,
                    level: 10,
                },
                n_lines: 0,
            },
        );

        assert_eq!(book.upgrade_better_leaves_from(&parent), 1);
        let child = parent.make_move(1u64 << leaf_move);
        assert!(book.contains(&child));
        assert_eq!(book.get(&child).unwrap().value, -4);
    }

    #[test]
    fn upgrade_better_leaves_skips_leaves_that_are_not_better() {
        let mut book = Book::empty();
        let child = Board::new().make_move(1u64 << D3);
        book.register(&child, BookElem::new(-9, 10)); // 親から見ると +9
        book.register(
            &Board::new(),
            BookElem {
                value: 9,
                level: 10,
                leaf: Leaf {
                    value: 1,
                    mv: 26,
                    level: 10,
                },
                n_lines: 0,
            },
        );

        assert_eq!(book.upgrade_better_leaves(), 0);
    }

    #[test]
    fn expand_leaves_registers_the_child_of_every_good_leaf() {
        let mut book = Book::empty();
        book.register(
            &Board::new(),
            BookElem {
                value: 4,
                level: 10,
                leaf: Leaf {
                    value: 4,
                    mv: D3 as i8,
                    level: 10,
                },
                n_lines: 0,
            },
        );

        assert_eq!(book.expand_leaves(0), 1);
        let child = Board::new().make_move(1u64 << D3);
        assert!(book.contains(&child));
        assert_eq!(book.get(&child).unwrap().value, -4);
    }

    #[test]
    fn expand_leaves_skips_leaves_outside_the_error_budget() {
        let mut book = Book::empty();
        book.register(
            &Board::new(),
            BookElem {
                value: 4,
                level: 10,
                leaf: Leaf {
                    value: -3, // 7 点悪い
                    mv: D3 as i8,
                    level: 10,
                },
                n_lines: 0,
            },
        );

        assert_eq!(book.expand_leaves(2), 0);
        assert_eq!(book.expand_leaves(7), 1);
    }

    #[test]
    fn expand_grows_the_book_and_keeps_it_consistent() {
        let solver = test_solver();
        let mut book = seeded_book();
        book.add_board(&Board::new(), 2, &solver);

        let added = book.expand(3, 4, 2, &solver);
        assert!(added > 0, "expand should register at least one position");
        assert!(book.len() > 1);

        // 追加された局面は親から合法手として見える。
        for (board, _) in book.iter() {
            for m in book.moves_with_value(board) {
                assert_ne!(board.moves() & (1u64 << m.mv), 0);
            }
        }
        assert!(book.best_move(&Board::new()).is_some());
    }

    #[test]
    fn expanded_book_survives_an_egbk3_round_trip() {
        let solver = test_solver();
        let mut book = seeded_book();
        book.add_board(&Board::new(), 2, &solver);
        book.expand(2, 4, 2, &solver);

        let loaded = Book::from_egbk3_bytes(&book.to_egbk3_bytes()).unwrap();
        assert_eq!(loaded.len(), book.len());
        for ((ba, ea), (bb, eb)) in book.iter().zip(loaded.iter()) {
            assert_eq!(ba, bb);
            assert_eq!(ea, eb);
        }
    }

    /// D3 の局面には対称でない子が 3 つあるので、誤差の効き方を見るのに使える。
    fn book_with_three_distinct_children() -> (Book, Board, Vec<(u8, Board)>) {
        let parent = Board::new().make_move(1u64 << D3);
        let mut children = Vec::new();
        let mut legal = parent.moves();
        while legal != 0 {
            let mv = legal.trailing_zeros() as u8;
            legal &= legal - 1;
            children.push((mv, parent.make_move(1u64 << mv)));
        }
        assert_eq!(children.len(), 3, "D3 の局面は 3 手ある");

        let mut book = Book::empty();
        book.register(&parent, BookElem::new(0, 10));
        (book, parent, children)
    }

    #[test]
    fn reduce_keeps_only_moves_within_the_error_budget() {
        let (mut book, parent, children) = book_with_three_distinct_children();
        // 親から見た値が 0 / -1 / -5 になるように子を登録する。
        book.register(&children[0].1, BookElem::new(0, 10));
        book.register(&children[1].1, BookElem::new(1, 10));
        book.register(&children[2].1, BookElem::new(5, 10));
        book.negamax_from(&parent, false);
        assert_eq!(book.get(&parent).unwrap().value, 0);
        assert_eq!(book.len(), 4);

        // 誤差 1 までなら 0 と -1 の子が残り、-5 の子だけ消える。
        let mut narrow = book.clone();
        assert_eq!(narrow.reduce_from(&parent, 5, 1, 1), 1);
        assert!(narrow.contains(&children[0].1));
        assert!(narrow.contains(&children[1].1));
        assert!(!narrow.contains(&children[2].1));

        // 誤差 0 なら最善手の子だけ残る。
        assert_eq!(book.reduce_from(&parent, 5, 0, 0), 2);
        assert!(book.contains(&parent));
        assert!(book.contains(&children[0].1));
        assert!(!book.contains(&children[1].1));
    }

    #[test]
    fn reduce_moves_a_deleted_link_into_the_leaf() {
        let (mut book, parent, children) = book_with_three_distinct_children();
        book.register(&children[0].1, BookElem::new(0, 10)); // 親から見て 0
        book.register(&children[1].1, BookElem::new(3, 10)); // 親から見て -3
        book.negamax_from(&parent, false);

        book.reduce_from(&parent, 5, 0, 0);
        assert!(!book.contains(&children[1].1));

        // 消えた手のうち最善のものが leaf に残る。
        let leaf = book.get(&parent).unwrap().leaf;
        assert!(leaf.is_move());
        assert_eq!(leaf.value, -3);
        assert_eq!(leaf.mv as u8, children[1].0);
    }

    #[test]
    fn remove_unreachable_drops_orphan_positions() {
        let solver = test_solver();
        let mut book = seeded_book();
        book.add_board(&Board::new(), 2, &solver);

        // 初期盤面から 4 手進んだ、link で繋がらない局面を足す。
        let mut orphan = Board::new();
        for _ in 0..4 {
            orphan = orphan.make_move(1u64 << orphan.moves().trailing_zeros());
        }
        book.register(&orphan, BookElem::new(0, 2));

        assert_eq!(book.remove_unreachable(), 1);
        assert!(!book.contains(&orphan));
        assert!(book.contains(&Board::new()));
    }

    #[test]
    fn delete_terminal_midsearch_removes_dead_ends_evaluated_in_the_midgame() {
        let mut book = Book::empty();
        // level 2 では空きマス 60 は中盤探索なので削除対象。
        book.register(&Board::new(), BookElem::new(0, 2));
        assert_eq!(book.delete_terminal_midsearch(), 1);
        assert!(book.is_empty());

        // level 60 なら完全読みの範囲なので残る。
        let mut book = Book::empty();
        book.register(&Board::new(), BookElem::new(0, 60));
        assert_eq!(book.delete_terminal_midsearch(), 0);
    }

    #[test]
    fn recalculate_n_lines_counts_the_subtree() {
        let mut book = Book::empty();
        let child = Board::new().make_move(1u64 << D3);
        book.register(&Board::new(), BookElem::new(0, 10));
        book.register(&child, BookElem::new(0, 10));

        book.recalculate_n_lines();
        assert_eq!(book.get(&child).unwrap().n_lines, 1);
        // 初期盤面の 4 手はすべて同じ子に落ちるので 4 回数えられる。
        assert_eq!(book.root().unwrap().n_lines, 1 + 4);
    }
}
