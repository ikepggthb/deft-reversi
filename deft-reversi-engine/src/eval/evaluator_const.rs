use crate::board::constant::*;

pub const POW3: [usize; 11] = [1, 3, 9, 27, 81, 243, 729, 2187, 6561, 19683, 59049];

pub const MAX_PATTERN_SQUARES: usize = 10;
pub const N_ROTATIONS: usize = 4;
pub const N_PATTERNS: usize = 11;
pub const N_FEATURES: usize = N_PATTERNS * N_ROTATIONS;

pub const SCORE_SCALE: i32 = 128;
pub const SCORE_MAX: i32 = 64;

pub const N_MOBILITY_MAX: usize = 128;
pub const N_MOBILITY_BASE: usize = 64;
pub const N_PHASES: usize = 60;
pub const N_BOARD_SQUARES: usize = 64;
// Compile-time feature map capacity for planned differential pattern updates.
#[allow(dead_code)]
pub const MAX_SQUARE_FEATURES: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatternDefinition {
    pub n_squares: usize,
    pub squares: [[u8; MAX_PATTERN_SQUARES]; N_ROTATIONS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// One entry in the planned square-to-pattern differential update map.
#[allow(dead_code)]
pub struct SquareFeature {
    pub feature_idx: u8,
    pub base3_weight: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// Per-square feature list for planned differential pattern updates.
#[allow(dead_code)]
pub struct SquareFeatures {
    pub len: u8,
    pub features: [SquareFeature; MAX_SQUARE_FEATURES],
}

impl SquareFeature {
    // Empty sentinel for compile-time square feature map construction.
    #[allow(dead_code)]
    pub const EMPTY: Self = Self {
        feature_idx: 0,
        base3_weight: 0,
    };
}

impl SquareFeatures {
    // Empty sentinel for compile-time square feature map construction.
    #[allow(dead_code)]
    pub const EMPTY: Self = Self {
        len: 0,
        features: [SquareFeature::EMPTY; MAX_SQUARE_FEATURES],
    };

    // Const builder for planned differential pattern update maps.
    #[allow(dead_code)]
    pub const fn push(mut self, feature: SquareFeature) -> Self {
        self.features[self.len as usize] = feature;
        self.len += 1;
        self
    }
}

// 4回転ぶんの pattern 定義。
// 短い pattern は n_squares までを使い、残りは NO_COORD で埋める。
#[rustfmt::skip]
pub const PATTERN_DEFINITIONS: [PatternDefinition; N_PATTERNS] = [
    PatternDefinition {
        n_squares: 10,
        squares: [
            [A1, C1, D1, E1, F1, H1, C2, D2, E2, F2],
            [A8, A6, A5, A4, A3, A1, B6, B5, B4, B3],
            [H8, F8, E8, D8, C8, A8, F7, E7, D7, C7],
            [H1, H3, H4, H5, H6, H8, G3, G4, G5, G6],
        ],
    },
    PatternDefinition {
        n_squares: 10,
        squares: [
            [A1, B1, C1, D1, E1, F1, G1, H1, B2, G2],
            [A8, A7, A6, A5, A4, A3, A2, A1, B7, B2],
            [H8, G8, F8, E8, D8, C8, B8, A8, G7, B7],
            [H1, H2, H3, H4, H5, H6, H7, H8, G2, G7],
        ],
    },
    PatternDefinition {
        n_squares: 10,
        squares: [
            [A1, H1, A2, B2, C2, D2, E2, F2, G2, H2],
            [A8, A1, B8, B7, B6, B5, B4, B3, B2, B1],
            [H8, A8, H7, G7, F7, E7, D7, C7, B7, A7],
            [H1, H8, G1, G2, G3, G4, G5, G6, G7, G8],
        ],
    },
    PatternDefinition {
        n_squares: 8,
        squares: [
            [A3, B3, C3, D3, E3, F3, G3, H3, NO_COORD, NO_COORD],
            [C8, C7, C6, C5, C4, C3, C2, C1, NO_COORD, NO_COORD],
            [H6, G6, F6, E6, D6, C6, B6, A6, NO_COORD, NO_COORD],
            [F1, F2, F3, F4, F5, F6, F7, F8, NO_COORD, NO_COORD],
        ],
    },
    PatternDefinition {
        n_squares: 8,
        squares: [
            [A4, B4, C4, D4, E4, F4, G4, H4, NO_COORD, NO_COORD],
            [D8, D7, D6, D5, D4, D3, D2, D1, NO_COORD, NO_COORD],
            [H5, G5, F5, E5, D5, C5, B5, A5, NO_COORD, NO_COORD],
            [E1, E2, E3, E4, E5, E6, E7, E8, NO_COORD, NO_COORD],
        ],
    },
    PatternDefinition {
        n_squares: 9,
        squares: [
            [A1, B1, C1, A2, B2, C2, A3, B3, C3, NO_COORD],
            [A8, A7, A6, B8, B7, B6, C8, C7, C6, NO_COORD],
            [H8, G8, F8, H7, G7, F7, H6, G6, F6, NO_COORD],
            [H1, H2, H3, G1, G2, G3, F1, F2, F3, NO_COORD],
        ],
    },
    PatternDefinition {
        n_squares: 10,
        squares: [
            [A1, B1, C1, D1, A2, B2, C2, A3, B3, A4],
            [A8, A7, A6, A5, B8, B7, B6, C8, C7, D8],
            [H8, G8, F8, E8, H7, G7, F7, H6, G6, H5],
            [H1, H2, H3, H4, G1, G2, G3, F1, F2, E1],
        ],
    },
    PatternDefinition {
        n_squares: 9,
        squares: [
            [A1, B1, E1, A2, B2, D2, C3, B4, A5, NO_COORD],
            [A8, A7, A4, B8, B7, B5, C6, D7, E8, NO_COORD],
            [H8, G8, D8, H7, G7, E7, F6, G5, H4, NO_COORD],
            [H1, H2, H5, G1, G2, G4, F3, E2, D1, NO_COORD],
        ],
    },
    PatternDefinition {
        n_squares: 6,
        squares: [
            [F1, E2, D3, C4, B5, A6, NO_COORD, NO_COORD, NO_COORD, NO_COORD],
            [A3, B4, C5, D6, E7, F8, NO_COORD, NO_COORD, NO_COORD, NO_COORD],
            [C8, D7, E6, F5, G4, H3, NO_COORD, NO_COORD, NO_COORD, NO_COORD],
            [H6, G5, F4, E3, D2, C1, NO_COORD, NO_COORD, NO_COORD, NO_COORD],
        ],
    },
    PatternDefinition {
        n_squares: 7,
        squares: [
            [G1, F2, E3, D4, C5, B6, A7, NO_COORD, NO_COORD, NO_COORD],
            [A2, B3, C4, D5, E6, F7, G8, NO_COORD, NO_COORD, NO_COORD],
            [B8, C7, D6, E5, F4, G3, H2, NO_COORD, NO_COORD, NO_COORD],
            [H7, G6, F5, E4, D3, C2, B1, NO_COORD, NO_COORD, NO_COORD],
        ],
    },
    PatternDefinition {
        n_squares: 8,
        squares: [
            [H1, G2, F3, E4, D5, C6, B7, A8, NO_COORD, NO_COORD],
            [A1, B2, C3, D4, E5, F6, G7, H8, NO_COORD, NO_COORD],
            [A8, B7, C6, D5, E4, F3, G2, H1, NO_COORD, NO_COORD],
            [H8, G7, F6, E5, D4, C3, B2, A1, NO_COORD, NO_COORD],
        ],
    },
];

// pattern_idx ごとの 3 進 table サイズ。
pub const PATTERN_TABLE_SIZES: [usize; N_PATTERNS] = build_pattern_table_sizes();
pub const TOTAL_PATTERN_WEIGHTS: usize = build_total_pattern_weights();

// 各マスがどの feature に影響するかを compile time に展開した表。
// Compile-time map retained for planned differential pattern updates.
#[allow(dead_code)]
pub const SQUARE_TO_FEATURES: [SquareFeatures; N_BOARD_SQUARES] = build_square_to_features();

const fn build_pattern_table_sizes() -> [usize; N_PATTERNS] {
    let mut table = [0usize; N_PATTERNS];
    let mut i = 0;

    while i < N_PATTERNS {
        table[i] = POW3[PATTERN_DEFINITIONS[i].n_squares];
        i += 1;
    }

    table
}

const fn build_total_pattern_weights() -> usize {
    let mut total = 0;
    let mut i = 0;

    while i < N_PATTERNS {
        total += PATTERN_TABLE_SIZES[i];
        i += 1;
    }

    total
}

// Builder retained with SQUARE_TO_FEATURES for differential update work.
#[allow(dead_code)]
const fn build_square_to_features() -> [SquareFeatures; N_BOARD_SQUARES] {
    let mut table = [SquareFeatures::EMPTY; N_BOARD_SQUARES];
    let mut pattern_idx = 0;

    while pattern_idx < N_PATTERNS {
        let pattern = &PATTERN_DEFINITIONS[pattern_idx];
        let mut rotation = 0;

        while rotation < N_ROTATIONS {
            let feature_idx = (pattern_idx * N_ROTATIONS + rotation) as u8;
            let mut pattern_square_idx = 0;

            while pattern_square_idx < pattern.n_squares {
                let square = pattern.squares[rotation][pattern_square_idx] as usize;
                let base3_weight = POW3[pattern.n_squares - 1 - pattern_square_idx] as u16;
                table[square] = table[square].push(SquareFeature {
                    feature_idx,
                    base3_weight,
                });
                pattern_square_idx += 1;
            }

            rotation += 1;
        }

        pattern_idx += 1;
    }

    table
}
