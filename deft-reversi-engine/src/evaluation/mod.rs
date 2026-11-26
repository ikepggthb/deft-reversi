//! 局面の静的評価（パターン評価関数・差分特徴更新 [`EvalState`]・補助ニューラルネット）を統合するモジュールです。
//!
//! 盤面を辺や角・斜めなどの「パターン（決まった位置のマス群）」に分解し、各マスを
//! 空き(0)・相手石(1)・自分石(2) の 3 値で表した 3 進数インデックスで重みテーブルを引きます。
//! さらに合法手数の差（モビリティ）と、必要に応じて小型ニューラルネットワークの出力を加算して、
//! 手番側から見た予想石差を算出します。

#[cfg(feature = "endgame-pattern-ordering")]
mod end_ordering;
mod pattern;
mod pen;
use crate::Board;
use pattern::definition::{
    FEATURE_COORD, PATTERN_TABLE_SIZES, SQUARE_TO_FEATURES, TOTAL_PATTERN_WEIGHTS,
};
#[allow(unused_imports)]
pub use pen::{PEN_DIM, PEN_GLOBALS, PEN_HIDDEN, PEN_INPUTS, PenPhase, global_inputs};
use std::{
    io::{self, Read, Write},
    sync::Arc,
};

/// 評価に使うパターンの種類数（11 種類）です。
pub const PATTERN_COUNT: usize = 11;
/// ゲーム進行度を表すフェーズ数（60：空きマス 60..=1 に対応し、空き 0 マスもフェーズ 59 に丸めます）です。
pub const PHASE_COUNT: usize = 60;
/// 1 フェーズあたりのパターン重み数（全パターンテーブルを連結した合計要素数）です。
pub const PATTERN_WEIGHT_COUNT: usize = TOTAL_PATTERN_WEIGHTS;
/// 1 フェーズあたりの着手可能数（モビリティ）重み数です（`64 + 自分の合法手数 - 相手の合法手数` で索引付けします）。
pub const MOBILITY_WEIGHT_COUNT: usize = 128;
/// 1 回の評価で参照するパターン特徴量の総数（11 パターン × 4 回転方向 = 44 個）です。
pub const FEATURE_COUNT: usize = PATTERN_COUNT * 4;

/// 1 回の評価で参照する線形特徴量（学習・チューニング用）です。
///
/// 補助ニューラルネットを持たないモデルでは、フェーズ `phase` の `bias + Σ patterns[i] + mobility[mobility]` を
/// 128 で割って四捨五入した値が [`PatternModel::evaluate`] の評価値になります（重みが 1/128 石単位で学習されているためです）。
/// 補助ニューラルネットを持つモデルでは、128 で割る前にその出力を加算します。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Features {
    /// 0..60 のフェーズ番号（`60 - 空きマス数`、満盤時は 59）です。
    pub phase: usize,
    /// このフェーズにおける `PATTERN_WEIGHT_COUNT` 個のパターン重み配列へのインデックス列です。
    pub patterns: [u32; FEATURE_COUNT],
    /// このフェーズにおける `MOBILITY_WEIGHT_COUNT` 個のモビリティ重み配列へのインデックスです。
    pub mobility: usize,
}
impl Features {
    /// 指定盤面から線形特徴量インデックスを抽出します。
    pub fn of(board: &Board) -> Self {
        // 学習トレーナーが全学習サンプルに対して高速に呼び出せるよう、
        // `EvalState::from_board` の `current` 側と同じ計算を相手視点なし・分岐なしで行います。
        let (player, opponent) = (board.player_bits(), board.opponent_bits());
        let mut patterns = [0; FEATURE_COUNT];
        let mut offset = 0;
        for (p, (pattern, size)) in FEATURE_COORD.iter().zip(PATTERN_TABLE_SIZES).enumerate() {
            for r in 0..4 {
                let mut index = 0u32;
                for &sq in &pattern.feature_coord[r][..pattern.n_squares as usize] {
                    let digit = 2 * (player >> sq & 1) + (opponent >> sq & 1);
                    index = index * 3 + digit as u32;
                }
                patterns[p * 4 + r] = offset as u32 + index;
            }
            offset += size;
        }
        Self {
            phase: phase_of(board),
            patterns,
            mobility: mobility_index(board.get_moves(), board),
        }
    }
}
/// 空きマス数から 0..60 のフェーズインデックスを返します。
fn phase_of(board: &Board) -> usize {
    usize::from(60u8.saturating_sub(board.empty_count())).min(59)
}
/// 合法手数差から 0..128 のモビリティ重みインデックスを返します。
fn mobility_index(moves: u64, board: &Board) -> usize {
    64 + moves.count_ones() as usize - board.get_opponent_moves().count_ones() as usize
}

