//! ロックフリー置換表（Transposition Table：一度調べた局面の探索結果を保存し、別の手順で同じ局面に到達したときに再利用する表）です。
//!
//! 複数スレッドから同時に読み書きしてもミューテックスで待たされないよう、
//! 各スロットを seqlock 風のシーケンス番号と全フィールドの原子操作で保護しています。
//! 読み取り中に他スレッドの書き込みと競合して整合性チェックに失敗した場合は、
//! 単にキャッシュミスとして扱い探索を続けます。
use crate::Board;
use std::sync::atomic::{AtomicU64, Ordering, fence};

/// 置換表の 1 エントリに格納する探索結果です（8 バイトにパックされます）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    /// 評価値の下界です（fail-high：スコアが探索窓の上限 β 以上であると分かったときなどに記録します）。
    pub lower: i8,
    /// 評価値の上界です（fail-low：スコアが探索窓の下限 α 以下であると分かったときなどに記録します）。
    pub upper: i8,
    /// 探索深さです（中盤では残余深さ、終盤完全読み時は空きマス数を格納します）。
    pub depth: u8,
    /// 選択率です（ProbCut の信頼度ステージや完全読みフラグをエンコードした品質値を格納します）。
    pub quality: u8,
    /// 最善手のマスインデックスです（0..63、無効時は 64 以上）。
    pub best: u8,
    /// 次善手のマスインデックスです（着手順序付けの補助に使い、無効時は 64 以上）。
    pub second: u8,
    /// 探索世代番号です（古い探索で書かれたエントリの置き換え優先度を判定するために使います）。
    pub generation: u8,
    /// 終盤完全読み（石差スコア）モードでの結果か、中盤の評価値モードでの結果かを示します。
    pub exact_mode: bool,
}
impl Entry {
    /// 8 バイトの `u64` にパックします。`b[6]` の最上位ビット（128）を有効エントリの目印として立てます。
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
    /// `u64` から `Entry` を復元します。有効フラグ（`b[6] & 128`）が立っていなければ `None` を返します。
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
/// 置換表内の 1 スロット（32 バイト）です。`sequence` の偶奇で書き込み中かどうかを表現します。
#[derive(Default)]
struct Slot {
    sequence: AtomicU64,
    player: AtomicU64,
    opponent: AtomicU64,
    data: AtomicU64,
}
impl Slot {
    /// 整合性の取れたスナップショットをロックフリーで読み取ります。書き込み競合中または空スロットなら `None` を返します。
    fn read(&self) -> Option<(u64, u64, Entry)> {
        // Acquire ロードで直前の書き込み完了を観測し、後続の Acquire フェンスで各フィールドの読み取りが
        // `sequence` 再検証より前に完了することを保証します。
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
    /// スロットが空いている（`sequence` が偶数）場合だけ CAS（Compare-And-Swap）で書き込み権を取り書き込みます。
    /// 他スレッドが書き込み中なら待たずにスキップします。
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
        // 奇数（書き込み中）を公開した後にデータを書き換え、最後に偶数へ戻して Release 公開します。
        fence(Ordering::Release);
        self.player.store(board.player_bits(), Ordering::Relaxed);
        self.opponent
            .store(board.opponent_bits(), Ordering::Relaxed);
        self.data.store(value.pack(), Ordering::Relaxed);
        self.sequence.store(seq.wrapping_add(2), Ordering::Release);
    }
}
/// 64 バイトのキャッシュラインに収まる 2-way 連想クラスタ（`Slot` × 2 = 64 バイト）です。
#[repr(align(64))]
#[derive(Default)]
struct Cluster {
    slots: [Slot; 2],
}
/// 2 のべき乗個のクラスタからなる共有置換表です。
pub(crate) struct Table {
    clusters: Box<[Cluster]>,
}
impl Table {
    /// 指定サイズ（MiB）以下の最大の 2 のべき乗クラスタ数で置換表を確保します。
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
    /// SplitMix64 風のハッシュミキシングで盤面から対応するクラスタを算出します。
    fn cluster(&self, board: Board) -> &Cluster {
        let mut key = board
            .player_bits()
            .wrapping_add(board.opponent_bits().rotate_left(23));
        key = (key ^ (key >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        key = (key ^ (key >> 27)).wrapping_mul(0x94d049bb133111eb);
        &self.clusters[(key ^ (key >> 31)) as usize & (self.clusters.len() - 1)]
    }
    /// 盤面に一致するエントリを検索して返します。
    pub(crate) fn get(&self, board: Board) -> Option<Entry> {
        self.cluster(board)
            .slots
            .iter()
            .filter_map(Slot::read)
            .find_map(|(p, o, e)| {
                (p == board.player_bits() && o == board.opponent_bits()).then_some(e)
            })
    }
    /// 置換表へエントリを格納します。
    /// 同じ盤面が既にあれば最善手・次善手の履歴や上下界をマージし、
    /// なければ空きスロット、または世代が古く探索が浅い側のスロットを上書きします。
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
