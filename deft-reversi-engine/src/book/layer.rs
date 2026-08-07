//! 手数 (ply) ごとの層に分けた局面表。
//!
//! book は初期局面から手を進めた DAG で、**子は必ず 1 手多い**という強い構造を
//! 持つ。この構造を素直に持つと、ハッシュ表に全局面をばらまく持ち方より扱いが
//! ずっと良くなる。
//!
//! - **評価値の伝播が線形走査になる**。ply の大きい方から処理すれば子は必ず
//!   確定済みなので、木を辿る必要も訪問済み判定も要らない
//! - **同じ ply の局面は互いに独立**。層の中はそのまま並列に処理できる
//!   (親子関係が層をまたぐので、層の中で依存が生じない)
//! - **参照の局所性が上がる**。子を引くのは次の層の索引だけで、層が小さいほど
//!   キャッシュに乗る
//! - **「25 手目まで」といった深さの制限が自然に書ける**
//!
//! ply は石数 - 4 で 0..=60。パスは石数を変えないので、ある局面とそのパス後の
//! 局面は同じ層に入る。

use super::sym::representative_board;
use super::value::{BookValue, Frontier};
use crate::board::board::Board;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// ply の最大値 (初期局面から 60 手)。
pub const MAX_PLY: usize = 60;
/// 層の数。
pub const N_LAYERS: usize = MAX_PLY + 1;

/// 盤面 2 つの u64 を splitmix64 の finalizer で混ぜるハッシュ。
///
/// `Board` の derive した `Hash` は `write_u64` を 2 回呼ぶだけなので、
/// バイト列経路は通らない。既定の SipHash より一桁速い。
#[derive(Default)]
pub struct BoardHasher(u64);

impl Hasher for BoardHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // Board からは呼ばれない経路。念のため正しく畳んでおく。
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }

    #[inline]
    fn write_u64(&mut self, v: u64) {
        let mut h = self.0 ^ v;
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        self.0 = h ^ (h >> 31);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

pub type BoardBuildHasher = BuildHasherDefault<BoardHasher>;

/// 盤面の ply (石数 - 4)。
#[inline]
pub fn ply_of(board: &Board) -> usize {
    ((board.player | board.opponent).count_ones() as usize).saturating_sub(4)
}

/// 局面の場所。層番号と層内の添字。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositionId {
    pub ply: u8,
    pub slot: u32,
}

/// 手数 1 つ分の層。
///
/// 盤面・値・frontier を別々の配列に持つ (列指向)。走査は 1 つの配列だけを
/// 舐めれば済むことが多く、キャッシュに無駄が乗らない。
#[derive(Clone, Debug, Default)]
pub struct PlyLayer {
    boards: Vec<Board>,
    values: Vec<BookValue>,
    frontiers: Vec<Frontier>,
    index: HashMap<Board, u32, BoardBuildHasher>,
}

