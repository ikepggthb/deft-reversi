use crate::board::constant::*;

use super::evaluator_const::{N_BOARD_SQUARES, POW3};

pub const NNUE_ACCUMULATOR_SIZE: usize = 256;
pub const NNUE_ACTIVATION_SCALE: i32 = 4096;
pub const NNUE_ACTIVATION_MAX: i32 = NNUE_ACTIVATION_SCALE - 1;
pub const NNUE_DEFAULT_WEIGHT_SCALE: i32 = 4096;
pub const NNUE_DENSE_LAYER_SIZES: [usize; 2] = [32, 32];

pub const NNUE_STONE_FEATURES: usize = N_BOARD_SQUARES * 2;
pub const NNUE_LEGAL_MOVE_FEATURES: usize = N_BOARD_SQUARES * 2;
pub const NNUE_QUADRANT_COUNT: usize = 4;
pub const NNUE_QUADRANT_BUCKETS: usize = 17;
pub const NNUE_QUADRANT_FEATURES: usize = NNUE_QUADRANT_COUNT * NNUE_QUADRANT_BUCKETS;
pub const NNUE_QUADRANT_PARITY_BUCKETS: usize = 2;
pub const NNUE_QUADRANT_PARITY_FEATURES: usize = NNUE_QUADRANT_COUNT * NNUE_QUADRANT_PARITY_BUCKETS;
pub const NNUE_GLOBAL_PARITY_FEATURES: usize = 2;
pub const NNUE_TOWER_COUNT: usize = 8;
pub const NNUE_PAIRWISE_SIZE_PER_VIEW: usize = NNUE_ACCUMULATOR_SIZE / 2;
pub const NNUE_DENSE_INPUT_SIZE: usize = NNUE_PAIRWISE_SIZE_PER_VIEW * 2;

pub const NNUE_STONE_OFFSET: usize = 0;
pub const NNUE_LEGAL_MOVE_OFFSET: usize = NNUE_STONE_OFFSET + NNUE_STONE_FEATURES;
pub const NNUE_QUADRANT_OFFSET: usize = NNUE_LEGAL_MOVE_OFFSET + NNUE_LEGAL_MOVE_FEATURES;
pub const NNUE_QUADRANT_PARITY_OFFSET: usize = NNUE_QUADRANT_OFFSET + NNUE_QUADRANT_FEATURES;
pub const NNUE_GLOBAL_PARITY_OFFSET: usize =
    NNUE_QUADRANT_PARITY_OFFSET + NNUE_QUADRANT_PARITY_FEATURES;
pub const NNUE_PATTERN_OFFSET: usize = NNUE_GLOBAL_PARITY_OFFSET + NNUE_GLOBAL_PARITY_FEATURES;

pub const NNUE_MAX_PATTERN_SQUARES: usize = 10;
pub const NNUE_PATTERN_INSTANCE_COUNT: usize = 54;
pub const NNUE_PATTERN_FEATURES: usize = build_total_pattern_features();
pub const NNUE_INPUT_SIZE: usize = NNUE_PATTERN_OFFSET + NNUE_PATTERN_FEATURES;
// Exposed for NNUE training/export shape validation.
#[allow(dead_code)]
pub const NNUE_MAX_ACTIVE_FEATURES_PER_VIEW: usize =
    192 + NNUE_QUADRANT_COUNT + NNUE_QUADRANT_COUNT + 1 + NNUE_PATTERN_INSTANCE_COUNT;

pub const NNUE_DEFAULT_PATTERN_SET: &str = "v2";
pub const NNUE_DEFAULT_SHARE_ROTATIONS: bool = false;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NnuePatternInstance {
    pub n_squares: usize,
    pub squares: [u8; NNUE_MAX_PATTERN_SQUARES],
    pub pattern_type: usize,
}

impl NnuePatternInstance {
    pub const fn new(n_squares: usize, squares: [u8; NNUE_MAX_PATTERN_SQUARES]) -> Self {
        Self {
            n_squares,
            squares,
            pattern_type: usize::MAX,
        }
    }