/// [`Features`] が索引付けする順で各パターンテーブルのサイズを返します。
#[cfg_attr(not(feature = "tuning"), allow(dead_code))]
pub fn pattern_table_sizes() -> [usize; PATTERN_COUNT] {
    PATTERN_TABLE_SIZES
}

/// 各パターン重みインデックスに対して、左右反転（鏡像）盤面に対応する重みインデックスを返します。
///
/// 1 つのパターンの 4 回転は同じテーブルを共有するため、回転に対する対称性は構造上保証されます。
/// さらにすべての重みが自身の鏡像先重みと等しければ、左右反転に対する対称性も成り立ちます。
/// どの鏡像変換も自身の回転に一致しないパターン（長対角線など）は自分自身に写像されます。
#[cfg_attr(not(feature = "tuning"), allow(dead_code))]
pub fn mirror_weights() -> Vec<u32> {
    let mut mirror: Vec<u32> = (0..TOTAL_PATTERN_WEIGHTS as u32).collect();
    let flip = |c: u8| c / 8 * 8 + (7 - c % 8);
    let mut offset = 0;
    for (pattern, size) in FEATURE_COORD.iter().zip(PATTERN_TABLE_SIZES) {
        let n = usize::from(pattern.n_squares);
        let coords = |r: usize| &pattern.feature_coord[r][..n];
        // `to[i]` は、向き `k` のマス `i` が左右反転後に向き `r` のどの位置へ移るかを表します。
        // すなわち、向き `r` は向き `k` が読むマスの鏡像を読みます。
        let to = (0..4)
            .flat_map(|k| (0..4).map(move |r| (k, r)))
            .find_map(|(k, r)| {
                coords(k)
                    .iter()
                    .map(|&c| coords(r).iter().position(|&d| d == flip(c)))
                    .collect::<Option<Vec<usize>>>()
            });
        if let Some(to) = to {
            for state in 0..size {
                let mut image = 0;
                let mut rest = state;
                for i in (0..n).rev() {
                    image += rest % 3 * pattern::definition::POW3[n - 1 - to[i]];
                    rest /= 3;
                }
                mirror[offset + state] = (offset + image) as u32;
            }
        }
        offset += size;
    }
    mirror
}