impl PlyLayer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            boards: Vec::with_capacity(capacity),
            values: Vec::with_capacity(capacity),
            frontiers: Vec::with_capacity(capacity),
            index: HashMap::with_capacity_and_hasher(capacity, Default::default()),
        }
    }

    /// 列を丸ごと受け取って層を作る。索引はここで一度に張る。
    ///
    /// 読み込みのホットパス。層どうしは独立なので、層ごとに並列に呼べる。
    /// 重複した盤面は先勝ちで捨てる。
    pub fn from_columns(
        boards: Vec<Board>,
        mut values: Vec<BookValue>,
        mut frontiers: Vec<Frontier>,
    ) -> Self {
        let n = boards.len();
        debug_assert_eq!(values.len(), n);
        values.resize(n, BookValue::undefined());
        frontiers.resize(n, Frontier::unset());

        let mut layer = Self {
            boards,
            values,
            frontiers,
            index: HashMap::with_capacity_and_hasher(n, Default::default()),
        };
        for (slot, board) in layer.boards.iter().enumerate() {
            layer.index.entry(*board).or_insert(slot as u32);
        }
        if layer.index.len() != n {
            // 重複があったので、索引が指している方だけを残して詰め直す。
            let index = std::mem::take(&mut layer.index);
            let mut slot = 0u32;
            layer.retain(|board, _| {
                let keep = index.get(board) == Some(&slot);
                slot += 1;
                keep
            });
        }
        layer
    }

    pub fn len(&self) -> usize {
        self.boards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.boards.is_empty()
    }

    /// 正規形の盤面から層内の添字を引く。
    #[inline]
    pub fn slot_of(&self, representative: &Board) -> Option<u32> {
        self.index.get(representative).copied()
    }

    #[inline]
    pub fn board(&self, slot: u32) -> &Board {
        &self.boards[slot as usize]
    }

    #[inline]
    pub fn value(&self, slot: u32) -> &BookValue {
        &self.values[slot as usize]
    }

    #[inline]
    pub fn value_mut(&mut self, slot: u32) -> &mut BookValue {
        &mut self.values[slot as usize]
    }

    #[inline]
    pub fn frontier(&self, slot: u32) -> &Frontier {
        &self.frontiers[slot as usize]
    }

    #[inline]
    pub fn frontier_mut(&mut self, slot: u32) -> &mut Frontier {
        &mut self.frontiers[slot as usize]
    }

    pub fn boards(&self) -> &[Board] {
        &self.boards
    }

    pub fn values(&self) -> &[BookValue] {
        &self.values
    }

    pub fn frontiers(&self) -> &[Frontier] {
        &self.frontiers
    }

    /// 正規形の盤面を追加する。既にあればその添字を返す。
    pub fn insert(&mut self, representative: Board, value: BookValue, frontier: Frontier) -> u32 {
        if let Some(slot) = self.slot_of(&representative) {
            return slot;
        }
        let slot = self.boards.len() as u32;
        self.boards.push(representative);
        self.values.push(value);
        self.frontiers.push(frontier);
        self.index.insert(representative, slot);
        slot
    }

    /// 正規形の盤面を追加する。既にあれば信用できる方を残す。
    ///
    /// 値は [`BookValue::is_superseded_by`]、frontier は「未設定なら埋める」
    /// で取り込む。取り込み (import / merge) のホットパス。
    pub fn upsert(&mut self, representative: Board, value: BookValue, frontier: Frontier) -> u32 {
        let before = self.boards.len();
        let slot = self.insert(representative, value, frontier);
        if self.boards.len() != before {
            return slot;
        }
        let current = &mut self.values[slot as usize];
        if current.is_superseded_by(&value) {
            *current = value;
        }
        let current = &mut self.frontiers[slot as usize];
        if current.is_unset() && !frontier.is_unset() {
            *current = frontier;
        }
        slot
    }

    /// 条件を満たさない局面を削除して詰め直す。削除した数を返す。
    ///
    /// 添字が変わるので、外に控えた [`PositionId`] は無効になる。
    pub fn retain(&mut self, mut keep: impl FnMut(&Board, &BookValue) -> bool) -> usize {
        let before = self.boards.len();
        let mut write = 0usize;
        for read in 0..self.boards.len() {
            if keep(&self.boards[read], &self.values[read]) {
                if write != read {
                    self.boards[write] = self.boards[read];
                    self.values[write] = self.values[read];
                    self.frontiers[write] = self.frontiers[read];
                }
                write += 1;
            }
        }
        self.boards.truncate(write);
        self.values.truncate(write);
        self.frontiers.truncate(write);

        self.index.clear();
        for (slot, board) in self.boards.iter().enumerate() {
            self.index.insert(*board, slot as u32);
        }
        before - write
    }
}

/// ply 層に分けた局面表。
#[derive(Clone, Debug)]
pub struct LayeredTable {
    layers: Vec<PlyLayer>,
}

impl Default for LayeredTable {
    fn default() -> Self {
        Self::new()
    }
}