    pub const fn with_type(
        n_squares: usize,
        squares: [u8; NNUE_MAX_PATTERN_SQUARES],
        pattern_type: usize,
    ) -> Self {
        Self {
            n_squares,
            squares,
            pattern_type,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NnueFeatureLayout {
    pub pattern_set: String,
    pub share_rotations: bool,
    pub pattern_instances: Vec<NnuePatternInstance>,
    pub pattern_instance_offsets: Vec<usize>,
    pub pattern_features: usize,
    pub input_size: usize,
    pub max_active_features_per_view: usize,
}

impl NnueFeatureLayout {
    pub fn new(pattern_set: &str, share_rotations: bool) -> Result<Self, String> {
        let pattern_instances = pattern_instances_for_set(pattern_set)?;
        let mut pattern_instance_offsets = Vec::with_capacity(pattern_instances.len());
        let mut type_offsets = std::collections::HashMap::new();
        let mut pattern_offset = 0usize;

        for instance in &pattern_instances {
            if share_rotations {
                if let Some(&offset) = type_offsets.get(&instance.pattern_type) {
                    pattern_instance_offsets.push(offset);
                } else {
                    type_offsets.insert(instance.pattern_type, pattern_offset);
                    pattern_instance_offsets.push(pattern_offset);
                    pattern_offset += POW3[instance.n_squares];
                }
            } else {
                pattern_instance_offsets.push(pattern_offset);
                pattern_offset += POW3[instance.n_squares];
            }
        }

        let max_active_features_per_view =
            192 + NNUE_QUADRANT_COUNT + NNUE_QUADRANT_COUNT + 1 + pattern_instances.len();

        Ok(Self {
            pattern_set: pattern_set.to_string(),
            share_rotations,
            pattern_instances,
            pattern_instance_offsets,
            pattern_features: pattern_offset,
            input_size: NNUE_PATTERN_OFFSET + pattern_offset,
            max_active_features_per_view,
        })
    }

    pub fn default_v2() -> Self {
        Self::new(NNUE_DEFAULT_PATTERN_SET, NNUE_DEFAULT_SHARE_ROTATIONS).unwrap()
    }
}

#[rustfmt::skip]
pub const NNUE_PATTERN_INSTANCES: [NnuePatternInstance; NNUE_PATTERN_INSTANCE_COUNT] = [
    rot8([C1, D1, E1, F1, C2, D2, E2, F2, NO_COORD, NO_COORD], 0),
    rot8([C1, D1, E1, F1, C2, D2, E2, F2, NO_COORD, NO_COORD], 1),
    rot8([C1, D1, E1, F1, C2, D2, E2, F2, NO_COORD, NO_COORD], 2),
    rot8([C1, D1, E1, F1, C2, D2, E2, F2, NO_COORD, NO_COORD], 3),

    rot8([A1, B1, C1, D1, E1, F1, G1, H1, NO_COORD, NO_COORD], 0),
    rot8([A1, B1, C1, D1, E1, F1, G1, H1, NO_COORD, NO_COORD], 1),
    rot8([A1, B1, C1, D1, E1, F1, G1, H1, NO_COORD, NO_COORD], 2),
    rot8([A1, B1, C1, D1, E1, F1, G1, H1, NO_COORD, NO_COORD], 3),

    rot8([A2, B2, C2, D2, E2, F2, G2, H2, NO_COORD, NO_COORD], 0),
    rot8([A2, B2, C2, D2, E2, F2, G2, H2, NO_COORD, NO_COORD], 1),
    rot8([A2, B2, C2, D2, E2, F2, G2, H2, NO_COORD, NO_COORD], 2),
    rot8([A2, B2, C2, D2, E2, F2, G2, H2, NO_COORD, NO_COORD], 3),

    rot8([A3, B3, C3, D3, E3, F3, G3, H3, NO_COORD, NO_COORD], 0),
    rot8([A3, B3, C3, D3, E3, F3, G3, H3, NO_COORD, NO_COORD], 1),
    rot8([A3, B3, C3, D3, E3, F3, G3, H3, NO_COORD, NO_COORD], 2),
    rot8([A3, B3, C3, D3, E3, F3, G3, H3, NO_COORD, NO_COORD], 3),

    rot8([A4, B4, C4, D4, E4, F4, G4, H4, NO_COORD, NO_COORD], 0),
    rot8([A4, B4, C4, D4, E4, F4, G4, H4, NO_COORD, NO_COORD], 1),
    rot8([A4, B4, C4, D4, E4, F4, G4, H4, NO_COORD, NO_COORD], 2),
    rot8([A4, B4, C4, D4, E4, F4, G4, H4, NO_COORD, NO_COORD], 3),

    rot9([A1, B1, C1, A2, B2, C2, A3, B3, C3, NO_COORD], 0),
    rot9([A1, B1, C1, A2, B2, C2, A3, B3, C3, NO_COORD], 1),
    rot9([A1, B1, C1, A2, B2, C2, A3, B3, C3, NO_COORD], 2),
    rot9([A1, B1, C1, A2, B2, C2, A3, B3, C3, NO_COORD], 3),

    rot4([D1, C2, B3, A4, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 0),
    rot4([D1, C2, B3, A4, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 1),
    rot4([D1, C2, B3, A4, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 2),
    rot4([D1, C2, B3, A4, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 3),

    rot5([E1, D2, C3, B4, A5, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 0),
    rot5([E1, D2, C3, B4, A5, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 1),
    rot5([E1, D2, C3, B4, A5, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 2),
    rot5([E1, D2, C3, B4, A5, NO_COORD, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 3),

    rot6([F1, E2, D3, C4, B5, A6, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 0),
    rot6([F1, E2, D3, C4, B5, A6, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 1),
    rot6([F1, E2, D3, C4, B5, A6, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 2),
    rot6([F1, E2, D3, C4, B5, A6, NO_COORD, NO_COORD, NO_COORD, NO_COORD], 3),

    rot7([G1, F2, E3, D4, C5, B6, A7, NO_COORD, NO_COORD, NO_COORD], 0),
    rot7([G1, F2, E3, D4, C5, B6, A7, NO_COORD, NO_COORD, NO_COORD], 1),
    rot7([G1, F2, E3, D4, C5, B6, A7, NO_COORD, NO_COORD, NO_COORD], 2),
    rot7([G1, F2, E3, D4, C5, B6, A7, NO_COORD, NO_COORD, NO_COORD], 3),

    NnuePatternInstance::new(8, [A1, B2, C3, D4, E5, F6, G7, H8, NO_COORD, NO_COORD]),
    NnuePatternInstance::new(8, [H1, G2, F3, E4, D5, C6, B7, A8, NO_COORD, NO_COORD]),

    rot10([A1, B1, C1, D1, E1, F1, G1, H1, B2, G2], 0),
    rot10([A1, B1, C1, D1, E1, F1, G1, H1, B2, G2], 1),
    rot10([A1, B1, C1, D1, E1, F1, G1, H1, B2, G2], 2),
    rot10([A1, B1, C1, D1, E1, F1, G1, H1, B2, G2], 3),

    rot10([A1, B1, C1, D1, E1, A2, B2, C2, D2, E2], 0),
    rot10([A1, B1, C1, D1, E1, A2, B2, C2, D2, E2], 1),
    rot10([A1, B1, C1, D1, E1, A2, B2, C2, D2, E2], 2),
    rot10([A1, B1, C1, D1, E1, A2, B2, C2, D2, E2], 3),

    rot10([A1, A2, A3, A4, A5, B1, B2, B3, B4, B5], 0),
    rot10([A1, A2, A3, A4, A5, B1, B2, B3, B4, B5], 1),
    rot10([A1, A2, A3, A4, A5, B1, B2, B3, B4, B5], 2),
    rot10([A1, A2, A3, A4, A5, B1, B2, B3, B4, B5], 3),
];

// Exposed for NNUE training/export feature layout validation.
#[allow(dead_code)]
pub const NNUE_PATTERN_INSTANCE_OFFSETS: [usize; NNUE_PATTERN_INSTANCE_COUNT] =
    build_pattern_instance_offsets();

#[rustfmt::skip]
pub const NNUE_QUADRANT_MASKS: [u64; NNUE_QUADRANT_COUNT] = [
    quadrant_mask(0, 0),
    quadrant_mask(4, 0),
    quadrant_mask(0, 4),
    quadrant_mask(4, 4),
];

const fn rot4(squares: [u8; NNUE_MAX_PATTERN_SQUARES], turns: usize) -> NnuePatternInstance {
    rotated_instance(4, squares, turns)
}

const fn rot5(squares: [u8; NNUE_MAX_PATTERN_SQUARES], turns: usize) -> NnuePatternInstance {
    rotated_instance(5, squares, turns)
}

const fn rot6(squares: [u8; NNUE_MAX_PATTERN_SQUARES], turns: usize) -> NnuePatternInstance {
    rotated_instance(6, squares, turns)
}

const fn rot7(squares: [u8; NNUE_MAX_PATTERN_SQUARES], turns: usize) -> NnuePatternInstance {
    rotated_instance(7, squares, turns)
}

const fn rot8(squares: [u8; NNUE_MAX_PATTERN_SQUARES], turns: usize) -> NnuePatternInstance {
    rotated_instance(8, squares, turns)
}

const fn rot9(squares: [u8; NNUE_MAX_PATTERN_SQUARES], turns: usize) -> NnuePatternInstance {
    rotated_instance(9, squares, turns)
}

const fn rot10(squares: [u8; NNUE_MAX_PATTERN_SQUARES], turns: usize) -> NnuePatternInstance {
    rotated_instance(10, squares, turns)
}

const fn rotated_instance(
    n_squares: usize,
    squares: [u8; NNUE_MAX_PATTERN_SQUARES],
    turns: usize,
) -> NnuePatternInstance {
    let mut rotated = [NO_COORD; NNUE_MAX_PATTERN_SQUARES];
    let mut i = 0;
    while i < n_squares {
        rotated[i] = rotate_square(squares[i], turns);
        i += 1;
    }
    NnuePatternInstance::new(n_squares, rotated)
}

const fn rotate_square(square: u8, turns: usize) -> u8 {
    let mut result = square;
    let mut i = 0;
    while i < turns {
        let x = result % 8;
        let y = result / 8;
        result = x * 8 + (7 - y);
        i += 1;
    }
    result
}

const fn build_total_pattern_features() -> usize {
    let mut total = 0;
    let mut i = 0;
    while i < NNUE_PATTERN_INSTANCE_COUNT {
        total += POW3[NNUE_PATTERN_INSTANCES[i].n_squares];
        i += 1;
    }
    total
}

// Kept with the exported offsets constant for NNUE training/export layout checks.
#[allow(dead_code)]
const fn build_pattern_instance_offsets() -> [usize; NNUE_PATTERN_INSTANCE_COUNT] {
    let mut offsets = [0usize; NNUE_PATTERN_INSTANCE_COUNT];
    let mut offset = 0;
    let mut i = 0;
    while i < NNUE_PATTERN_INSTANCE_COUNT {
        offsets[i] = offset;
        offset += POW3[NNUE_PATTERN_INSTANCES[i].n_squares];
        i += 1;
    }
    offsets
}

const fn quadrant_mask(x0: u8, y0: u8) -> u64 {
    let mut mask = 0u64;
    let mut y = 0;
    while y < 4 {
        let mut x = 0;
        while x < 4 {
            let square = (y0 + y) * 8 + x0 + x;
            mask |= 1u64 << square;
            x += 1;
        }
        y += 1;
    }
    mask
}

fn pattern_instances_for_set(pattern_set: &str) -> Result<Vec<NnuePatternInstance>, String> {
    let mut instances = Vec::new();
    match pattern_set {
        "v1" => {
            for (idx, instance) in NNUE_PATTERN_INSTANCES.iter().take(42).enumerate() {
                instances.push(with_pattern_type(*instance, pattern_type_for_v2_index(idx)));
            }
        }
        "no-corner2x5" => {
            for (idx, instance) in NNUE_PATTERN_INSTANCES.iter().take(46).enumerate() {
                instances.push(with_pattern_type(*instance, pattern_type_for_v2_index(idx)));
            }
        }
        "v2" => {
            for (idx, instance) in NNUE_PATTERN_INSTANCES.iter().enumerate() {
                instances.push(with_pattern_type(*instance, pattern_type_for_v2_index(idx)));
            }
        }
        _ => return Err(format!("unsupported pattern_set: {pattern_set}")),
    }
    Ok(instances)
}

fn with_pattern_type(instance: NnuePatternInstance, pattern_type: usize) -> NnuePatternInstance {
    NnuePatternInstance::with_type(instance.n_squares, instance.squares, pattern_type)
}

fn pattern_type_for_v2_index(idx: usize) -> usize {
    match idx {
        0..=3 => 0,
        4..=7 => 1,
        8..=11 => 2,
        12..=15 => 3,
        16..=19 => 4,
        20..=23 => 5,
        24..=27 => 6,
        28..=31 => 7,
        32..=35 => 8,
        36..=39 => 9,
        40..=41 => 10,
        42..=45 => 11,
        46..=49 => 12,
        50..=53 => 13,
        _ => unreachable!("invalid NNUE pattern index"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nnue_feature_dimensions_match_spec() {
        assert_eq!(NNUE_PATTERN_INSTANCE_COUNT, 54);
        assert_eq!(NNUE_PATTERN_FEATURES, 944_622);
        assert_eq!(NNUE_INPUT_SIZE, 944_956);
        assert_eq!(NNUE_MAX_ACTIVE_FEATURES_PER_VIEW, 255);

        let s2 = NnueFeatureLayout::new("v2", true).unwrap();
        assert_eq!(s2.pattern_instances.len(), 54);
        assert_eq!(s2.pattern_features, 239_436);
        assert_eq!(s2.input_size, 239_770);
        assert_eq!(s2.max_active_features_per_view, 255);
    }

    #[test]
    fn pattern_instance_offsets_cover_pattern_range() {
        let last = NNUE_PATTERN_INSTANCE_COUNT - 1;
        assert_eq!(
            NNUE_PATTERN_INSTANCE_OFFSETS[last] + POW3[NNUE_PATTERN_INSTANCES[last].n_squares],
            NNUE_PATTERN_FEATURES
        );
    }

    #[test]
    fn quadrant_masks_cover_board_once() {
        let mut union = 0u64;
        for &mask in &NNUE_QUADRANT_MASKS {
            assert_eq!(mask.count_ones(), 16);
            assert_eq!(union & mask, 0);
            union |= mask;
        }
        assert_eq!(union, u64::MAX);
    }
}
