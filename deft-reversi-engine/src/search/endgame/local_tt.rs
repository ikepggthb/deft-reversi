//! スレッドローカルな終盤置換表（TT）です。
//!
//! 終盤の残り 13 マス以下では探索速度が非常に速く、共有置換表への原子操作や競合コストが目立ちます。
//! そのため各スレッド上の `Context` がこの軽量キャッシュを専有し、
//! 参照（`probe`）時はスロットのポインタではなく不変スナップショット（[`EndProbe`]）を返します。
//!
//! ## 出典
//!
//! 空きマス数ごとにスレッド専有の表を持つ構成とハッシュ関数の乗数は、[Egaroucid](https://github.com/Nyanyan/Egaroucid)（山名琢翔、GPL-3.0-or-later）の `endsearch_nws.hpp` に基づいています。
use super::{LOCAL_END_MAX, TAIL_EMPTIES};
use crate::Board;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
/// 空きマス数レイヤあたりのスロット数（2 のべき乗）です。
const LOCAL_SLOTS: usize = 1024;
/// 保持する空きマス数レイヤ数（`TAIL_EMPTIES..=LOCAL_END_MAX`）です。
const LAYERS: usize = (LOCAL_END_MAX - TAIL_EMPTIES + 1) as usize;

/// 空きマス数ごとのローカル置換表を保持する、スレッド専有のキャッシュです。
#[derive(Default)]
pub(in crate::search) struct EndgameCache {
    layers: [Vec<EndEntry>; LAYERS],
}
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
thread_local! {
    /// スレッド上のどのコンテキストも保持していない間（ヘルパージョブ間や、マスターが合流待ちで
    /// 子孫ジョブを実行している間）に一時退避されるキャッシュと、その生成エポック番号です。
    static THREAD_CACHE: RefCell<(u64, EndgameCache)> = RefCell::new((0, EndgameCache::default()));
}
/// モデル・MPC・共有置換表の 1 つの状態に対してプロセス内で一意なエポック（世代）番号を発行します。
/// 新しいエポックが発行されると、古いエポック下で作成されたキャッシュは自動的に破棄されます。
pub(in crate::search) fn new_cache_epoch() -> u64 {
    NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)
}
impl EndgameCache {
    /// キャッシュ内容をすべてクリアします。
    pub(in crate::search) fn clear(&mut self) {
        *self = Self::default();
    }
    /// このスレッドに退避されているキャッシュが `epoch` 一致であれば取り出し、不一致なら空を返します。
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
    /// `epoch` と紐づけてキャッシュをスレッドローカルスロットへ退避します。
    pub(in crate::search) fn put_thread(self, epoch: u64) {
        THREAD_CACHE.with(|slot| *slot.borrow_mut() = (epoch, self));
    }
    /// 指定空きマス数 `empties` のレイヤが未確保なら初期化します。
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
    /// 置換表を引いてスナップショット（[`EndProbe`]）を返します。
    #[inline]
    pub(super) fn probe(&self, board: Board, quality: u8) -> EndProbe {
        let empties = board.empty_count();
        debug_assert!((TAIL_EMPTIES..=LOCAL_END_MAX).contains(&empties));
        // 手番石と相手石に異なる奇数を掛けてから排他的論理和を取ります。
        // 乗算前に XOR すると、32 マス離れた位置での色入れ替え同士が相殺して系統的な衝突を起こすためです。
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
    /// 探索結果の上下界と最善手をローカル置換表へ格納します。
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

// 6 バイトに収まる不変スナップショットです（盤面コピー・ポインタ・評価状態を持ちません）。
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
        // 取り出すとスロットは空になるため、入れ子のジョブは専用の新しいキャッシュを得ます。
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