impl LayeredTable {
    pub fn new() -> Self {
        Self {
            layers: (0..N_LAYERS).map(|_| PlyLayer::default()).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.layers.iter().map(PlyLayer::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(PlyLayer::is_empty)
    }

    pub fn layer(&self, ply: usize) -> &PlyLayer {
        &self.layers[ply]
    }

    pub fn layer_mut(&mut self, ply: usize) -> &mut PlyLayer {
        &mut self.layers[ply]
    }

    /// 局面が入っている最も深い ply。空なら `None`。
    pub fn max_ply(&self) -> Option<usize> {
        (0..N_LAYERS)
            .rev()
            .find(|&ply| !self.layers[ply].is_empty())
    }

    /// 盤面 (対称形を含む) の場所を引く。
    pub fn locate(&self, board: &Board) -> Option<PositionId> {
        let ply = ply_of(board);
        let (representative, _) = representative_board(board);
        self.locate_representative(ply, &representative)
    }

    /// 正規形の盤面と ply から場所を引く。
    #[inline]
    pub fn locate_representative(&self, ply: usize, representative: &Board) -> Option<PositionId> {
        let slot = self.layers.get(ply)?.slot_of(representative)?;
        Some(PositionId {
            ply: ply as u8,
            slot,
        })
    }

    #[inline]
    pub fn value(&self, id: PositionId) -> &BookValue {
        self.layers[id.ply as usize].value(id.slot)
    }

    #[inline]
    pub fn value_mut(&mut self, id: PositionId) -> &mut BookValue {
        self.layers[id.ply as usize].value_mut(id.slot)
    }

    #[inline]
    pub fn frontier(&self, id: PositionId) -> &Frontier {
        self.layers[id.ply as usize].frontier(id.slot)
    }

    #[inline]
    pub fn frontier_mut(&mut self, id: PositionId) -> &mut Frontier {
        self.layers[id.ply as usize].frontier_mut(id.slot)
    }

    #[inline]
    pub fn board(&self, id: PositionId) -> &Board {
        self.layers[id.ply as usize].board(id.slot)
    }

    /// 正規形の盤面を追加する。
    pub fn insert(
        &mut self,
        representative: Board,
        value: BookValue,
        frontier: Frontier,
    ) -> PositionId {
        let ply = ply_of(&representative);
        let slot = self.layers[ply].insert(representative, value, frontier);
        PositionId {
            ply: ply as u8,
            slot,
        }
    }

    /// 全局面を ply の浅い順に走査する。
    pub fn iter(&self) -> impl Iterator<Item = (PositionId, &Board, &BookValue)> {
        self.layers.iter().enumerate().flat_map(|(ply, layer)| {
            layer.boards().iter().zip(layer.values()).enumerate().map(
                move |(slot, (board, value))| {
                    (
                        PositionId {
                            ply: ply as u8,
                            slot: slot as u32,
                        },
                        board,
                        value,
                    )
                },
            )
        })
    }

    pub fn clear(&mut self) {
        for layer in &mut self.layers {
            *layer = PlyLayer::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::value::BookValue;

    const D3: u8 = 19;

    #[test]
    fn ply_is_the_move_count() {
        assert_eq!(ply_of(&Board::new()), 0);
        let board = Board::new().make_move(1u64 << D3);
        assert_eq!(ply_of(&board), 1);
    }

    #[test]
    fn a_passed_board_stays_in_the_same_layer() {
        let board = Board::new().make_move(1u64 << D3);
        assert_eq!(ply_of(&board), ply_of(&board.passed()));
    }

    #[test]
    fn insert_and_locate_round_trip_through_symmetry() {
        let mut table = LayeredTable::new();
        let board = Board::new().make_move(1u64 << D3);
        let (representative, _) = representative_board(&board);

        let id = table.insert(representative, BookValue::exact(4), Frontier::unset());
        assert_eq!(id.ply, 1);

        // どの対称形から引いても同じ場所に着く。
        for sym in board.all_symmetries() {
            assert_eq!(table.locate(&sym), Some(id));
        }
        assert_eq!(table.value(id).score, 4);
    }

    #[test]
    fn insert_is_idempotent() {
        let mut table = LayeredTable::new();
        let board = Board::new();
        let a = table.insert(board, BookValue::exact(0), Frontier::unset());
        let b = table.insert(board, BookValue::exact(9), Frontier::unset());
        assert_eq!(a, b);
        assert_eq!(table.len(), 1);
        // 2 回目は無視される (値の更新は呼び出し側の責任)。
        assert_eq!(table.value(a).score, 0);
    }

    #[test]
    fn layers_are_separate() {
        let mut table = LayeredTable::new();
        let child = Board::new().make_move(1u64 << D3);
        table.insert(Board::new(), BookValue::exact(0), Frontier::unset());
        table.insert(
            representative_board(&child).0,
            BookValue::exact(1),
            Frontier::unset(),
        );

        assert_eq!(table.layer(0).len(), 1);
        assert_eq!(table.layer(1).len(), 1);
        assert_eq!(table.len(), 2);
        assert_eq!(table.max_ply(), Some(1));
    }

    #[test]
    fn retain_compacts_and_reindexes() {
        let mut layer = PlyLayer::default();
        let boards: Vec<Board> = (0..5)
            .map(|i| Board {
                player: 1u64 << i,
                opponent: 1u64 << (i + 32),
            })
            .collect();
        for (i, board) in boards.iter().enumerate() {
            layer.insert(*board, BookValue::exact(i as i8), Frontier::unset());
        }

        // 偶数番だけ残す。
        let mut seen = 0;
        let removed = layer.retain(|_, _| {
            let keep = seen % 2 == 0;
            seen += 1;
            keep
        });

        assert_eq!(removed, 2);
        assert_eq!(layer.len(), 3);
        // 索引が詰め直されていること。
        for (slot, board) in [(0u32, boards[0]), (1, boards[2]), (2, boards[4])] {
            assert_eq!(layer.slot_of(&board), Some(slot));
            assert_eq!(*layer.board(slot), board);
        }
        assert_eq!(layer.slot_of(&boards[1]), None);
    }

    #[test]
    fn iter_walks_shallow_layers_first() {
        let mut table = LayeredTable::new();
        let child = Board::new().make_move(1u64 << D3);
        table.insert(
            representative_board(&child).0,
            BookValue::exact(1),
            Frontier::unset(),
        );
        table.insert(Board::new(), BookValue::exact(0), Frontier::unset());

        let plies: Vec<u8> = table.iter().map(|(id, _, _)| id.ply).collect();
        assert_eq!(plies, vec![0, 1]);
    }
}
