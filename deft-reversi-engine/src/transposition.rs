//! All payload fields are atomic: a rejected snapshot is just a cache miss.
use crate::Board;
use std::sync::atomic::{AtomicU64, Ordering, fence};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    pub lower: i8,
    pub upper: i8,
    pub depth: u8,
    pub quality: u8,
    pub best: u8,
    pub second: u8,
    pub generation: u8,
    pub exact_mode: bool,
}
impl Entry {
    fn pack(self) -> u64 {
        u64::from_le_bytes([
            self.lower as u8,
            self.upper as u8,
            self.depth,
            self.quality,
            self.best,
            self.generation,
            128 | self.exact_mode as u8,
            self.second,
        ])
    }
    fn unpack(bits: u64) -> Option<Self> {
        let b = bits.to_le_bytes();
        (b[6] & 128 != 0).then_some(Self {
            lower: b[0] as i8,
            upper: b[1] as i8,
            depth: b[2],
            quality: b[3],
            best: b[4],
            generation: b[5],
            exact_mode: b[6] & 1 != 0,
            second: b[7],
        })
    }
}
#[derive(Default)]
struct Slot {
    sequence: AtomicU64,
    player: AtomicU64,
    opponent: AtomicU64,
    data: AtomicU64,
}
impl Slot {
    fn read(&self) -> Option<(u64, u64, Entry)> {
        // Acquire observes the previous publication; the fence below keeps payload
        // loads before sequence validation. Payload remains atomic even on a miss.
        let before = self.sequence.load(Ordering::Acquire);
        if before & 1 != 0 {
            return None;
        }
        let player = self.player.load(Ordering::Relaxed);
        let opponent = self.opponent.load(Ordering::Relaxed);
        let data = self.data.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        if self.sequence.load(Ordering::Relaxed) != before {
            return None;
        }
        Some((player, opponent, Entry::unpack(data)?))
    }
    fn write(&self, board: Board, value: Entry) {
        let seq = self.sequence.load(Ordering::Relaxed);
        if seq & 1 != 0
            || self
                .sequence
                .compare_exchange(
                    seq,
                    seq.wrapping_add(1),
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_err()
        {
            return;
        }
        // Publish the odd marker before payload modifications, then release the
        // completed snapshot. Readers never use an unvalidated payload.
        fence(Ordering::Release);
        self.player.store(board.player_bits(), Ordering::Relaxed);
        self.opponent
            .store(board.opponent_bits(), Ordering::Relaxed);
        self.data.store(value.pack(), Ordering::Relaxed);
        self.sequence.store(seq.wrapping_add(2), Ordering::Release);
    }
}
#[repr(align(64))]
#[derive(Default)]
struct Cluster {
    slots: [Slot; 2],
}
pub(crate) struct Table {
    clusters: Box<[Cluster]>,
}
impl Table {
    pub(crate) fn new(mib: usize) -> Result<Self, &'static str> {
        let bytes = mib.checked_mul(1024 * 1024).ok_or("TT size overflow")?;
        if bytes < 64 {
            return Err("TT must be at least 1 MiB");
        }
        let count = 1usize << (usize::BITS - 1 - (bytes / 64).leading_zeros());
        let mut clusters = Vec::new();
        clusters
            .try_reserve_exact(count)
            .map_err(|_| "TT allocation failed")?;
        clusters.resize_with(count, Cluster::default);
        Ok(Self {
            clusters: clusters.into_boxed_slice(),
        })
    }
    fn cluster(&self, board: Board) -> &Cluster {
        let mut key = board
            .player_bits()
            .wrapping_add(board.opponent_bits().rotate_left(23));
        key = (key ^ (key >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        key = (key ^ (key >> 27)).wrapping_mul(0x94d049bb133111eb);
        &self.clusters[(key ^ (key >> 31)) as usize & (self.clusters.len() - 1)]
    }
    pub(crate) fn get(&self, board: Board) -> Option<Entry> {
        self.cluster(board)
            .slots
            .iter()
            .filter_map(Slot::read)
            .find_map(|(p, o, e)| {
                (p == board.player_bits() && o == board.opponent_bits()).then_some(e)
            })
    }
    pub(crate) fn store(&self, board: Board, mut value: Entry) {
        let slots = &self.cluster(board).slots;
        let snapshots = [slots[0].read(), slots[1].read()];
        let same = snapshots.iter().position(|s| {
            s.is_some_and(|(p, o, _)| p == board.player_bits() && o == board.opponent_bits())
        });
        let target = if let Some(i) = same {
            let old = snapshots[i].unwrap().2;
            if value.best >= 64 {
                value.best = old.best;
                value.second = old.second;
            } else if value.best != old.best {
                value.second = old.best;
            } else {
                value.second = old.second;
            }
            if old.depth == value.depth
                && old.quality == value.quality
                && old.exact_mode == value.exact_mode
            {
                let lower = old.lower.max(value.lower);
                let upper = old.upper.min(value.upper);
                if lower <= upper {
                    value.lower = lower;
                    value.upper = upper;
                }
            }
            i
        } else {
            snapshots
                .iter()
                .position(Option::is_none)
                .unwrap_or_else(|| {
                    let rank = |i: usize| {
                        let e = snapshots[i].unwrap().2;
                        (e.generation == value.generation, e.depth)
                    };
                    if rank(0) <= rank(1) { 0 } else { 1 }
                })
        };
        slots[target].write(board, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_and_bound_merge() {
        assert_eq!(std::mem::size_of::<Cluster>(), 64);
        let table = Table::new(1).unwrap();
        let board = Board::new();
        let entry = Entry {
            lower: -10,
            upper: 64,
            depth: 5,
            quality: 0,
            best: 19,
            second: 64,
            generation: 1,
            exact_mode: false,
        };
        table.store(board, entry);
        table.store(
            board,
            Entry {
                lower: -64,
                upper: 10,
                ..entry
            },
        );
        let value = table.get(board).unwrap();
        assert_eq!((value.lower, value.upper), (-10, 10));
        assert!(table.get(board.board_after_pass()).is_none());
    }
}

#[cfg(test)]
mod concurrent_tests {
    use super::*;
    use std::{sync::Arc, thread};
    #[test]
    fn concurrent_snapshots_match_their_board() {
        let table = Arc::new(Table {
            clusters: vec![Cluster::default()].into_boxed_slice(),
        });
        let mut threads = Vec::new();
        for id in 0..4 {
            let table = table.clone();
            threads.push(thread::spawn(move || {
                for n in 0..10000 {
                    let board = Board::from_bits(1u64 << id, 1u64 << (id + 8)).unwrap();
                    table.store(
                        board,
                        Entry {
                            lower: id as i8,
                            upper: id as i8,
                            depth: 5,
                            quality: 0,
                            best: 19,
                            second: 64,
                            generation: n as u8,
                            exact_mode: false,
                        },
                    );
                    if let Some(entry) = table.get(board) {
                        assert_eq!(entry.lower, id as i8);
                        assert_eq!(entry.upper, id as i8);
                    }
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
    }
}
