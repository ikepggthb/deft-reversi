use super::board::board::*;
use super::board::constant::*;
use std::mem;
use std::sync::atomic::{fence, AtomicU64, AtomicU8, Ordering};

// 用語:
// - TT: transposition table。探索済み局面を一時保存する表。
// - entry: 1 局面分の保存領域。
// - cluster: 近い hash の entry をまとめた単位。この実装では 64 byte に 2 entry。
// - 保存先位置(slot): probe で選び、store で同じ位置に書く場所。
// - 保存値(value): 評価値の上下限、深さ、候補手など。
// - hit / miss: hit は同じ局面が見つかった状態。miss は見つからなかった状態。
// - generation: entry の世代。古い entry を置換しやすくするための番号。

/// 各 TT エントリに保存する候補手の数。
pub const N_TT_MOVES: usize = 2;

/// デフォルトの transposition table サイズ。単位は MiB。
pub const DEFAULT_TT_MB_SIZE: usize = 256;

const CLUSTER_SIZE: usize = 2;
const CLUSTER_BYTES: usize = 64;
const MIB: usize = 1024 * 1024;
const OCCUPIED_FLAG: u8 = 1;
const AGE_WEIGHT: i32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TTValueFields {
    lower: i8,
    upper: i8,
    lv: u8,
    selectivity_lv: u8,
    move0: u8,
    move1: u8,
    generation: u8,
    flags: u8,
}

// 探索結果本体を atomic word 1 個に収める。TTEntry では AtomicU64 として保存し、
// 評価値の上下限、深さ、世代、候補手をまとめて読み書きできるようにする。
const _: () = assert!(mem::size_of::<TTValueFields>() == 8);

/// transposition table のエントリに保存する探索結果本体。
///
/// board key 自体はここには保存しない。`TTEntry` 側で完全な
/// `Board { player, opponent }` を別に保存し、見つかったときに毎回比較するため、
/// hash collision で別局面を誤って一致扱いすることはない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TTValue {
    fields: TTValueFields,
}

/// table 内のエントリ保存先位置。
///
/// 探索側は [`TranspositionTable::probe`] または
/// [`TranspositionTable::probe_with_key`] が返した保存先位置を保持し、
/// [`TranspositionTable::store`] に渡す。これにより store 時の cluster 再探索を避ける。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TTSlot {
    cluster_index: usize,
    entry_index: usize,
}

/// transposition table を probe した結果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TTProbe {
    /// 完全に一致する board が見つかった。
    Hit { value: TTValue, slot: TTSlot },
    /// board は見つからなかった。`slot` は推奨される保存先位置。
    Miss { slot: TTSlot },
}

#[repr(C)]
struct TTEntry {
    player: AtomicU64,
    opponent: AtomicU64,
    packed_value: AtomicU64,
    seq: AtomicU64,
}

#[repr(C, align(64))]
struct TTCluster {
    entries: [TTEntry; CLUSTER_SIZE],
}

/// 探索中に頻繁に実行される処理向けに最適化した共有 transposition table。
///
/// table は 64 byte cluster の配列で、1 cluster に 2 entry を持つ。
/// 読み取りは seqlock 風の snapshot で行い、mutex は取得しない。
/// 書き込みは CAS で 1 entry を確保して新しい探索結果を書き込む。
/// TT は一時保存領域なので、競合時に書き込みが捨てられても探索の正しさは壊れない。
pub struct TranspositionTable {
    clusters: Box<[TTCluster]>,
    cluster_mask: usize,
    generation: AtomicU8,
}

impl Default for TranspositionTable {
    fn default() -> Self {
        Self::with_mb_size(DEFAULT_TT_MB_SIZE)
    }
}

impl TranspositionTable {
    /// [`DEFAULT_TT_MB_SIZE`] の table を作る。
    pub fn new() -> Self {
        Self::default()
    }

