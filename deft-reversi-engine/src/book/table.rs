//! book の局面表と、走査用の世代印。
//!
//! 大きな book (数千万局面) を扱えるようにするための土台。
//!
//! - 盤面をキーにしたハッシュ表。ハッシュは 2 つの u64 を splitmix64 で混ぜる
//!   だけなので、既定の SipHash より一桁速い
//! - 各エントリに走査の世代印 ([`Entry::visited`]) を持たせ、木を辿るときに
//!   `HashSet<Board>` を作らなくて済むようにしている。世代を進めるだけで
//!   「全部未訪問」に戻せるので、走査の前処理も O(1)
//!
//! 1000 万局面の読み込みで `BTreeMap` の 8.95 秒に対して 0.68 秒 (実測)。

use super::elem::BookElem;
use crate::board::board::Board;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// 盤面 2 つの u64 を splitmix64 の finalizer で混ぜるハッシュ。
///
/// `Board` の derive した `Hash` は `write_u64` を 2 回呼ぶだけなので、
/// バイト列経路は通らない。
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
pub type BoardMap = HashMap<Board, Entry, BoardBuildHasher>;

/// 走査の世代印がまだ一度も使われていないことを表す値。
const UNVISITED: u32 = 0;

/// 表に入る 1 エントリ。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub elem: BookElem,
    /// 走査の世代印。[`Generation`] と一致していれば訪問済み。
    /// ファイルには保存しない。
    visited: u32,
}

impl Entry {
    pub fn new(elem: BookElem) -> Self {
        Self {
            elem,
            visited: UNVISITED,
        }
    }
}

/// 走査 1 回分の世代。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(u32);

impl Entry {
    /// この世代で訪問済みか。
    #[inline]
    pub fn is_visited(&self, generation: Generation) -> bool {
        self.visited == generation.0
    }

    /// この世代で訪問済みにする。すでに訪問済みなら `false`。
    #[inline]
    pub fn mark_visited(&mut self, generation: Generation) -> bool {
        if self.visited == generation.0 {
            return false;
        }
        self.visited = generation.0;
        true
    }
}

/// 世代を配る側。`Book` が 1 つ持つ。
#[derive(Clone, Debug, Default)]
pub struct Generations {
    current: u32,
}

impl Generations {
    /// 次の走査を始める。全エントリが未訪問に戻る。
    ///
    /// 世代が一周したときだけ、全エントリの印を消してから配り直す。
    pub fn next(&mut self, positions: &mut BoardMap) -> Generation {
        match self.current.checked_add(1) {
            Some(next) => self.current = next,
            None => {
                for entry in positions.values_mut() {
                    entry.visited = UNVISITED;
                }
                self.current = 1;
            }
        }
        Generation(self.current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::BuildHasher;

    fn hash_of(board: &Board) -> u64 {
        BoardBuildHasher::default().hash_one(board)
    }

    #[test]
    fn hash_differs_for_boards_that_differ_in_one_bit() {
        let base = Board::new();
        let mut collisions = 0;
        let base_hash = hash_of(&base);
        for bit in 0..64 {
            let moved = Board {
                player: base.player ^ (1u64 << bit),
                opponent: base.opponent,
            };
            if hash_of(&moved) == base_hash {
                collisions += 1;
            }
        }
        assert_eq!(collisions, 0);
    }

    #[test]
    fn hash_depends_on_both_bitboards() {
        let a = Board {
            player: 0x1234,
            opponent: 0x5678,
        };
        let b = Board {
            player: 0x5678,
            opponent: 0x1234,
        };
        assert_ne!(hash_of(&a), hash_of(&b));
    }

    /// 分布が偏っていないことを軽く確認する (下位ビットが均等に散るか)。
    #[test]
    fn hash_spreads_sequential_boards() {
        let mut buckets = [0usize; 64];
        for i in 0..64u64 {
            for j in 0..64u64 {
                let board = Board {
                    player: 1u64 << i,
                    opponent: 1u64 << j,
                };
                buckets[(hash_of(&board) % 64) as usize] += 1;
            }
        }
        // 完全一様なら各 64。極端な偏りだけを弾く。
        let max = *buckets.iter().max().unwrap();
        assert!(max < 64 * 4, "hash is badly distributed: max bucket {max}");
    }

    #[test]
    fn generations_mark_and_reset_without_touching_entries() {
        let mut positions = BoardMap::default();
        positions.insert(Board::new(), Entry::new(BookElem::default()));
        let mut generations = Generations::default();

        let g1 = generations.next(&mut positions);
        let entry = positions.get_mut(&Board::new()).unwrap();
        assert!(!entry.is_visited(g1));
        assert!(entry.mark_visited(g1));
        assert!(!entry.mark_visited(g1), "2 回目は false");
        assert!(entry.is_visited(g1));

        // 世代を進めるだけで未訪問に戻る。
        let g2 = generations.next(&mut positions);
        assert!(!positions[&Board::new()].is_visited(g2));
    }

    #[test]
    fn generation_wraparound_clears_the_marks() {
        let mut positions = BoardMap::default();
        positions.insert(Board::new(), Entry::new(BookElem::default()));
        let mut generations = Generations { current: u32::MAX };

        let g = generations.next(&mut positions);
        assert_eq!(g, Generation(1));
        assert!(!positions[&Board::new()].is_visited(g));
    }
}