/// 実行時に使う評価モデルです（パターン順の重みテーブルを持ち、4 回転方向で重みを共有します）。
#[derive(Clone)]
pub struct PatternModel {
    phases: Arc<[PatternPhase]>,
    /// 各フェーズの補助ニューラルネットパラメータ（存在する場合）。
    pen: Option<Arc<[PenPhase]>>,
}
/// 1 フェーズ分の線形評価パラメータ（パターン重み・モビリティ重み・バイアス）です。
#[derive(Clone)]
pub struct PatternPhase {
    weights: Box<[i16]>,
    mobility: [i16; 128],
    bias: i16,
}
impl PatternPhase {
    /// パターン種別ごとの重み配列・モビリティ重み・バイアスから [`PatternPhase`] を構築します。
    pub fn new(
        patterns: Vec<Vec<i16>>,
        mobility: [i16; 128],
        bias: i16,
    ) -> Result<Self, &'static str> {
        if patterns.len() != PATTERN_COUNT
            || patterns
                .iter()
                .zip(PATTERN_TABLE_SIZES)
                .any(|(p, n)| p.len() != n)
        {
            return Err("invalid pattern table dimensions");
        }
        Ok(Self {
            weights: patterns.into_iter().flatten().collect(),
            mobility,
            bias,
        })
    }
    /// [`Features`] が索引付けするのと同じ連結済みフラット配列から [`PatternPhase`] を構築します。
    pub fn from_flat(
        patterns: Vec<i16>,
        mobility: [i16; 128],
        bias: i16,
    ) -> Result<Self, &'static str> {
        if patterns.len() != TOTAL_PATTERN_WEIGHTS {
            return Err("invalid pattern table dimensions");
        }
        Ok(Self {
            weights: patterns.into_boxed_slice(),
            mobility,
            bias,
        })
    }
}
impl PatternModel {
    /// 60 フェーズ分の [`PatternPhase`] から [`PatternModel`] を構築します。
    pub fn new(phases: Vec<PatternPhase>) -> Result<Self, &'static str> {
        if phases.len() != PHASE_COUNT {
            return Err("model requires 60 phases");
        }
        Ok(Self {
            phases: phases.into(),
            pen: None,
        })
    }
    /// 60 フェーズ分の補助ニューラルネットをモデルに付加します。
    pub fn with_pen(mut self, pen: Vec<PenPhase>) -> Result<Self, &'static str> {
        if pen.len() != PHASE_COUNT {
            return Err("PEN requires 60 phases");
        }
        self.pen = Some(pen.into());
        Ok(self)
    }
    /// 補助ニューラルネットを保持しているかを返します。
    pub fn has_pen(&self) -> bool {
        self.pen.is_some()
    }
    /// 全重みが 0 のダミーモデルを返します（ルール検証や探索テスト用であり、学習済み AI ではありません）。
    pub fn zero() -> Self {
        Self::new(
            (0..PHASE_COUNT)
                .map(|_| PatternPhase {
                    weights: vec![0; TOTAL_PATTERN_WEIGHTS].into_boxed_slice(),
                    mobility: [0; 128],
                    bias: 0,
                })
                .collect(),
        )
        .unwrap()
    }
    /// 手番側から見た `board` の静的評価値（石差単位、-64..=64）を返します。
    /// 終局局面でも通常と同じ評価式を適用します（探索の末端では終局スコアを別途使います）。
    pub fn evaluate(&self, board: &Board) -> i32 {
        self.evaluate_state(board, &EvalState::from_board(*board))
    }
    #[cfg(test)]
    pub(crate) fn nonzero_for_tests() -> Self {
        let mut model = Self::zero();
        for (phase, values) in Arc::get_mut(&mut model.phases)
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            for (i, weight) in values.weights.iter_mut().enumerate() {
                *weight = ((i * 37 + phase * 19) % 257) as i16 - 128;
            }
            for (i, weight) in values.mobility.iter_mut().enumerate() {
                *weight = (i as i16 - 64) * 3;
            }
            values.bias = phase as i16 - 30;
        }
        model
    }
    pub(crate) fn evaluate_state(&self, board: &Board, state: &EvalState) -> i32 {
        self.evaluate_state_and_moves(board, state).0
    }
    /// 差分更新された [`EvalState`] を用いて評価値を計算し、同時にモビリティ計算で求めた合法手マスクも返します。
    pub(crate) fn evaluate_state_and_moves(&self, board: &Board, state: &EvalState) -> (i32, u64) {
        let weights = &self.phases[phase_of(board)];
        let mut raw = i32::from(weights.bias);
        let mut offset = 0;
        for (p, size) in PATTERN_TABLE_SIZES.iter().enumerate() {
            for r in 0..4 {
                raw += i32::from(weights.weights[offset + state.current[p * 4 + r] as usize]);
            }
            offset += size;
        }
        let moves = board.get_moves();
        let opponent_moves = board.get_opponent_moves();
        raw += i32::from(
            weights.mobility
                [64 + moves.count_ones() as usize - opponent_moves.count_ones() as usize],
        );
        if let Some(pen) = &self.pen {
            raw += pen[phase_of(board)].evaluate(
                board,
                state,
                moves.count_ones(),
                opponent_moves.count_ones(),
            );
        }
        (((raw + raw.signum() * 64) / 128).clamp(-64, 64), moves)
    }
    /// この評価重みが対応する DPR2 バージョンを返します。
    /// 石数入力を読むネットワーク（`PEN2`）を持つ場合は 4、それ以外のネットワーク（`PEN1`）を持つ場合は 3、
    /// ネットワークを持たない場合は 1 です。[`Model::write_to`](crate::Model::write_to) で較正データを末尾に書くと
    /// バージョン 1 はバージョン 2 になります。
    pub(crate) fn version(&self) -> u32 {
        match &self.pen {
            Some(pen) if pen.iter().any(PenPhase::needs_pen2) => 4,
            Some(_) => 3,
            None => 1,
        }
    }
    /// 較正データを除いた評価モデル全体を書き出します（補助ニューラルネットを持つ場合は DPR2 バージョン 4 または 3、なければバージョン 1）。
    /// 較正データも含めた書き出しは [`Model::write_to`](crate::Model::write_to) を使用してください（レイアウトは `model.rs` 参照）。
    pub fn write_to(&self, mut out: impl Write) -> io::Result<()> {
        let version = self.version();
        if version >= 3 {
            out.write_all(b"DPR2")?;
            out.write_all(&version.to_le_bytes())?;
            self.write_phases(&mut out)?;
            self.write_pen(&mut out)
        } else {
            out.write_all(b"DPR2\x01\x00\x00\x00")?;
            self.write_phases(&mut out)
        }
    }
    /// DPR2 バージョン 1 を読み込みます。全バージョンを読み込むには [`Model::from_file`](crate::Model::from_file) を使ってください。
    pub fn read_from(mut input: impl Read) -> io::Result<Self> {
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        if &magic != b"DPR2\x01\x00\x00\x00" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported pattern model/schema",
            ));
        }
        let model = Self::read_phases(&mut input)?;
        let mut extra = [0];
        if input.read(&mut extra)? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "trailing model data",
            ));
        }
        Ok(model)
    }
    /// 60 フェーズ分の重み（リトルエンディアン `i16` のパターンテーブル、モビリティ、バイアス）を書き出します。
    pub(crate) fn write_phases(&self, mut out: impl Write) -> io::Result<()> {
        let mut bytes = Vec::with_capacity((TOTAL_PATTERN_WEIGHTS + 129) * 2);
        for phase in self.phases.iter() {
            bytes.clear();
            for &value in phase
                .weights
                .iter()
                .chain(phase.mobility.iter())
                .chain(std::iter::once(&phase.bias))
            {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            out.write_all(&bytes)?;
        }
        Ok(())
    }
    /// ネットワーク形式のタグ（バージョン 3 では `PEN1`、バージョン 4 では `PEN2`）と 60 フェーズ分のネットワークを書き出します（`pen.rs` 参照）。
    pub(crate) fn write_pen(&self, mut out: impl Write) -> io::Result<()> {
        let (tag, inputs) = if self.version() == 4 {
            (b"PEN2", PEN_INPUTS)
        } else {
            (b"PEN1", pen::PEN1_INPUTS)
        };
        out.write_all(tag)?;
        for phase in self.pen.iter().flat_map(|p| p.iter()) {
            phase.write(&mut out, inputs)?;
        }
        Ok(())
    }
    /// タグに続く 60 フェーズ分のネットワークを読み込みます（DPR2 バージョン 3 では `PEN1`、バージョン 4 では `PEN2`）。
    pub(crate) fn read_pen(mut self, mut input: impl Read, version: u32) -> io::Result<Self> {
        let (expected, inputs) = if version == 4 {
            (b"PEN2", PEN_INPUTS)
        } else {
            (b"PEN1", pen::PEN1_INPUTS)
        };
        let mut tag = [0; 4];
        input.read_exact(&mut tag)?;
        if &tag != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing pattern embedding network",
            ));
        }
        let pen = (0..PHASE_COUNT)
            .map(|_| PenPhase::read(&mut input, inputs))
            .collect::<io::Result<Vec<_>>>()?;
        self.pen = Some(pen.into());
        Ok(self)
    }
    pub(crate) fn read_phases(mut input: impl Read) -> io::Result<Self> {
        let mut phases = Vec::with_capacity(PHASE_COUNT);
        let mut bytes = vec![0; (TOTAL_PATTERN_WEIGHTS + 129) * 2];
        for _ in 0..PHASE_COUNT {
            input.read_exact(&mut bytes)?;
            let mut values = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes([b[0], b[1]]));
            let weights = values.by_ref().take(TOTAL_PATTERN_WEIGHTS).collect();
            let mobility = std::array::from_fn(|_| values.next().unwrap());
            let bias = values.next().unwrap();
            phases.push(PatternPhase {
                weights,
                mobility,
                bias,
            });
        }
        Ok(Self::new(phases).unwrap())
    }
}