    /// `mb_size` MiB を要求サイズとして table を作る。
    ///
    /// index 計算を `key & mask` にするため、実際の cluster 数は 2 の累乗に切り下げる。
    /// 実際に確保されたサイズは [`Self::actual_byte_size`] または
    /// [`Self::actual_mb_size`] で確認できる。
    pub fn with_mb_size(mb_size: usize) -> Self {
        let requested_clusters = ((mb_size.saturating_mul(MIB)) / CLUSTER_BYTES).max(1);
        Self::with_cluster_count(Self::floor_power_of_two(requested_clusters))
    }

    /// TT cluster 用に実際に確保された byte 数を返す。
    pub fn actual_byte_size(&self) -> usize {
        self.clusters.len() * mem::size_of::<TTCluster>()
    }

    /// TT cluster 用に実際に確保されたサイズを MiB 単位で返す。
    pub fn actual_mb_size(&self) -> usize {
        self.actual_byte_size() / MIB
    }

    /// すべての entry を消去し、generation を `1` に戻す。
    ///
    /// `&mut self` を要求するため、worker thread が共有参照を持ったまま clear できない。
    /// 通常の探索中に entry を古く扱いたい場合は [`Self::advance_generation`] を使う。
    pub fn clear(&mut self) {
        // SAFETY: &mut self により並行 reader / writer が存在しないことが保証される。
        // AtomicU64 と TTCluster は全 byte 0 の bit pattern が有効。
        unsafe {
            std::ptr::write_bytes(
                self.clusters.as_mut_ptr() as *mut u8,
                0,
                self.clusters.len() * mem::size_of::<TTCluster>(),
            );
        }
        self.generation.store(1, Ordering::Relaxed);
    }

    /// 論理 generation を進め、新しい値を返す。
    ///
    /// 古い generation の entry は置換候補として優先される。
    /// `u8` generation が 0 に wrap した場合、全 entry を消去して generation を `1` に戻す。
    pub fn advance_generation(&self) -> u8 {
        let next = self.generation.load(Ordering::Relaxed).wrapping_add(1);
        if next == 0 {
            self.clear_entries_relaxed();
            self.generation.store(1, Ordering::Relaxed);
            1
        } else {
            self.generation.store(next, Ordering::Relaxed);
            next
        }
    }

    /// 現在の論理 generation を返す。
    pub fn generation(&self) -> u8 {
        self.generation.load(Ordering::Relaxed)
    }

    /// cluster index に使う deterministic hash key を返す。
    ///
    /// これは hit 判定の材料ではない。hit 時は完全な `player` / `opponent`
    /// bitboard を比較する。
    #[inline(always)]
    pub fn key(board: &Board) -> u64 {
        Self::mix_board(board.player, board.opponent)
    }

    /// `board` の cluster index を返す。
    ///
    /// 主に test や診断用。
    #[inline(always)]
    pub fn hash_board(&self, board: &Board) -> usize {
        self.cluster_index(Self::key(board))
    }

    /// 対応 architecture で `board` の cluster を prefetch する。
    #[inline(always)]
    pub fn prefetch(&self, board: &Board) {
        self.prefetch_key(Self::key(board));
    }

