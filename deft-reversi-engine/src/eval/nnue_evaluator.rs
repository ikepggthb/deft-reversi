use crate::board::board::Board;
use crate::file::{invalid_data, DenseLayerData, NnueEvaluatorData, NnueTowerData};
use std::io;
use std::sync::OnceLock;

use super::evaluator::Evaluator;
use super::evaluator_const::{N_BOARD_SQUARES, POW3, SCORE_MAX};
use super::nnue_features::*;

const PLAYER_VIEW: usize = 0;
const OPPONENT_VIEW: usize = 1;
const NNUE_VIEW_COUNT: usize = 2;
const NNUE_OUTPUT_INPUT_SIZE: usize = NNUE_DENSE_LAYER_SIZES[NNUE_DENSE_LAYER_SIZES.len() - 1];
const NNUE_ACCUMULATOR_ROW_SIZE_MAX: usize = NNUE_ACCUMULATOR_SIZE;
const NNUE_PREFETCH_DIST: usize = 4;
const NNUE_PREFETCH_STRIDE_BYTES: usize = 64;

pub struct NnueEvaluator {
    weights: NnueWeights,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NnueState {
    accumulators: [[i32; NNUE_ACCUMULATOR_ROW_SIZE_MAX]; NNUE_VIEW_COUNT],
    accumulator_len: usize,
    active_features: [[usize; NNUE_MAX_ACTIVE_FEATURES_PER_VIEW]; NNUE_VIEW_COUNT],
    active_feature_lens: [usize; NNUE_VIEW_COUNT],
}

struct NnueWeights {
    layout: NnueFeatureLayout,
    accumulator_size: usize,
    dense_input_size: usize,
    input_weights: Vec<Vec<i16>>,
    input_bias: Vec<i32>,
    towers: Vec<NnueTowerWeights>,
    activation_scale: i32,
    weight_scale: i32,
}

struct NnueTowerWeights {
    dense_layers: Vec<DenseLayerWeights>,
    output_weights: Vec<i16>,
    output_bias: i32,
}

struct DenseLayerWeights {
    weights: Vec<Vec<i16>>,
    bias: Vec<i32>,
}

impl Default for NnueEvaluator {
    fn default() -> Self {
        Self {
            weights: NnueWeights::default(),
        }
    }
}

impl Default for NnueWeights {
    fn default() -> Self {
        Self {
            layout: NnueFeatureLayout::default_v2(),
            accumulator_size: NNUE_ACCUMULATOR_SIZE,
            dense_input_size: NNUE_DENSE_INPUT_SIZE,
            input_weights: vec![vec![0; NNUE_ACCUMULATOR_SIZE]; NNUE_INPUT_SIZE],
            input_bias: vec![0; NNUE_ACCUMULATOR_SIZE],
            towers: (0..NNUE_TOWER_COUNT)
                .map(|_| NnueTowerWeights::default())
                .collect(),
            activation_scale: NNUE_ACTIVATION_SCALE,
            weight_scale: NNUE_DEFAULT_WEIGHT_SCALE,
        }
    }
}

impl NnueEvaluator {
    pub fn evaluate(&self, _board: &Board, state: &NnueState) -> i32 {
        let player_accumulator = state.accumulator_for_view(PLAYER_VIEW);
        let opponent_accumulator = state.accumulator_for_view(OPPONENT_VIEW);

        let bucket = phase_bucket(_board);
        let mut activation = Vec::with_capacity(self.weights.dense_input_size);
        append_pairwise_activation_dispatch(&mut activation, &player_accumulator, &self.weights);
        append_pairwise_activation_dispatch(&mut activation, &opponent_accumulator, &self.weights);

        let tower = &self.weights.towers[bucket];
        for layer in &tower.dense_layers {
            activation = evaluate_dense_layer(&activation, layer, &self.weights);
        }

        let score = quantized_dot_dispatch(
            &activation,
            &tower.output_weights,
            tower.output_bias,
            self.weights.weight_scale,
        );
        score.clamp(-SCORE_MAX, SCORE_MAX)
    }

    pub fn evaluate_board_slow(&self, board: &Board) -> i32 {
        let state = self.state_from_board(board);
        self.evaluate(board, &state)
    }

    pub fn state_from_board(&self, board: &Board) -> NnueState {
        NnueState::from_board_with_layout(board, &self.weights.layout).with_accumulators(self)
    }

    pub(crate) fn from_data(data: NnueEvaluatorData) -> io::Result<Self> {
        Ok(Self {
            weights: NnueWeights::from_data(data)?,
        })
    }

    // Serialization hook retained for engine-file export tooling.
    #[allow(dead_code)]
    pub(crate) fn to_data(&self) -> NnueEvaluatorData {
        self.weights.to_data()
    }

    pub(crate) fn validate_data(data: &NnueEvaluatorData) -> io::Result<()> {
        let _ = NnueWeights::from_data(data.clone())?;
        Ok(())
    }

    fn build_accumulator(&self, active_features: &[usize]) -> Vec<i32> {
        build_accumulator_dispatch(&self.weights, active_features)
    }
}

impl Evaluator for NnueEvaluator {
    fn evaluate(&self, board: &Board) -> i32 {
        self.evaluate_board_slow(board)
    }
}

fn force_scalar() -> bool {
    static FORCE_SCALAR: OnceLock<bool> = OnceLock::new();
    *FORCE_SCALAR
        .get_or_init(|| std::env::var("DEFT_NNUE_FORCE_SCALAR").is_ok_and(|value| value == "1"))
}

fn build_accumulator_dispatch(weights: &NnueWeights, active_features: &[usize]) -> Vec<i32> {
    if !force_scalar() {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if std::is_x86_feature_detected!("avx2") {
                return unsafe { build_accumulator_avx2(weights, active_features) };
            }
        }
    }
    build_accumulator_scalar(weights, active_features)
}