/// ベクトル加算で一括差分更新できるよう、44 レーンを 48 に切り上げたレーン数です。
const LANES: usize = 48;

/// `SQUARE_DELTA[sq][f]` はマス `sq` が特徴量 `f` に持つ 3 進重みです（無関係な場合は 0）。
static SQUARE_DELTA: [[u16; LANES]; 64] = {
    let mut table = [[0u16; LANES]; 64];
    let mut sq = 0;
    while sq < 64 {
        let features = &SQUARE_TO_FEATURES[sq];
        let mut i = 0;
        while i < features.len as usize {
            let feature = features.features[i];
            table[sq][feature.feature_idx as usize] = feature.base3_weight;
            i += 1;
        }
        sq += 1;
    }
    table
};

/// 差分評価用の 3 進パターンインデックスを保持する状態です。
///
/// 1 手進めるたびに 44 個の特徴量を盤面全体から計算し直すと重いため、変化したマス分だけ加減算します。
/// さらに手番側視点（`current`）と相手側視点（`swapped`）の両方をあらかじめ保持しておくことで、
/// 手番交代時に 3 進数の各桁（1 ↔ 2）を反転させる手間をなくしています。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EvalState {
    current: [u16; LANES],
    swapped: [u16; LANES],
}
impl EvalState {
    /// 盤面全体から 44 特徴量（両視点分）の 3 進インデックスを初期化します。
    pub(crate) fn from_board(board: Board) -> Self {
        let mut state = Self {
            current: [0; LANES],
            swapped: [0; LANES],
        };
        for (p, pattern) in FEATURE_COORD.iter().enumerate() {
            for r in 0..4 {
                for &sq in &pattern.feature_coord[r][..pattern.n_squares as usize] {
                    let bit = 1u64 << sq;
                    let digit = if board.player_bits() & bit != 0 {
                        2
                    } else if board.opponent_bits() & bit != 0 {
                        1
                    } else {
                        0
                    };
                    state.current[p * 4 + r] = state.current[p * 4 + r] * 3 + digit;
                    state.swapped[p * 4 + r] =
                        state.swapped[p * 4 + r] * 3 + if digit == 0 { 0 } else { 3 - digit };
                }
            }
        }
        state
    }
    /// パス時に手番側と相手側の視点を入れ替えます。
    pub(crate) fn passed(self) -> Self {
        Self {
            current: self.swapped,
            swapped: self.current,
        }
    }
    /// 着手（`bit` に石を置き、`flips` を裏返す）後の差分インデックスを計算します。
    ///
    /// 着手マスは手番側で 0→2（相手視点で 0→1）、裏返ったマスは手番側で 1→2（相手視点で 2→1）へ
    /// 変化するため、それに対応する 3 進重みを加減算したうえで両視点を入れ替えます。
    #[inline]
    pub(crate) fn played(self, bit: u64, mut flips: u64) -> Self {
        let mut flipped = [0u16; LANES];
        while flips != 0 {
            let delta = &SQUARE_DELTA[flips.trailing_zeros() as usize];
            flips &= flips - 1;
            for (sum, d) in flipped.iter_mut().zip(delta) {
                *sum = sum.wrapping_add(*d);
            }
        }
        let placed = &SQUARE_DELTA[bit.trailing_zeros() as usize];
        let mut next = self;
        for i in 0..LANES {
            next.current[i] = self.swapped[i]
                .wrapping_add(placed[i])
                .wrapping_sub(flipped[i]);
            next.swapped[i] = self.current[i]
                .wrapping_add(placed[i].wrapping_mul(2))
                .wrapping_add(flipped[i]);
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn differential_matches_refresh_through_games() {
        let mut random = 73u64;
        for _ in 0..64 {
            let mut board = Board::new();
            let mut state = EvalState::from_board(board);
            loop {
                assert_eq!(state, EvalState::from_board(board));
                let moves = board.get_moves();
                if moves == 0 {
                    if board.is_terminal() {
                        break;
                    }
                    board = board.board_after_pass();
                    state = state.passed();
                    continue;
                }
                let bit = crate::test_util::random_move(moves, &mut random);
                let flips = board.flip_mask(bit);
                #[cfg(feature = "endgame-pattern-ordering")]
                assert_eq!(
                    state.ordering_child_indices(bit, flips),
                    EvalState::from_board(board.after_known_move(bit, flips)).current,
                );
                state = state.played(bit, flips);
                board = board.after_known_move(bit, flips);
            }
        }
    }
    #[test]
    fn mirror_weights_read_the_mirror_image() {
        let mirror = mirror_weights();
        let moved = (0..mirror.len())
            .filter(|&i| mirror[i] as usize != i)
            .count();
        assert!(moved > mirror.len() / 2, "{moved}");
        for (i, &m) in mirror.iter().enumerate() {
            assert_eq!(mirror[m as usize] as usize, i);
        }
        // 長対角線だけは回転のなかに自身の鏡像が含まれるため不変になりますが、
        // それ以外のすべてのテーブルでは状態同士が鏡像のペアを作ります。
        let mut offset = 0;
        for (p, size) in PATTERN_TABLE_SIZES.into_iter().enumerate() {
            let moved = (offset..offset + size).any(|i| mirror[i] as usize != i);
            assert_eq!(moved, p != PATTERN_COUNT - 1, "pattern {p}");
            offset += size;
        }
        let flip = |bits: u64| {
            (0..64)
                .filter(|sq| bits >> sq & 1 != 0)
                .fold(0u64, |acc, sq| acc | 1 << (sq / 8 * 8 + 7 - sq % 8))
        };
        let mut random = 5u64;
        let mut board = Board::new();
        while !board.is_terminal() {
            let image =
                Board::from_bits(flip(board.player_bits()), flip(board.opponent_bits())).unwrap();
            let mut a: Vec<u32> = Features::of(&board).patterns.to_vec();
            let mut b: Vec<u32> = Features::of(&image)
                .patterns
                .iter()
                .map(|&i| mirror[i as usize])
                .collect();
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(a, b);
            let moves = board.get_moves();
            board = if moves == 0 {
                board.board_after_pass()
            } else {
                board.board_after_move(crate::test_util::random_move(moves, &mut random))
            };
        }
    }
    #[test]
    fn schema_tables_fit_u16() {
        for size in PATTERN_TABLE_SIZES {
            assert!(size <= u16::MAX as usize);
        }
        assert_eq!(pattern::definition::POW3[10], 59049);
    }
}

#[cfg(test)]
mod score_tests {
    use super::*;
    #[test]
    fn features_read_the_weights_evaluate_reads() {
        let model = PatternModel::nonzero_for_tests();
        let mut random = 11u64;
        let mut board = Board::new();
        while !board.is_terminal() {
            let f = Features::of(&board);
            let phase = &model.phases[f.phase];
            let raw = i32::from(phase.bias)
                + f.patterns
                    .iter()
                    .map(|&i| i32::from(phase.weights[i as usize]))
                    .sum::<i32>()
                + i32::from(phase.mobility[f.mobility]);
            assert_eq!(
                model.evaluate(&board),
                ((raw + raw.signum() * 64) / 128).clamp(-64, 64)
            );
            let moves = board.get_moves();
            board = if moves == 0 {
                board.board_after_pass()
            } else {
                board.board_after_move(crate::test_util::random_move(moves, &mut random))
            };
        }
        let flat = PatternPhase::from_flat(vec![1; TOTAL_PATTERN_WEIGHTS], [0; 128], 0).unwrap();
        assert_eq!(flat.weights.len(), TOTAL_PATTERN_WEIGHTS);
        assert!(PatternPhase::from_flat(vec![], [0; 128], 0).is_err());
    }
    #[test]
    fn phase_mobility_and_signed_rounding() {
        let phases = (0..60)
            .map(|phase| {
                let mut mobility = [0; 128];
                mobility[64] = 64;
                PatternPhase {
                    weights: vec![0; TOTAL_PATTERN_WEIGHTS].into_boxed_slice(),
                    mobility,
                    bias: phase as i16 * 128,
                }
            })
            .collect();
        let model = PatternModel::new(phases).unwrap();
        assert_eq!(model.evaluate(&Board::new()), 1);
        let board = Board::new();
        let bit = board.get_moves() & board.get_moves().wrapping_neg();
        let child = board.after_known_move(bit, board.flip_mask(bit));
        assert_eq!(
            model.evaluate(&child),
            if child.get_moves().count_ones() == child.get_opponent_moves().count_ones() {
                2
            } else {
                1
            }
        );
        let negative = PatternModel::new(
            (0..60)
                .map(|_| PatternPhase {
                    weights: vec![0; TOTAL_PATTERN_WEIGHTS].into_boxed_slice(),
                    mobility: [0; 128],
                    bias: -64,
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(negative.evaluate(&Board::new()), -1);
    }
}