    /// 計算済み key に対応する cluster を prefetch する。
    ///
    /// `x86_64` では L1 hint 付きの `_mm_prefetch` を発行する。
    /// それ以外の architecture では no-op。
    #[inline(always)]
    pub fn prefetch_key(&self, key: u64) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let cluster_index = self.cluster_index(key);
            // SAFETY: cluster_index は key を確保済み cluster 範囲に mask するため、
            // `add(cluster_index)` は slice 内に留まる。
            let cluster_ptr = self.clusters.as_ptr().add(cluster_index) as *const i8;
            std::arch::x86_64::_mm_prefetch(cluster_ptr, std::arch::x86_64::_MM_HINT_T0);
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = key;
        }
    }

    /// `board` を table から probe する。
    ///
    /// 速度を優先する場合、[`Self::key`] を 1 回だけ計算し、
    /// prefetch や store でも使うなら [`Self::probe_with_key`] を使う。
    #[inline(always)]
    pub fn probe(&self, board: &Board) -> TTProbe {
        self.probe_with_key(board, Self::key(board))
    }

    /// 計算済み key を使って table を probe する。
    ///
    /// hit は保存済みの完全な board が `board` と一致したことを意味する。
    /// miss の場合でも、probe した cluster から選ばれた保存先位置を返す。
    #[inline(always)]
    pub fn probe_with_key(&self, board: &Board, key: u64) -> TTProbe {
        const PROBE_RETRIES: usize = 3;

        let cluster_index = self.cluster_index(key);
        let generation = self.generation();
        let mut best_replacement_slot = TTSlot {
            cluster_index,
            entry_index: 0,
        };
        let mut found_replacement_candidate = false;

        for _ in 0..PROBE_RETRIES {
            // SAFETY: cluster_index は `cluster_index` が作り、table 長の範囲に mask される。
            let cluster = unsafe { self.clusters.get_unchecked(cluster_index) };
            let mut saw_writer = false;
            let mut best_replacement_score = i32::MAX;

            for entry_index in 0..CLUSTER_SIZE {
                // SAFETY: entry_index は `0..CLUSTER_SIZE` から来る。
                let entry = unsafe { cluster.entries.get_unchecked(entry_index) };
                let Some((player, opponent, stored_value, _)) = entry.try_load_snapshot() else {
                    saw_writer = true;
                    continue;
                };

                let slot = TTSlot {
                    cluster_index,
                    entry_index,
                };

                if stored_value.is_occupied()
                    && player == board.player
                    && opponent == board.opponent
                {
                    return TTProbe::Hit {
                        value: stored_value,
                        slot,
                    };
                }

                if !stored_value.is_occupied() {
                    return TTProbe::Miss { slot };
                }

                // score が低いほど置換しやすい。
                let replacement_score = stored_value.replacement_score(generation);
                if replacement_score < best_replacement_score {
                    best_replacement_score = replacement_score;
                    best_replacement_slot = slot;
                    found_replacement_candidate = true;
                }
            }

            if !saw_writer {
                return TTProbe::Miss {
                    slot: best_replacement_slot,
                };
            }

            std::hint::spin_loop();
        }

        TTProbe::Miss {
            slot: if found_replacement_candidate {
                best_replacement_slot
            } else {
                TTSlot {
                    cluster_index,
                    entry_index: 0,
                }
            },
        }
    }

    /// `board` に対応する保存値だけを返す。
    ///
    /// 保存先位置を再利用しない呼び出し側には便利。miss 後に store する頻繁な処理では
    /// [`Self::probe`] を使う。
    #[inline(always)]
    pub fn get(&self, board: &Board) -> Option<TTValue> {
        self.probe(board).value()
    }

    /// 事前に probe した保存先位置に探索結果を保存する。
    ///
    /// `lower` / `upper` は `i8`、`lv` / `selectivity_lv` は `u8` として保存される。
    /// debug build では範囲を検査する。release build では探索側が範囲内に収める前提。
    #[inline(always)]
    pub fn store(
        &self,
        slot: TTSlot,
        board: &Board,
        lower: i32,
        upper: i32,
        lv: i32,
        selectivity_lv: i32,
        best_move: u8,
    ) {
        validate_store_args(lower, upper, lv, selectivity_lv);

        let new_value = TTValueFields::new(
            lower as i8,
            upper as i8,
            lv as u8,
            selectivity_lv as u8,
            best_move,
            self.generation(),
        );
        let entry = self.entry(slot);
        entry.save(board, new_value);
    }

    /// probe と store を 1 回で行う互換 helper。
    ///
    /// 探索中に頻繁に実行される処理では、選ばれた保存先位置を再利用するために
    /// [`Self::probe`] / [`Self::probe_with_key`] と [`Self::store`] を分けて使う。
    #[inline(always)]
    pub fn add(
        &self,
        board: &Board,
        lower: i32,
        upper: i32,
        lv: i32,
        selectivity_lv: i32,
        best_move: u8,
    ) {
        let slot = self.probe(board).slot();
        self.store(slot, board, lower, upper, lv, selectivity_lv, best_move);
    }

    /// 使用中の entry 数を数える。
    ///
    /// table 全体を走査するため、探索中に頻繁に実行される処理ではなく test や診断向け。
    pub fn count_used_tt(&self) -> usize {
        self.clusters
            .iter()
            .map(|cluster| {
                cluster
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry
                            .try_load_snapshot()
                            .is_some_and(|(_, _, stored_value, _)| stored_value.is_occupied())
                    })
                    .count()
            })
            .sum()
    }

    fn with_cluster_count(cluster_count: usize) -> Self {
        debug_assert!(cluster_count.is_power_of_two());
        let clusters = (0..cluster_count)
            .map(|_| TTCluster::default())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            clusters,
            cluster_mask: cluster_count - 1,
            generation: AtomicU8::new(1),
        }
    }

    #[inline(always)]
    fn entry(&self, slot: TTSlot) -> &TTEntry {
        debug_assert!(slot.cluster_index < self.clusters.len());
        debug_assert!(slot.entry_index < CLUSTER_SIZE);
        // SAFETY: TTSlot はこの table の probe path で作られる。
        // debug assertion は test や新しい呼び出し側の誤用を検出するためのもの。
        unsafe {
            self.clusters
                .get_unchecked(slot.cluster_index)
                .entries
                .get_unchecked(slot.entry_index)
        }
    }

    #[inline(always)]
    fn cluster_index(&self, key: u64) -> usize {
        key as usize & self.cluster_mask
    }

    #[inline(always)]
    fn mix_board(player: u64, opponent: u64) -> u64 {
        // hit 時に完全な board equality を確認するため、hash は cluster を選ぶだけでよい。
        // そのため小さな deterministic mixer で十分。
        let mut x = player.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        x ^= opponent.rotate_left(32).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        x ^ (x >> 33)
    }

    fn floor_power_of_two(n: usize) -> usize {
        debug_assert!(n > 0);
        1usize << (usize::BITS - 1 - n.leading_zeros())
    }

    fn clear_entries_relaxed(&self) {
        // generation wrap 時だけ使う。sweep 中の通常 reader は miss か古い値を観測しうるが、
        // TT は一時保存領域なのでどちらも許容できる。
        for cluster in self.clusters.iter() {
            for entry in cluster.entries.iter() {
                entry.clear_relaxed();
            }
        }
    }
}