fn build_accumulator_scalar(weights: &NnueWeights, active_features: &[usize]) -> Vec<i32> {
    let mut accumulator = Vec::with_capacity(weights.accumulator_row_size());
    accumulator.extend_from_slice(&weights.input_bias);
    accumulator.resize(weights.accumulator_row_size(), 0);
    for (feature_idx, &feature_id) in active_features.iter().enumerate() {
        prefetch_weight_row_dispatch(weights, active_features, feature_idx + NNUE_PREFETCH_DIST);
        let row = &weights.input_weights[feature_id];
        for (acc, &weight) in accumulator.iter_mut().zip(row) {
            *acc += weight as i32;
        }
    }
    accumulator
}

#[inline(always)]
fn prefetch_weight_row_dispatch(weights: &NnueWeights, feature_ids: &[usize], feature_idx: usize) {
    if feature_idx >= feature_ids.len() || force_scalar() {
        return;
    }
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse") {
            unsafe {
                prefetch_weight_row_x86(weights, feature_ids[feature_idx]);
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn prefetch_weight_row_x86(weights: &NnueWeights, feature_id: usize) {
    use std::arch::x86_64::*;

    let row = &weights.input_weights[feature_id];
    let row_bytes = weights.accumulator_row_size() * std::mem::size_of::<i16>();
    let mut offset = 0usize;
    while offset < row_bytes {
        _mm_prefetch(row.as_ptr().cast::<i8>().add(offset), _MM_HINT_T0);
        offset += NNUE_PREFETCH_STRIDE_BYTES;
    }
}

#[cfg(target_arch = "x86")]
#[inline(always)]
unsafe fn prefetch_weight_row_x86(weights: &NnueWeights, feature_id: usize) {
    use std::arch::x86::*;

    let row = &weights.input_weights[feature_id];
    let row_bytes = weights.accumulator_row_size() * std::mem::size_of::<i16>();
    let mut offset = 0usize;
    while offset < row_bytes {
        _mm_prefetch(row.as_ptr().cast::<i8>().add(offset), _MM_HINT_T0);
        offset += NNUE_PREFETCH_STRIDE_BYTES;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn build_accumulator_avx2(weights: &NnueWeights, active_features: &[usize]) -> Vec<i32> {
    use std::arch::x86_64::*;

    let mut accumulator = Vec::with_capacity(weights.accumulator_row_size());
    accumulator.extend_from_slice(&weights.input_bias);
    accumulator.resize(weights.accumulator_row_size(), 0);
    for (feature_idx, &feature_id) in active_features.iter().enumerate() {
        prefetch_weight_row_dispatch(weights, active_features, feature_idx + NNUE_PREFETCH_DIST);
        let row = &weights.input_weights[feature_id];
        let mut idx = 0usize;
        while idx + 16 <= weights.accumulator_row_size() {
            let row_ptr = row.as_ptr().add(idx) as *const __m128i;
            let lo16 = _mm_loadu_si128(row_ptr);
            let hi16 = _mm_loadu_si128(row_ptr.add(1));
            let lo32 = _mm256_cvtepi16_epi32(lo16);
            let hi32 = _mm256_cvtepi16_epi32(hi16);

            let acc_ptr = accumulator.as_mut_ptr().add(idx) as *mut __m256i;
            let acc_lo = _mm256_loadu_si256(acc_ptr);
            let acc_hi = _mm256_loadu_si256(acc_ptr.add(1));
            _mm256_storeu_si256(acc_ptr, _mm256_add_epi32(acc_lo, lo32));
            _mm256_storeu_si256(acc_ptr.add(1), _mm256_add_epi32(acc_hi, hi32));
            idx += 16;
        }
        while idx < weights.accumulator_row_size() {
            accumulator[idx] += row[idx] as i32;
            idx += 1;
        }
    }
    accumulator
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "avx2")]
unsafe fn build_accumulator_avx2(weights: &NnueWeights, active_features: &[usize]) -> Vec<i32> {
    use std::arch::x86::*;

    let mut accumulator = Vec::with_capacity(weights.accumulator_row_size());
    accumulator.extend_from_slice(&weights.input_bias);
    accumulator.resize(weights.accumulator_row_size(), 0);
    for (feature_idx, &feature_id) in active_features.iter().enumerate() {
        prefetch_weight_row_dispatch(weights, active_features, feature_idx + NNUE_PREFETCH_DIST);
        let row = &weights.input_weights[feature_id];
        let mut idx = 0usize;
        while idx + 16 <= weights.accumulator_row_size() {
            let row_ptr = row.as_ptr().add(idx) as *const __m128i;
            let lo16 = _mm_loadu_si128(row_ptr);
            let hi16 = _mm_loadu_si128(row_ptr.add(1));
            let lo32 = _mm256_cvtepi16_epi32(lo16);
            let hi32 = _mm256_cvtepi16_epi32(hi16);

            let acc_ptr = accumulator.as_mut_ptr().add(idx) as *mut __m256i;
            let acc_lo = _mm256_loadu_si256(acc_ptr);
            let acc_hi = _mm256_loadu_si256(acc_ptr.add(1));
            _mm256_storeu_si256(acc_ptr, _mm256_add_epi32(acc_lo, lo32));
            _mm256_storeu_si256(acc_ptr.add(1), _mm256_add_epi32(acc_hi, hi32));
            idx += 16;
        }
        while idx < weights.accumulator_row_size() {
            accumulator[idx] += row[idx] as i32;
            idx += 1;
        }
    }
    accumulator
}

impl NnueState {
    #[allow(dead_code)]
    pub fn from_board(board: &Board) -> Self {
        Self::from_board_with_layout(board, &NnueFeatureLayout::default_v2())
    }

    pub fn from_board_with_layout(board: &Board, layout: &NnueFeatureLayout) -> Self {
        let player_moves = board.moves();
        let opponent_moves = board.opponent_moves();
        let player_features = active_features_for_view_array(
            layout,
            board.player,
            board.opponent,
            player_moves,
            opponent_moves,
        );
        let opponent_features = active_features_for_view_array(
            layout,
            board.opponent,
            board.player,
            opponent_moves,
            player_moves,
        );
        Self {
            accumulators: [[0; NNUE_ACCUMULATOR_ROW_SIZE_MAX]; NNUE_VIEW_COUNT],
            accumulator_len: 0,
            active_features: [player_features.items, opponent_features.items],
            active_feature_lens: [player_features.len, opponent_features.len],
        }
    }

    pub fn active_features_for_view(&self, view: usize) -> &[usize] {
        &self.active_features[view][..self.active_feature_lens[view]]
    }

    pub fn accumulator_for_view(&self, view: usize) -> &[i32] {
        &self.accumulators[view][..self.accumulator_len]
    }

    #[allow(dead_code)]
    pub fn active_features(&self) -> &[usize] {
        self.active_features_for_view(PLAYER_VIEW)
    }

    fn with_accumulators(mut self, evaluator: &NnueEvaluator) -> Self {
        self.accumulator_len = evaluator.weights.accumulator_row_size();
        for view in 0..NNUE_VIEW_COUNT {
            let accumulator = evaluator.build_accumulator(self.active_features_for_view(view));
            self.accumulators[view][..self.accumulator_len].copy_from_slice(&accumulator);
        }
        self
    }
}

impl NnueWeights {
    fn accumulator_row_size(&self) -> usize {
        self.accumulator_size
    }

    fn from_data(data: NnueEvaluatorData) -> io::Result<Self> {
        let acc_size = if data.acc_size == 0 {
            data.accumulator_size
        } else {
            data.acc_size
        };
        if data.accumulator_size != acc_size {
            return Err(invalid_data(format!(
                "accumulator_size must match acc_size, got {}/{}",
                data.accumulator_size, acc_size
            )));
        }
        if !matches!(acc_size, 64 | 128 | 256) {
            return Err(invalid_data(format!(
                "accumulator_size must be one of 64,128,256, got {acc_size}"
            )));
        }
        let layout = NnueFeatureLayout::new(&data.pattern_set, data.share_rotations)
            .map_err(invalid_data)?;
        let dense_input_size = acc_size;

        if data.input_size != layout.input_size {
            return Err(invalid_data(format!(
                "input_size must be {}, got {}",
                layout.input_size, data.input_size
            )));
        }
        if data.activation_scale != NNUE_ACTIVATION_SCALE {
            return Err(invalid_data(format!(
                "activation_scale must be {NNUE_ACTIVATION_SCALE}, got {}",
                data.activation_scale
            )));
        }
        if data.weight_scale <= 0 {
            return Err(invalid_data(format!(
                "weight_scale must be positive, got {}",
                data.weight_scale
            )));
        }
        if data.tower_count != NNUE_TOWER_COUNT || data.towers.len() != NNUE_TOWER_COUNT {
            return Err(invalid_data(format!(
                "tower_count/towers length must be {NNUE_TOWER_COUNT}, got {}/{}",
                data.tower_count,
                data.towers.len()
            )));
        }
        if !data.pairwise {
            return Err(invalid_data("pairwise must be true for NNUE v2"));
        }
        if data.input_weights.len() != layout.input_size {
            return Err(invalid_data(format!(
                "input_weights length must be {}, got {}",
                layout.input_size,
                data.input_weights.len()
            )));
        }
        if data.input_bias.len() != acc_size {
            return Err(invalid_data(format!(
                "input_bias length must be {acc_size}, got {}",
                data.input_bias.len()
            )));
        }
        validate_matrix_width(&data.input_weights, acc_size, "input_weights")?;
        let towers = data
            .towers
            .into_iter()
            .enumerate()
            .map(|item| NnueTowerWeights::from_data(item, dense_input_size))
            .collect::<io::Result<Vec<_>>>()?;

        Ok(Self {
            layout,
            accumulator_size: acc_size,
            dense_input_size,
            input_weights: data.input_weights,
            input_bias: data.input_bias,
            towers,
            activation_scale: data.activation_scale,
            weight_scale: data.weight_scale,
        })
    }

    // Serialization hook retained for engine-file export tooling.
    #[allow(dead_code)]
    fn to_data(&self) -> NnueEvaluatorData {
        NnueEvaluatorData {
            input_size: self.layout.input_size,
            acc_size: self.accumulator_size,
            pattern_set: self.layout.pattern_set.clone(),
            share_rotations: self.layout.share_rotations,
            accumulator_size: self.accumulator_size,
            activation_scale: self.activation_scale,
            weight_scale: self.weight_scale,
            tower_count: NNUE_TOWER_COUNT,
            pairwise: true,
            input_weights: self.input_weights.clone(),
            input_bias: self.input_bias.clone(),
            towers: self
                .towers
                .iter()
                .map(|tower| tower.to_data(self.dense_input_size))
                .collect(),
        }
    }
}

impl Default for NnueTowerWeights {
    fn default() -> Self {
        Self {
            dense_layers: default_dense_layers(),
            output_weights: vec![0; NNUE_OUTPUT_INPUT_SIZE],
            output_bias: 0,
        }
    }
}

impl NnueTowerWeights {
    fn from_data(
        (tower_idx, data): (usize, NnueTowerData),
        dense_input_size: usize,
    ) -> io::Result<Self> {
        if data.dense_layers.len() != NNUE_DENSE_LAYER_SIZES.len() {
            return Err(invalid_data(format!(
                "towers[{tower_idx}].dense_layers length must be {}, got {}",
                NNUE_DENSE_LAYER_SIZES.len(),
                data.dense_layers.len()
            )));
        }
        if data.output_weights.len() != NNUE_OUTPUT_INPUT_SIZE {
            return Err(invalid_data(format!(
                "towers[{tower_idx}].output_weights length must be {NNUE_OUTPUT_INPUT_SIZE}, got {}",
                data.output_weights.len()
            )));
        }
        let dense_layers = data
            .dense_layers
            .into_iter()
            .enumerate()
            .map(|item| DenseLayerWeights::from_data(item, dense_input_size))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            dense_layers,
            output_weights: data.output_weights,
            output_bias: data.output_bias,
        })
    }

    // Serialization hook retained for engine-file export tooling.
    #[allow(dead_code)]
    fn to_data(&self, dense_input_size: usize) -> NnueTowerData {
        NnueTowerData {
            dense_layers: self
                .dense_layers
                .iter()
                .enumerate()
                .map(|(idx, layer)| {
                    layer.to_data(
                        dense_layer_input_size(idx, dense_input_size),
                        NNUE_DENSE_LAYER_SIZES[idx],
                    )
                })
                .collect(),
            output_weights: self.output_weights.to_vec(),
            output_bias: self.output_bias,
        }
    }
}

impl DenseLayerWeights {
    fn from_data(
        (idx, data): (usize, DenseLayerData),
        dense_input_size: usize,
    ) -> io::Result<Self> {
        let expected_input_size = dense_layer_input_size(idx, dense_input_size);
        let expected_output_size = NNUE_DENSE_LAYER_SIZES[idx];

        if data.input_size != expected_input_size {
            return Err(invalid_data(format!(
                "dense_layers[{idx}].input_size must be {expected_input_size}, got {}",
                data.input_size
            )));
        }
        if data.output_size != expected_output_size {
            return Err(invalid_data(format!(
                "dense_layers[{idx}].output_size must be {expected_output_size}, got {}",
                data.output_size
            )));
        }
        if data.weights.len() != expected_output_size {
            return Err(invalid_data(format!(
                "dense_layers[{idx}].weights length must be {expected_output_size}, got {}",
                data.weights.len()
            )));
        }
        if data.bias.len() != expected_output_size {
            return Err(invalid_data(format!(
                "dense_layers[{idx}].bias length must be {expected_output_size}, got {}",
                data.bias.len()
            )));
        }

        for (row_idx, row) in data.weights.iter().enumerate() {
            if row.len() != expected_input_size {
                return Err(invalid_data(format!(
                    "dense_layers[{idx}].weights[{row_idx}] length must be {expected_input_size}, got {}",
                    row.len()
                )));
            }
        }

        Ok(Self {
            weights: data.weights,
            bias: data.bias,
        })
    }

    // Serialization hook retained for engine-file export tooling.
    #[allow(dead_code)]
    fn to_data(&self, input_size: usize, output_size: usize) -> DenseLayerData {
        DenseLayerData {
            input_size,
            output_size,
            weights: self.weights.clone(),
            bias: self.bias.clone(),
        }
    }
}

fn validate_matrix_width(matrix: &[Vec<i16>], expected_width: usize, name: &str) -> io::Result<()> {
    for (idx, row) in matrix.iter().enumerate() {
        if row.len() != expected_width {
            return Err(invalid_data(format!(
                "{name}[{idx}] length must be {expected_width}, got {}",
                row.len()
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ActiveFeatureBuffer {
    items: [usize; NNUE_MAX_ACTIVE_FEATURES_PER_VIEW],
    len: usize,
}

impl ActiveFeatureBuffer {
    fn new() -> Self {
        Self {
            items: [0; NNUE_MAX_ACTIVE_FEATURES_PER_VIEW],
            len: 0,
        }
    }

    fn push(&mut self, feature_id: usize) {
        debug_assert!(self.len < self.items.len());
        self.items[self.len] = feature_id;
        self.len += 1;
    }
}

fn active_features_for_view_array(
    layout: &NnueFeatureLayout,
    own: u64,
    opponent: u64,
    own_moves: u64,
    opponent_moves: u64,
) -> ActiveFeatureBuffer {
    let mut active_features = ActiveFeatureBuffer::new();
    append_bitboard_features_array(&mut active_features, NNUE_STONE_OFFSET, own);
    append_bitboard_features_array(
        &mut active_features,
        NNUE_STONE_OFFSET + N_BOARD_SQUARES,
        opponent,
    );
    append_bitboard_features_array(&mut active_features, NNUE_LEGAL_MOVE_OFFSET, own_moves);
    append_bitboard_features_array(
        &mut active_features,
        NNUE_LEGAL_MOVE_OFFSET + N_BOARD_SQUARES,
        opponent_moves,
    );
    append_quadrant_features_array(&mut active_features, own | opponent);
    append_pattern_features_array(&mut active_features, layout, own, opponent);

    debug_assert!(active_features.len <= layout.max_active_features_per_view);
    debug_assert!(active_features.items[..active_features.len]
        .iter()
        .all(|&id| id < layout.input_size));
    active_features
}

fn append_bitboard_features_array(
    active_features: &mut ActiveFeatureBuffer,
    offset: usize,
    mut bitboard: u64,
) {
    while bitboard != 0 {
        let square = bitboard.trailing_zeros() as usize;
        active_features.push(offset + square);
        bitboard &= bitboard - 1;
    }
}

fn append_quadrant_features_array(active_features: &mut ActiveFeatureBuffer, occupied: u64) {
    let total_empty = N_BOARD_SQUARES - occupied.count_ones() as usize;
    for (idx, &mask) in NNUE_QUADRANT_MASKS.iter().enumerate() {
        let empty_count = (mask & !occupied).count_ones() as usize;
        debug_assert!(empty_count < NNUE_QUADRANT_BUCKETS);
        active_features.push(NNUE_QUADRANT_OFFSET + idx * NNUE_QUADRANT_BUCKETS + empty_count);
        active_features.push(
            NNUE_QUADRANT_PARITY_OFFSET + idx * NNUE_QUADRANT_PARITY_BUCKETS + (empty_count & 1),
        );
    }
    active_features.push(NNUE_GLOBAL_PARITY_OFFSET + (total_empty & 1));
}

fn append_pattern_features_array(
    active_features: &mut ActiveFeatureBuffer,
    layout: &NnueFeatureLayout,
    own: u64,
    opponent: u64,
) {
    for (idx, instance) in layout.pattern_instances.iter().enumerate() {
        let pattern_code = pattern_code(instance, layout.share_rotations, own, opponent);
        active_features
            .push(NNUE_PATTERN_OFFSET + layout.pattern_instance_offsets[idx] + pattern_code);
    }
}

fn pattern_code(
    instance: &NnuePatternInstance,
    share_rotations: bool,
    own: u64,
    opponent: u64,
) -> usize {
    let mut code = 0usize;
    let mut reverse_code = 0usize;
    for (idx, &square) in instance.squares.iter().take(instance.n_squares).enumerate() {
        let bit = 1u64 << square;
        let digit = if own & bit != 0 {
            1
        } else if opponent & bit != 0 {
            2
        } else {
            0
        };
        code = code * 3 + digit;
        reverse_code += digit * POW3[idx];
    }
    if share_rotations && instance.pattern_type == 10 {
        code = code.min(reverse_code);
    }
    debug_assert!(code < POW3[instance.n_squares]);
    code
}

fn default_dense_layers() -> Vec<DenseLayerWeights> {
    let mut layers = Vec::with_capacity(NNUE_DENSE_LAYER_SIZES.len());
    let mut input_size = NNUE_DENSE_INPUT_SIZE;
    for &output_size in &NNUE_DENSE_LAYER_SIZES {
        layers.push(DenseLayerWeights {
            weights: vec![vec![0; input_size]; output_size],
            bias: vec![0; output_size],
        });
        input_size = output_size;
    }
    layers
}

fn dense_layer_input_size(idx: usize, dense_input_size: usize) -> usize {
    if idx == 0 {
        dense_input_size
    } else {
        NNUE_DENSE_LAYER_SIZES[idx - 1]
    }
}

fn append_pairwise_activation_dispatch(
    output: &mut Vec<i32>,
    accumulator: &[i32],
    weights: &NnueWeights,
) {
    if !force_scalar() {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if std::is_x86_feature_detected!("avx2") {
                unsafe {
                    append_pairwise_activation_avx2(output, accumulator, weights);
                }
                return;
            }
        }
    }
    append_pairwise_activation_scalar(output, accumulator, weights);
}

fn append_pairwise_activation_scalar(
    output: &mut Vec<i32>,
    accumulator: &[i32],
    weights: &NnueWeights,
) {
    let pairwise_size = weights.accumulator_size / 2;
    for idx in 0..pairwise_size {
        let a = accumulator[idx].clamp(0, NNUE_ACTIVATION_MAX) as i64;
        let b = accumulator[idx + pairwise_size].clamp(0, NNUE_ACTIVATION_MAX) as i64;
        output.push(((a * b) >> 12) as i32);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn append_pairwise_activation_avx2(
    output: &mut Vec<i32>,
    accumulator: &[i32],
    weights: &NnueWeights,
) {
    use std::arch::x86_64::*;

    let pairwise_size = weights.accumulator_size / 2;
    let start_len = output.len();
    output.resize(start_len + pairwise_size, 0);
    let out = output.as_mut_ptr().add(start_len);
    let max = _mm256_set1_epi32(NNUE_ACTIVATION_MAX);
    let zero = _mm256_setzero_si256();
    let mut idx = 0usize;
    while idx + 8 <= pairwise_size {
        let a = _mm256_loadu_si256(accumulator.as_ptr().add(idx) as *const __m256i);
        let b = _mm256_loadu_si256(accumulator.as_ptr().add(idx + pairwise_size) as *const __m256i);
        let a = _mm256_min_epi32(_mm256_max_epi32(a, zero), max);
        let b = _mm256_min_epi32(_mm256_max_epi32(b, zero), max);
        let even = _mm256_mul_epi32(a, b);
        let odd = _mm256_mul_epi32(_mm256_srli_epi64(a, 32), _mm256_srli_epi64(b, 32));
        let even = _mm256_srli_epi64(even, 12);
        let odd = _mm256_slli_epi64(_mm256_srli_epi64(odd, 12), 32);
        _mm256_storeu_si256(
            out.add(idx) as *mut __m256i,
            _mm256_blend_epi32(even, odd, 0b10101010),
        );
        idx += 8;
    }
    while idx < pairwise_size {
        let a = accumulator[idx].clamp(0, NNUE_ACTIVATION_MAX) as i64;
        let b = accumulator[idx + pairwise_size].clamp(0, NNUE_ACTIVATION_MAX) as i64;
        *out.add(idx) = ((a * b) >> 12) as i32;
        idx += 1;
    }
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "avx2")]
unsafe fn append_pairwise_activation_avx2(
    output: &mut Vec<i32>,
    accumulator: &[i32],
    weights: &NnueWeights,
) {
    use std::arch::x86::*;

    let pairwise_size = weights.accumulator_size / 2;
    let start_len = output.len();
    output.resize(start_len + pairwise_size, 0);
    let out = output.as_mut_ptr().add(start_len);
    let max = _mm256_set1_epi32(NNUE_ACTIVATION_MAX);
    let zero = _mm256_setzero_si256();
    let mut idx = 0usize;
    while idx + 8 <= pairwise_size {
        let a = _mm256_loadu_si256(accumulator.as_ptr().add(idx) as *const __m256i);
        let b = _mm256_loadu_si256(accumulator.as_ptr().add(idx + pairwise_size) as *const __m256i);
        let a = _mm256_min_epi32(_mm256_max_epi32(a, zero), max);
        let b = _mm256_min_epi32(_mm256_max_epi32(b, zero), max);
        let even = _mm256_mul_epi32(a, b);
        let odd = _mm256_mul_epi32(_mm256_srli_epi64(a, 32), _mm256_srli_epi64(b, 32));
        let even = _mm256_srli_epi64(even, 12);
        let odd = _mm256_slli_epi64(_mm256_srli_epi64(odd, 12), 32);
        _mm256_storeu_si256(
            out.add(idx) as *mut __m256i,
            _mm256_blend_epi32(even, odd, 0b10101010),
        );
        idx += 8;
    }
    while idx < pairwise_size {
        let a = accumulator[idx].clamp(0, NNUE_ACTIVATION_MAX) as i64;
        let b = accumulator[idx + pairwise_size].clamp(0, NNUE_ACTIVATION_MAX) as i64;
        *out.add(idx) = ((a * b) >> 12) as i32;
        idx += 1;
    }
}

fn phase_bucket(board: &Board) -> usize {
    let empties = N_BOARD_SQUARES - (board.player | board.opponent).count_ones() as usize;
    (empties / 8).min(NNUE_TOWER_COUNT - 1)
}

fn evaluate_dense_layer(
    input: &[i32],
    layer: &DenseLayerWeights,
    weights: &NnueWeights,
) -> Vec<i32> {
    layer
        .weights
        .iter()
        .zip(&layer.bias)
        .map(|(layer_weights, &bias)| {
            quantized_dot_dispatch(input, layer_weights, bias, weights.weight_scale)
                .clamp(0, weights.activation_scale - 1)
        })
        .collect()
}

fn quantized_dot(input: &[i32], weights: &[i16], bias: i32, weight_scale: i32) -> i32 {
    let mut raw = bias as i64;
    for (&value, &weight) in input.iter().zip(weights) {
        raw += value as i64 * weight as i64;
    }
    round_div_i64(raw, weight_scale as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn quantized_dot_dispatch(input: &[i32], weights: &[i16], bias: i32, weight_scale: i32) -> i32 {
    if !force_scalar() {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if std::is_x86_feature_detected!("avx2") {
                return unsafe { quantized_dot_avx2(input, weights, bias, weight_scale) };
            }
        }
    }
    quantized_dot(input, weights, bias, weight_scale)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn quantized_dot_avx2(input: &[i32], weights: &[i16], bias: i32, weight_scale: i32) -> i32 {
    use std::arch::x86_64::*;

    debug_assert_eq!(input.len(), weights.len());
    let mut acc64 = _mm256_setzero_si256();
    let mut idx = 0usize;
    while idx + 8 <= input.len() {
        let lo = _mm_loadu_si128(input.as_ptr().add(idx) as *const __m128i);
        let hi = _mm_loadu_si128(input.as_ptr().add(idx + 4) as *const __m128i);
        let packed_input = _mm_packs_epi32(lo, hi);
        let packed_weights = _mm_loadu_si128(weights.as_ptr().add(idx) as *const __m128i);
        let products = _mm_madd_epi16(packed_input, packed_weights);
        acc64 = _mm256_add_epi64(acc64, _mm256_cvtepi32_epi64(products));
        idx += 8;
    }

    let mut lanes = [0i64; 4];
    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, acc64);
    let mut raw = bias as i64 + lanes.iter().sum::<i64>();
    while idx < input.len() {
        raw += input[idx] as i64 * weights[idx] as i64;
        idx += 1;
    }
    round_div_i64(raw, weight_scale as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "avx2")]
unsafe fn quantized_dot_avx2(input: &[i32], weights: &[i16], bias: i32, weight_scale: i32) -> i32 {
    use std::arch::x86::*;

    debug_assert_eq!(input.len(), weights.len());
    let mut acc64 = _mm256_setzero_si256();
    let mut idx = 0usize;
    while idx + 8 <= input.len() {
        let lo = _mm_loadu_si128(input.as_ptr().add(idx) as *const __m128i);
        let hi = _mm_loadu_si128(input.as_ptr().add(idx + 4) as *const __m128i);
        let packed_input = _mm_packs_epi32(lo, hi);
        let packed_weights = _mm_loadu_si128(weights.as_ptr().add(idx) as *const __m128i);
        let products = _mm_madd_epi16(packed_input, packed_weights);
        acc64 = _mm256_add_epi64(acc64, _mm256_cvtepi32_epi64(products));
        idx += 8;
    }

    let mut lanes = [0i64; 4];
    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, acc64);
    let mut raw = bias as i64 + lanes.iter().sum::<i64>();
    while idx < input.len() {
        raw += input[idx] as i64 * weights[idx] as i64;
        idx += 1;
    }
    round_div_i64(raw, weight_scale as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn round_div_i64(value: i64, divisor: i64) -> i64 {
    debug_assert!(divisor > 0);
    if value > 0 {
        (value + divisor / 2) / divisor
    } else if value < 0 {
        (value - divisor / 2) / divisor
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{
        DenseLayerData, EngineFile, EvaluatorData, Metadata, NnueEvaluatorData, NnueTowerData,
    };
    use crate::search::mpc::MpcConfig;
    use serde::Deserialize;
    use std::collections::HashMap;

    const D3: u64 = 1u64 << 19;

    fn played_board() -> Board {
        let board = Board::new();
        let flip = board.flip_bit(D3);
        board.make_move_from_flip_bit(D3, flip)
    }

    #[test]
    fn default_nnue_returns_zero_for_sample_boards() {
        let evaluator = NnueEvaluator::default();
        for board in [Board::new(), played_board()] {
            assert_eq!(evaluator.evaluate_board_slow(&board), 0);
        }
    }

    #[test]
    fn accumulator_dispatch_matches_scalar() {
        let mut data = NnueEvaluatorData::default();
        for (feature_idx, row) in data.input_weights.iter_mut().enumerate().take(512) {
            for (idx, weight) in row.iter_mut().enumerate() {
                *weight = (((feature_idx * 31 + idx * 17) % 257) as i16) - 128;
            }
        }
        for (idx, bias) in data.input_bias.iter_mut().enumerate() {
            *bias = ((idx * 13) % 97) as i32 - 48;
        }
        let weights = NnueWeights::from_data(data).unwrap();
        let active_features: Vec<usize> = (0..512).step_by(7).collect();
        assert_eq!(
            build_accumulator_dispatch(&weights, &active_features),
            build_accumulator_scalar(&weights, &active_features)
        );
    }

    fn next_u32(seed: &mut u64) -> u32 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (*seed >> 32) as u32
    }

    #[test]
    fn pairwise_dispatch_matches_scalar() {
        let weights = NnueWeights::default();
        let mut seed = 0x6d5a_56da_cafe_f00d;
        for _ in 0..512 {
            let mut accumulator = vec![0; weights.accumulator_row_size()];
            for value in accumulator.iter_mut().take(weights.accumulator_size) {
                *value = (next_u32(&mut seed) % 8192) as i32 - 2048;
            }
            let mut scalar = Vec::new();
            append_pairwise_activation_scalar(&mut scalar, &accumulator, &weights);
            let mut dispatched = Vec::new();
            append_pairwise_activation_dispatch(&mut dispatched, &accumulator, &weights);
            assert_eq!(dispatched, scalar);
        }
    }

    #[test]
    fn quantized_dot_dispatch_matches_scalar() {
        let mut seed = 0x1234_5678_9abc_def0;
        for len in [1usize, 3, 8, 17, 32, 128, 256] {
            for _ in 0..128 {
                let input: Vec<i32> = (0..len)
                    .map(|_| (next_u32(&mut seed) % NNUE_ACTIVATION_SCALE as u32) as i32)
                    .collect();
                let weights: Vec<i16> = (0..len)
                    .map(|_| (next_u32(&mut seed) % 511) as i16 - 255)
                    .collect();
                let bias = (next_u32(&mut seed) % 200_001) as i32 - 100_000;
                let weight_scale = (next_u32(&mut seed) % 4096) as i32 + 1;
                assert_eq!(
                    quantized_dot_dispatch(&input, &weights, bias, weight_scale),
                    quantized_dot(&input, &weights, bias, weight_scale),
                    "len={len} bias={bias} weight_scale={weight_scale}"
                );
            }
        }
    }

    #[test]
    fn nnue_state_contains_two_viewpoints_in_range() {
        for board in [Board::new(), played_board()] {
            let state = NnueState::from_board(&board);
            for view in 0..NNUE_VIEW_COUNT {
                let features = state.active_features_for_view(view);
                assert!(!features.is_empty());
                assert!(features.len() <= NNUE_MAX_ACTIVE_FEATURES_PER_VIEW);
                assert!(features.iter().all(|&id| id < NNUE_INPUT_SIZE));
            }
        }
    }

    #[test]
    fn initial_board_has_spec_feature_groups() {
        let state = NnueState::from_board(&Board::new());
        let features = state.active_features_for_view(PLAYER_VIEW);
        assert!(features.iter().any(|&id| id < NNUE_LEGAL_MOVE_OFFSET));
        assert!(features
            .iter()
            .any(|&id| (NNUE_LEGAL_MOVE_OFFSET..NNUE_QUADRANT_OFFSET).contains(&id)));
        assert!(features
            .iter()
            .any(|&id| (NNUE_QUADRANT_OFFSET..NNUE_PATTERN_OFFSET).contains(&id)));
        assert!(features.iter().any(|&id| id >= NNUE_PATTERN_OFFSET));
    }

    #[test]
    fn invalid_nnue_shapes_return_invalid_data() {
        assert!(NnueEvaluator::validate_data(&NnueEvaluatorData::default()).is_ok());

        let mut data = NnueEvaluator::default().to_data();
        data.input_size -= 1;
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.accumulator_size = 64;
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.activation_scale -= 1;
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.weight_scale = 0;
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.input_weights.pop();
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.input_bias.pop();
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.towers.pop();
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.towers[0].dense_layers[0].input_size -= 1;
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.towers[0].dense_layers[0].output_size -= 1;
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.towers[0].dense_layers[0].weights.pop();
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.towers[0].dense_layers[0].weights[0].pop();
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.towers[0].dense_layers[0].bias.pop();
        assert!(NnueEvaluator::validate_data(&data).is_err());

        let mut data = NnueEvaluator::default().to_data();
        data.towers[0].output_weights.pop();
        assert!(NnueEvaluator::validate_data(&data).is_err());
    }

    #[derive(Deserialize)]
    struct ActiveFeatureFixture {
        cases: Vec<ActiveFeatureCase>,
    }

    #[derive(Deserialize)]
    struct ActiveFeatureCase {
        name: String,
        own: u64,
        opponent: u64,
        active_features: [Vec<usize>; 2],
    }

    #[derive(Deserialize)]
    struct InferenceFixture {
        model: CompactNnueModel,
        cases: Vec<InferenceCase>,
    }

    #[derive(Deserialize)]
    struct InferenceCase {
        name: String,
        own: u64,
        opponent: u64,
        expected: i32,
    }

    #[derive(Deserialize)]
    struct CompactNnueModel {
        weight_scale: i32,
        input_bias: Vec<i32>,
        input_weights: HashMap<usize, Vec<i16>>,
        towers: Vec<CompactTower>,
    }

    #[derive(Deserialize)]
    struct CompactTower {
        dense_layers: Vec<CompactDenseLayer>,
        output_weights: Vec<i16>,
        output_bias: i32,
    }

    #[derive(Deserialize)]
    struct CompactDenseLayer {
        weights: Vec<Vec<i16>>,
        bias: Vec<i32>,
    }

    fn board_from_case(own: u64, opponent: u64) -> Board {
        Board {
            player: own,
            opponent,
        }
    }

    fn expand_compact_model(model: CompactNnueModel) -> NnueEvaluatorData {
        let mut input_weights = vec![vec![0; NNUE_ACCUMULATOR_SIZE]; NNUE_INPUT_SIZE];
        for (feature_id, weights) in model.input_weights {
            input_weights[feature_id] = weights;
        }
        let towers = model
            .towers
            .into_iter()
            .map(|tower| {
                let mut input_size = NNUE_DENSE_INPUT_SIZE;
                let dense_layers = tower
                    .dense_layers
                    .into_iter()
                    .enumerate()
                    .map(|(idx, layer)| {
                        let output_size = NNUE_DENSE_LAYER_SIZES[idx];
                        let data = DenseLayerData {
                            input_size,
                            output_size,
                            weights: layer.weights,
                            bias: layer.bias,
                        };
                        input_size = output_size;
                        data
                    })
                    .collect();
                NnueTowerData {
                    dense_layers,
                    output_weights: tower.output_weights,
                    output_bias: tower.output_bias,
                }
            })
            .collect();

        NnueEvaluatorData {
            input_size: NNUE_INPUT_SIZE,
            acc_size: NNUE_ACCUMULATOR_SIZE,
            pattern_set: NNUE_DEFAULT_PATTERN_SET.to_string(),
            share_rotations: NNUE_DEFAULT_SHARE_ROTATIONS,
            accumulator_size: NNUE_ACCUMULATOR_SIZE,
            activation_scale: NNUE_ACTIVATION_SCALE,
            weight_scale: model.weight_scale,
            tower_count: NNUE_TOWER_COUNT,
            pairwise: true,
            input_weights,
            input_bias: model.input_bias,
            towers,
        }
    }

    #[test]
    fn active_features_match_python_scalar_fixture() {
        let fixture: ActiveFeatureFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/nnue_v2_active_features.json"
        ))
        .unwrap();

        assert_eq!(fixture.cases.len(), 10);
        for case in fixture.cases {
            let state = NnueState::from_board(&board_from_case(case.own, case.opponent));
            for view in 0..NNUE_VIEW_COUNT {
                let mut actual = state.active_features_for_view(view).to_vec();
                actual.sort_unstable();
                assert_eq!(
                    actual, case.active_features[view],
                    "active feature mismatch case={} view={}",
                    case.name, view
                );
            }
        }
    }

    #[test]
    fn evaluate_board_slow_matches_python_integer_reference_fixture() {
        let fixture: InferenceFixture =
            serde_json::from_str(include_str!("../../tests/fixtures/nnue_v2_inference.json"))
                .unwrap();
        let data = expand_compact_model(fixture.model);
        let engine_file = EngineFile {
            format_version: 3,
            metadata: Metadata {
                eval_name: "nnue-v2-fixture".to_string(),
                eval_version: "fixture".to_string(),
                ..Metadata::default()
            },
            evaluator: EvaluatorData::Nnue(data),
            mpc: MpcConfig::default(),
        };
        let (_, evaluator, _) =
            EngineFile::read_string(&serde_json::to_string(&engine_file).unwrap())
                .unwrap()
                .into_parts()
                .unwrap();

        assert!(fixture.cases.len() >= 20);
        for case in fixture.cases {
            let actual = evaluator.evaluate(&board_from_case(case.own, case.opponent));
            assert_eq!(actual, case.expected, "score mismatch case={}", case.name);
        }
    }
}
