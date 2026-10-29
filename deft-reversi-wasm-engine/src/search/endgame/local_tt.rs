//! Per-thread endgame TT, held by the context running on that thread; probes
//! are immutable snapshots, never slot pointers.
use super::{LOCAL_END_MAX, TAIL_EMPTIES};
use crate::Board;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
const LOCAL_SLOTS: usize = 1024;
const LAYERS: usize = (LOCAL_END_MAX - TAIL_EMPTIES + 1) as usize;

#[derive(Default)]
pub(in crate::search) struct EndgameCache {
    layers: [Vec<EndEntry>; LAYERS],
}
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
thread_local! {
    /// The thread's cache while no context on this thread holds it: between
    /// helper jobs, and while a master joins its split (the jobs it runs
    /// meanwhile use it), with the epoch it was filled under.
    static THREAD_CACHE: RefCell<(u64, EndgameCache)> = RefCell::new((0, EndgameCache::default()));
}
/// A process-unique tag for one model/MPC/table state; a new one invalidates
/// every cache made under the old one.
pub(in crate::search) fn new_cache_epoch() -> u64 {
    NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)
}
impl EndgameCache {
    pub(in crate::search) fn clear(&mut self) {
        *self = Self::default();
    }
    /// This thread's cache if it was filled under `epoch`, else an empty one.
    pub(in crate::search) fn take_thread(epoch: u64) -> Self {
        THREAD_CACHE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let cache = std::mem::take(&mut slot.1);
            if slot.0 == epoch {
                cache
            } else {
                Self::default()
            }
        })
    }
    pub(in crate::search) fn put_thread(self, epoch: u64) {
        THREAD_CACHE.with(|slot| *slot.borrow_mut() = (epoch, self));
    }
    pub(super) fn prepare(&mut self, empties: u8) {
        let layer = &mut self.layers[usize::from(empties - TAIL_EMPTIES)];
        if layer.is_empty() {
            layer.resize(
                LOCAL_SLOTS,
                EndEntry {
                    quality: 255,
                    player: 0,
                    opponent: 0,
                    lower: -64,
                    upper: 64,
                    best: 64,
                },
            );
        }
    }
    #[inline]
    pub(super) fn probe(&self, board: Board, quality: u8) -> EndProbe {
        let empties = board.empty_count();
        debug_assert!((TAIL_EMPTIES..=LOCAL_END_MAX).contains(&empties));
        // Mix colors independently: xor-before-multiply cancels color swaps
        // separated by 32 squares, creating systematic aliases.
        let hash = board.player_bits().wrapping_mul(0x9dda1c54cfe6b6e9)
            ^ board.opponent_bits().wrapping_mul(0xa2e6c0300831e05a);
        let index = (hash >> 54) as u16;
        let mut probe = EndProbe {
            index,
            ..EndProbe::EMPTY
        };
        if let Some(entry) =
            self.layers[usize::from(empties - TAIL_EMPTIES)].get(usize::from(index))
            && entry.player == board.player_bits()
            && entry.opponent == board.opponent_bits()
            && entry.quality == quality
        {
            probe.lower = entry.lower;
            probe.upper = entry.upper;
            probe.best = entry.best;
            probe.matched = true;
        }
        probe
    }
    pub(super) fn store(
        &mut self,
        board: Board,
        quality: u8,
        probe: EndProbe,
        score: i32,
        beta: i32,
        best: u8,
    ) {
        let (lower, upper) = if score >= beta {
            (score as i8, probe.upper)
        } else {
            (probe.lower, score as i8)
        };
        self.layers[usize::from(board.empty_count() - TAIL_EMPTIES)][usize::from(probe.index)] =
            EndEntry {
                player: board.player_bits(),
                opponent: board.opponent_bits(),
                quality,
                lower,
                upper,
                best,
            };
    }
}

#[derive(Clone, Copy)]
struct EndEntry {
    quality: u8,
    player: u64,
    opponent: u64,
    lower: i8,
    upper: i8,
    best: u8,
}

// Six bytes: no board copy, pointer, or evaluation state in the candidate.
#[derive(Clone, Copy)]
pub(super) struct EndProbe {
    pub(super) index: u16,
    pub(super) lower: i8,
    pub(super) upper: i8,
    pub(super) best: u8,
    pub(super) matched: bool,
}
impl EndProbe {
    pub(super) const EMPTY: Self = Self {
        index: 0,
        lower: -64,
        upper: 64,
        best: 64,
        matched: false,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separated_color_swaps_do_not_systematically_alias() {
        let cache = EndgameCache::default();
        let a = Board::from_bits(0x5545555555555540, 0xaabaaaaaaaaaaa80).unwrap();
        let b = Board::from_bits(0x5555555555455540, 0xaaaaaaaaaabaaa80).unwrap();
        assert_eq!(a.empty_count(), b.empty_count());
        assert_ne!(cache.probe(a, 0).index, cache.probe(b, 0).index);
        assert_eq!(std::mem::size_of::<EndEntry>(), 24);
        assert_eq!(std::mem::size_of::<EndProbe>(), 6);
    }
    #[test]
    fn thread_cache_is_reused_only_under_its_epoch() {
        let board = Board::from_bits(0x5545555555555540, 0xaabaaaaaaaaaaa80).unwrap();
        let (epoch, stale) = (new_cache_epoch(), new_cache_epoch());
        let mut cache = EndgameCache::default();
        cache.prepare(board.empty_count());
        let probe = cache.probe(board, 0);
        cache.store(board, 0, probe, 10, 11, 64);
        cache.put_thread(epoch);
        let cache = EndgameCache::take_thread(epoch);
        assert!(cache.probe(board, 0).matched);
        // Taking it leaves the slot empty: a nested job gets its own cache.
        assert!(
            EndgameCache::take_thread(epoch)
                .layers
                .iter()
                .all(Vec::is_empty)
        );
        cache.put_thread(epoch);
        assert!(!EndgameCache::take_thread(stale).probe(board, 0).matched);
    }
}