impl TTProbe {
    /// この probe 結果に対応する保存先位置を返す。
    ///
    /// hit では一致した entry、miss では推奨される置換 entry を指す。
    #[inline(always)]
    pub fn slot(&self) -> TTSlot {
        match self {
            TTProbe::Hit { slot, .. } | TTProbe::Miss { slot } => *slot,
        }
    }

    /// hit なら保存済み探索結果、miss なら `None` を返す。
    #[inline(always)]
    pub fn value(&self) -> Option<TTValue> {
        match self {
            TTProbe::Hit {
                value: hit_value, ..
            } => Some(*hit_value),
            TTProbe::Miss { .. } => None,
        }
    }

    /// probe が完全一致 board を見つけた場合に `true` を返す。
    #[inline(always)]
    pub fn is_hit(&self) -> bool {
        matches!(self, TTProbe::Hit { .. })
    }
}

impl TTValue {
    /// この局面に保存された lower bound。
    #[inline(always)]
    pub fn lower(self) -> i8 {
        self.fields.lower
    }

    /// この局面に保存された upper bound。
    #[inline(always)]
    pub fn upper(self) -> i8 {
        self.fields.upper
    }

    /// この entry に紐づく探索 depth / level。
    #[inline(always)]
    pub fn lv(self) -> u8 {
        self.fields.lv
    }

    /// この entry に紐づく selectivity level。
    #[inline(always)]
    pub fn selectivity_lv(self) -> u8 {
        self.fields.selectivity_lv
    }

    /// 保存された候補手を優先順で返す。
    ///
    /// 手がない entry は `NO_COORD` を使う。
    #[inline(always)]
    pub fn moves(self) -> [u8; N_TT_MOVES] {
        [self.fields.move0, self.fields.move1]
    }

    /// この entry が書かれた generation。
    #[inline(always)]
    pub fn generation(self) -> u8 {
        self.fields.generation
    }

    #[inline(always)]
    fn is_occupied(self) -> bool {
        self.fields.flags & OCCUPIED_FLAG != 0
    }

    #[inline(always)]
    fn quality(self) -> u16 {
        ((self.fields.lv as u16) << 8) | self.fields.selectivity_lv as u16
    }

    #[inline(always)]
    fn relative_age(self, generation: u8) -> i32 {
        generation.wrapping_sub(self.fields.generation) as i32
    }

    #[inline(always)]
    fn replacement_score(self, generation: u8) -> i32 {
        let age = self.relative_age(generation);
        if age == 0 {
            self.quality() as i32
        } else {
            i32::MIN / 2 + self.quality() as i32 - age * AGE_WEIGHT
        }
    }
}

impl TTValueFields {
    #[inline(always)]
    fn new(
        lower: i8,
        upper: i8,
        lv: u8,
        selectivity_lv: u8,
        best_move: u8,
        generation: u8,
    ) -> Self {
        Self {
            lower,
            upper,
            lv,
            selectivity_lv,
            move0: best_move,
            move1: NO_COORD,
            generation,
            flags: OCCUPIED_FLAG,
        }
    }

    #[inline(always)]
    fn pack(self) -> u64 {
        // SAFETY: TTValueFields は #[repr(C)] で、ちょうど 8 byte であることを assert 済み。
        // 整数 field だけを持つため、すべての bit pattern が有効。
        unsafe { mem::transmute(self) }
    }

    #[inline(always)]
    fn unpack(packed: u64) -> Self {
        // SAFETY: TTValueFields は整数 field だけを持つため、
        // すべての u64 bit pattern が有効な保存値になる。
        unsafe { mem::transmute(packed) }
    }

    #[inline(always)]
    fn is_occupied(self) -> bool {
        self.flags & OCCUPIED_FLAG != 0
    }

    #[inline(always)]
    fn quality(self) -> u16 {
        ((self.lv as u16) << 8) | self.selectivity_lv as u16
    }

    #[inline(always)]
    fn merge_same_level(self, new_value: Self) -> Self {
        // 同じ quality の entry は alpha-beta window を狭められる。
        // bound が矛盾した場合は、無効な区間を作らず新しい結果を優先する。
        let merged_lower = self.lower.max(new_value.lower);
        let merged_upper = self.upper.min(new_value.upper);
        let mut merged = new_value;
        if merged_lower <= merged_upper {
            merged.lower = merged_lower;
            merged.upper = merged_upper;
        }
        merged.move0 = self.move0;
        merged.move1 = self.move1;
        merged.add_best_move(new_value.move0);
        merged
    }

    #[inline(always)]
    fn replace_same_board(self, mut new_value: Self) -> Self {
        // 同じ board でまだ有用な場合、前 entry の move ordering 情報を残す。
        if new_value.move0 == NO_COORD {
            new_value.move0 = self.move0;
            new_value.move1 = self.move1;
        } else if new_value.move0 != self.move0 {
            new_value.move1 = self.move0;
        }
        new_value
    }

    #[inline(always)]
    fn add_best_move(&mut self, best_move: u8) {
        if best_move != NO_COORD && self.move0 != best_move {
            self.move1 = self.move0;
            self.move0 = best_move;
        }
    }
}

impl Default for TTEntry {
    fn default() -> Self {
        Self {
            player: AtomicU64::new(0),
            opponent: AtomicU64::new(0),
            packed_value: AtomicU64::new(0),
            seq: AtomicU64::new(0),
        }
    }
}

impl TTEntry {
    const SAVE_RETRIES: usize = 4;

    #[inline(always)]
    fn try_load_snapshot(&self) -> Option<(u64, u64, TTValue, u64)> {
        // 偶数 seq は安定状態または idle、奇数 seq は writer が entry を保持中。
        let start_seq = self.seq.load(Ordering::Acquire);
        if start_seq & 1 != 0 {
            return None;
        }

        let player = self.player.load(Ordering::Relaxed);
        let opponent = self.opponent.load(Ordering::Relaxed);
        let packed_value = self.packed_value.load(Ordering::Relaxed);

        // 保存値の load が 2 回目の seq load より後ろへ reorder されるのを防ぐ。
        fence(Ordering::Acquire);
        let end_seq = self.seq.load(Ordering::Relaxed);
        if start_seq != end_seq {
            return None;
        }

        Some((
            player,
            opponent,
            TTValue {
                fields: TTValueFields::unpack(packed_value),
            },
            start_seq,
        ))
    }

    fn save(&self, board: &Board, new_value: TTValueFields) {
        for _ in 0..Self::SAVE_RETRIES {
            let Some((stored_player, stored_opponent, stored_value, observed_seq)) =
                self.try_load_snapshot()
            else {
                std::hint::spin_loop();
                continue;
            };

            let stored_fields = stored_value.fields;
            let matches_stored_board = stored_value.is_occupied()
                && stored_player == board.player
                && stored_opponent == board.opponent;

            let value_to_write = if matches_stored_board {
                if stored_fields.quality() == new_value.quality() {
                    stored_fields.merge_same_level(new_value)
                } else if stored_fields.generation != new_value.generation
                    || new_value.quality() >= stored_fields.quality()
                {
                    stored_fields.replace_same_board(new_value)
                } else {
                    return;
                }
            } else if !stored_fields.is_occupied()
                || stored_fields.generation != new_value.generation
                || new_value.quality() >= stored_fields.quality()
            {
                new_value
            } else {
                return;
            };

            // 別 writer が CAS に勝った場合は数回 retry する。
            // TT は一時保存領域なので、最終的に write を捨ててもよい。
            if self.seqlock_write(observed_seq, board, value_to_write) {
                return;
            }

            std::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn seqlock_write(
        &self,
        expected_seq: u64,
        board: &Board,
        value_to_write: TTValueFields,
    ) -> bool {
        debug_assert_eq!(expected_seq & 1, 0);
        if expected_seq & 1 != 0 {
            return false;
        }

        // 偶数 seq を次の奇数値に変更して、この entry を確保する。
        if self
            .seq
            .compare_exchange(
                expected_seq,
                expected_seq | 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return false;
        }

        // seq を再び偶数にする前に保存値を書き込む。
        fence(Ordering::Release);
        self.player.store(board.player, Ordering::Relaxed);
        self.opponent.store(board.opponent, Ordering::Relaxed);
        self.packed_value
            .store(value_to_write.pack(), Ordering::Relaxed);
        self.seq
            .store(expected_seq.wrapping_add(2), Ordering::Release);
        true
    }

    fn clear_relaxed(&self) {
        self.player.store(0, Ordering::Relaxed);
        self.opponent.store(0, Ordering::Relaxed);
        self.packed_value.store(0, Ordering::Relaxed);
        self.seq.store(0, Ordering::Relaxed);
    }
}

impl Default for TTCluster {
    fn default() -> Self {
        Self {
            entries: [TTEntry::default(), TTEntry::default()],
        }
    }
}

#[inline(always)]
fn validate_store_args(lower: i32, upper: i32, lv: i32, selectivity_lv: i32) {
    const I8_MAX: i32 = i8::MAX as i32;
    const I8_MIN: i32 = i8::MIN as i32;
    const U8_MAX: i32 = u8::MAX as i32;
    debug_assert!(
        I8_MIN <= lower && lower <= upper && upper <= I8_MAX,
        "in function t_table::store(), lower: {lower}, upper: {upper}, lv: {lv}"
    );
    debug_assert!(
        (0..=U8_MAX).contains(&lv) && (0..=U8_MAX).contains(&selectivity_lv),
        "in function t_table::store(), lv: {lv}, selectivity lv: {selectivity_lv}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    fn board(player: u64, opponent: u64) -> Board {
        Board { player, opponent }
    }

    fn tiny_tt() -> TranspositionTable {
        TranspositionTable::with_cluster_count(16)
    }

    fn colliding_board(tt: &TranspositionTable, base: &Board) -> Board {
        let base_index = tt.hash_board(base);
        for candidate_seed in 1..100_000 {
            let candidate = board(candidate_seed, !candidate_seed);
            if candidate.player != base.player
                && candidate.opponent != base.opponent
                && tt.hash_board(&candidate) == base_index
            {
                return candidate;
            }
        }
        panic!("failed to find colliding board for test");
    }

    #[test]
    fn table_layout_is_cache_line_clustered() {
        assert_eq!(mem::size_of::<TTValueFields>(), 8);
        assert_eq!(mem::size_of::<TTEntry>(), 32);
        assert_eq!(mem::size_of::<TTCluster>(), 64);
        assert_eq!(mem::align_of::<TTCluster>(), 64);
    }

    #[test]
    fn mb_size_is_rounded_down_to_power_of_two_clusters() {
        let tt = TranspositionTable::with_mb_size(1);
        assert_eq!(tt.actual_mb_size(), 1);
        assert!(tt.clusters.len().is_power_of_two());

        let tiny = TranspositionTable::with_mb_size(0);
        assert_eq!(tiny.clusters.len(), 1);
    }

    #[test]
    fn add_and_get_returns_stored_entry() {
        let tt = tiny_tt();
        let board = board(0x0000_0008_1000_0000, 0x0000_0010_0800_0000);

        tt.add(&board, -12, 34, 7, 2, 19);
        let stored_value = tt.get(&board).unwrap();

        assert_eq!(stored_value.lower(), -12);
        assert_eq!(stored_value.upper(), 34);
        assert_eq!(stored_value.lv(), 7);
        assert_eq!(stored_value.selectivity_lv(), 2);
        assert_eq!(stored_value.moves(), [19, NO_COORD]);
        assert_eq!(tt.count_used_tt(), 1);
    }

    #[test]
    fn strict_board_match_prevents_false_hit_on_hash_collision() {
        let tt = tiny_tt();
        let first = board(0x01, 0x02);
        let second = colliding_board(&tt, &first);

        tt.add(&first, -1, 1, 5, 0, 10);

        assert!(tt.get(&first).is_some());
        assert!(tt.get(&second).is_none());
    }

    #[test]
    fn cluster_keeps_two_colliding_entries() {
        let tt = tiny_tt();
        let first = board(0x01, 0x02);
        let second = colliding_board(&tt, &first);

        tt.add(&first, -1, 1, 5, 0, 10);
        tt.add(&second, -2, 2, 5, 0, 20);

        assert!(tt.get(&first).is_some());
        assert!(tt.get(&second).is_some());
        assert_eq!(tt.count_used_tt(), 2);
    }

    #[test]
    fn same_level_update_merges_bounds_and_moves() {
        let tt = tiny_tt();
        let board = board(0x10, 0x20);

        tt.add(&board, -10, 20, 6, 1, 10);
        tt.add(&board, -5, 12, 6, 1, 30);
        let stored_value = tt.get(&board).unwrap();

        assert_eq!(stored_value.lower(), -5);
        assert_eq!(stored_value.upper(), 12);
        assert_eq!(stored_value.lv(), 6);
        assert_eq!(stored_value.selectivity_lv(), 1);
        assert_eq!(stored_value.moves(), [30, 10]);
    }

    #[test]
    fn inconsistent_same_level_bounds_fall_back_to_new_bounds() {
        let tt = tiny_tt();
        let board = board(0x10, 0x20);

        tt.add(&board, 1, 5, 6, 1, 10);
        tt.add(&board, -5, -1, 6, 1, 30);
        let stored_value = tt.get(&board).unwrap();

        assert_eq!(stored_value.lower(), -5);
        assert_eq!(stored_value.upper(), -1);
        assert_eq!(stored_value.moves(), [30, 10]);
    }

    #[test]
    fn higher_level_replaces_and_keeps_old_move_as_second() {
        let tt = tiny_tt();
        let board = board(0x01, 0x02);

        tt.add(&board, -1, 1, 5, 0, 10);
        tt.add(&board, -2, 2, 6, 1, 20);
        let stored_value = tt.get(&board).unwrap();

        assert_eq!(stored_value.lower(), -2);
        assert_eq!(stored_value.upper(), 2);
        assert_eq!(stored_value.lv(), 6);
        assert_eq!(stored_value.selectivity_lv(), 1);
        assert_eq!(stored_value.moves(), [20, 10]);
    }

    #[test]
    fn no_coord_update_preserves_existing_moves() {
        let tt = tiny_tt();
        let board = board(0x01, 0x02);

        tt.add(&board, -1, 1, 5, 0, 10);
        tt.add(&board, -2, 2, 6, 1, NO_COORD);
        let stored_value = tt.get(&board).unwrap();

        assert_eq!(stored_value.lower(), -2);
        assert_eq!(stored_value.upper(), 2);
        assert_eq!(stored_value.moves(), [10, NO_COORD]);
    }

    #[test]
    fn lower_quality_current_generation_entry_does_not_replace() {
        let tt = TranspositionTable::with_cluster_count(1);
        let strong0 = board(0x01, 0x02);
        let strong1 = board(0x03, 0x04);
        let weak = board(0x05, 0x06);

        tt.add(&strong0, -1, 1, 10, 0, 10);
        tt.add(&strong1, -2, 2, 9, 0, 20);
        tt.add(&weak, -3, 3, 1, 0, 30);

        assert!(tt.get(&strong0).is_some());
        assert!(tt.get(&strong1).is_some());
        assert!(tt.get(&weak).is_none());
    }

    #[test]
    fn old_generation_entry_is_replaced_first() {
        let tt = TranspositionTable::with_cluster_count(1);
        let old = board(0x01, 0x02);
        let current = board(0x03, 0x04);
        let new = board(0x05, 0x06);

        tt.add(&old, -1, 1, 10, 0, 10);
        tt.advance_generation();
        tt.add(&current, -2, 2, 1, 0, 20);
        tt.add(&new, -3, 3, 1, 0, 30);

        assert!(tt.get(&old).is_none());
        assert!(tt.get(&current).is_some());
        assert!(tt.get(&new).is_some());
    }

    #[test]
    fn probe_slot_can_be_reused_for_store() {
        let tt = tiny_tt();
        let board = board(0x11, 0x22);
        let probe = tt.probe(&board);

        assert!(!probe.is_hit());
        tt.store(probe.slot(), &board, -4, 4, 3, 2, 12);

        let probe = tt.probe(&board);
        assert!(probe.is_hit());
        assert_eq!(probe.value().unwrap().moves(), [12, NO_COORD]);
    }

    #[test]
    fn concurrent_probe_and_store_keep_entries_valid() {
        let tt = Arc::new(TranspositionTable::with_cluster_count(64));
        let mut handles = Vec::new();

        for thread_id in 0..8u64 {
            let tt = Arc::clone(&tt);
            handles.push(thread::spawn(move || {
                for position_index in 0..2_000u64 {
                    let player_bits = (thread_id << 48) ^ position_index.wrapping_mul(0x9e37_79b9);
                    let opponent_bits = !player_bits.rotate_left(7);
                    let board = board(player_bits, opponent_bits);
                    let probe = tt.probe(&board);
                    tt.store(
                        probe.slot(),
                        &board,
                        -64 + (position_index % 16) as i32,
                        64 - (position_index % 16) as i32,
                        (position_index % 32) as i32,
                        (thread_id % 8) as i32,
                        (position_index % 64) as u8,
                    );
                    if let Some(stored_value) = tt.get(&board) {
                        assert!(stored_value.lower() <= stored_value.upper());
                        assert!(stored_value.moves()[0] <= NO_COORD);
                    }
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert!(tt.count_used_tt() > 0);
    }

    #[test]
    #[ignore]
    fn bench_mixed_probe_store() {
        const N: usize = 1_000_000_0;
        let tt = TranspositionTable::with_mb_size(16);
        let start = Instant::now();

        for position_index in 0..N as u64 {
            let board = board(
                position_index.wrapping_mul(0x9e37_79b9),
                !position_index.rotate_left(13),
            );
            let probe = tt.probe(&board);
            tt.store(
                probe.slot(),
                &board,
                -10,
                10,
                (position_index & 31) as i32,
                0,
                (position_index & 63) as u8,
            );
        }

        let elapsed = start.elapsed();
        println!(
            "mixed probe/store: {:.2} ns/op",
            elapsed.as_nanos() as f64 / N as f64
        );
    }
}
