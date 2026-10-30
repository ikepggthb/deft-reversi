use crate::board::pos::*;

/// `X` marks a square belonging to the pattern. Squares are read from A1 to H8.
pub const PATTERN_FEATURES: [&str; N_PATTERNS] = [
    concat!(
        "X.XXXX.X\n",
        "..XXXX..\n",
        "........\n",
        "........\n",
        "........\n",
        "........\n",
        "........\n",
        "........",
    ),
    concat!(
        "XXXXXXXX\n",
        ".X....X.\n",
        "........\n",
        "........\n",
        "........\n",
        "........\n",
        "........\n",
        "........",
    ),
    concat!(
        "X......X\n",
        "XXXXXXXX\n",
        "........\n",
        "........\n",
        "........\n",
        "........\n",
        "........\n",
        "........",
    ),
    concat!(
        "........\n",
        "........\n",
        "XXXXXXXX\n",
        "........\n",
        "........\n",
        "........\n",
        "........\n",
        "........",
    ),
    concat!(
        "........\n",
        "........\n",
        "........\n",
        "XXXXXXXX\n",
        "........\n",
        "........\n",
        "........\n",
        "........",
    ),
    concat!(
        "XXX.....\n",
        "XXX.....\n",
        "XXX.....\n",
        "........\n",
        "........\n",
        "........\n",
        "........\n",
        "........",
    ),
    concat!(
        "XXXX....\n",
        "XXX.....\n",
        "XX......\n",
        "X.......\n",
        "........\n",
        "........\n",
        "........\n",
        "........",
    ),
    concat!(
        "XX..X...\n",
        "XX.X....\n",
        "..X.....\n",
        ".X......\n",
        "X.......\n",
        "........\n",
        "........\n",
        "........",
    ),
    concat!(
        ".....X..\n",
        "....X...\n",
        "...X....\n",
        "..X.....\n",
        ".X......\n",
        "X.......\n",
        "........\n",
        "........",
    ),
    concat!(
        "......X.\n",
        ".....X..\n",
        "....X...\n",
        "...X....\n",
        "..X.....\n",
        ".X......\n",
        "X.......\n",
        "........",
    ),
    concat!(
        ".......X\n",
        "......X.\n",
        ".....X..\n",
        "....X...\n",
        "...X....\n",
        "..X.....\n",
        ".X......\n",
        "X.......",
    ),
];

pub const POW3: [usize; 11] = [1, 3, 9, 27, 81, 243, 729, 2187, 6561, 19683, 59049];

pub const MAX_PATTERN_SQUARE: usize = 10;
pub const N_ROTATIONS: usize = 4;
pub const N_PATTERNS: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoordToFeature {
    pub n_squares: u8,
    pub feature_coord: [[u8; MAX_PATTERN_SQUARE]; N_ROTATIONS],
}

pub const FEATURE_COORD: [CoordToFeature; N_PATTERNS] = build_coord_to_features();

/// Builds all pattern coordinates from their 8x8 text representations.
pub const fn build_coord_to_features() -> [CoordToFeature; N_PATTERNS] {
    let mut result = [CoordToFeature::EMPTY; N_PATTERNS];
    let mut pattern_idx = 0;

    while pattern_idx < N_PATTERNS {
        result[pattern_idx] = build_coord_to_feature(PATTERN_FEATURES[pattern_idx]);
        pattern_idx += 1;
    }

    result
}

/// Builds one pattern and its four rotations.
///
/// `X` is a pattern square, `.` is a non-pattern square, and ASCII whitespace
/// is ignored. Pattern squares retain their A1-to-H8 reading order in every
/// rotation so that their base-3 weights remain stable.
pub const fn build_coord_to_feature(pattern: &str) -> CoordToFeature {
    let bytes = pattern.as_bytes();
    let mut result = CoordToFeature::EMPTY;
    let mut byte_idx = 0;
    let mut square = 0;

    while byte_idx < bytes.len() {
        match bytes[byte_idx] {
            b'X' => {
                if square >= N_BOARD_SQUARES {
                    panic!("pattern contains more than 64 squares");
                }
                if result.n_squares as usize >= MAX_PATTERN_SQUARE {
                    panic!("pattern contains too many feature squares");
                }

                let feature_square_idx = result.n_squares as usize;
                let coord = square as u8;
                let mut rotation = 0;
                while rotation < N_ROTATIONS {
                    result.feature_coord[rotation][feature_square_idx] =
                        rotate_coord(coord, rotation);
                    rotation += 1;
                }

                result.n_squares += 1;
                square += 1;
            }
            b'.' => {
                if square >= N_BOARD_SQUARES {
                    panic!("pattern contains more than 64 squares");
                }
                square += 1;
            }
            b' ' | b'\n' | b'\r' | b'\t' => {}
            _ => panic!("pattern contains an invalid character"),
        }
        byte_idx += 1;
    }

    if square != N_BOARD_SQUARES {
        panic!("pattern must describe exactly 64 squares");
    }

    result
}

const fn rotate_coord(coord: u8, rotation: usize) -> u8 {
    let file = coord % 8;
    let rank = coord / 8;

    match rotation {
        0 => rank * 8 + file,
        1 => (7 - file) * 8 + rank,
        2 => (7 - rank) * 8 + (7 - file),
        3 => file * 8 + (7 - rank),
        _ => panic!("rotation must be between 0 and 3"),
    }
}

impl CoordToFeature {
    const EMPTY: Self = Self {
        n_squares: 0,
        feature_coord: [[NO_COORD; MAX_PATTERN_SQUARE]; N_ROTATIONS],
    };
}

// -----------------------------------------------------------
pub const MAX_SQUARE_FEATURES: usize = 11;
pub const N_BOARD_SQUARES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SquareFeature {
    pub feature_idx: u8,
    pub base3_weight: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// Per-square feature list for planned differential pattern updates.
pub struct SquareFeatures {
    pub len: u8,
    pub features: [SquareFeature; MAX_SQUARE_FEATURES],
}

impl SquareFeatures {
    // Empty sentinel for compile-time square feature map construction.
    pub const EMPTY: Self = Self {
        len: 0,
        features: [SquareFeature::EMPTY; MAX_SQUARE_FEATURES],
    };

    // Const builder for planned differential pattern update maps.
    pub const fn push(mut self, feature: SquareFeature) -> Self {
        self.features[self.len as usize] = feature;
        self.len += 1;
        self
    }
}

impl SquareFeature {
    // Empty sentinel for compile-time square feature map construction.
    pub const EMPTY: Self = Self {
        feature_idx: 0,
        base3_weight: 0,
    };
}

// pattern_idx ごとの 3 進 table サイズ。
pub const PATTERN_TABLE_SIZES: [usize; N_PATTERNS] = build_pattern_table_sizes();
pub const TOTAL_PATTERN_WEIGHTS: usize = build_total_pattern_weights();

// 各マスがどの feature に影響するかを compile time に展開した表。
// Compile-time map retained for planned differential pattern updates.
pub const SQUARE_TO_FEATURES: [SquareFeatures; N_BOARD_SQUARES] = build_square_to_features();

const fn build_pattern_table_sizes() -> [usize; N_PATTERNS] {
    let mut table = [0usize; N_PATTERNS];
    let mut i = 0;

    while i < N_PATTERNS {
        table[i] = POW3[FEATURE_COORD[i].n_squares as usize];
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
const fn build_square_to_features() -> [SquareFeatures; N_BOARD_SQUARES] {
    let mut table = [SquareFeatures::EMPTY; N_BOARD_SQUARES];
    let mut pattern_idx = 0;

    while pattern_idx < N_PATTERNS {
        let pattern = &FEATURE_COORD[pattern_idx];
        let mut rotation = 0;

        while rotation < N_ROTATIONS {
            let feature_idx = (pattern_idx * N_ROTATIONS + rotation) as u8;
            let mut pattern_square_idx = 0;

            while pattern_square_idx < pattern.n_squares {
                let square = pattern.feature_coord[rotation][pattern_square_idx as usize] as usize;
                let base3_weight =
                    POW3[pattern.n_squares as usize - 1 - pattern_square_idx as usize] as u16;
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

#[cfg(test)]
mod tests {
    use super::*;

    // The previous hand-written table is retained in tests to verify the generator.
    #[cfg(test)]
    const EXPECTED_FEATURE_COORD: [CoordToFeature; N_PATTERNS] = [
        // 1
        CoordToFeature {
            n_squares: 10,
            feature_coord: [
                [A1, C1, D1, E1, F1, H1, C2, D2, E2, F2],
                [A8, A6, A5, A4, A3, A1, B6, B5, B4, B3],
                [H8, F8, E8, D8, C8, A8, F7, E7, D7, C7],
                [H1, H3, H4, H5, H6, H8, G3, G4, G5, G6],
            ],
        },
        // 2
        CoordToFeature {
            n_squares: 10,
            feature_coord: [
                [A1, B1, C1, D1, E1, F1, G1, H1, B2, G2],
                [A8, A7, A6, A5, A4, A3, A2, A1, B7, B2],
                [H8, G8, F8, E8, D8, C8, B8, A8, G7, B7],
                [H1, H2, H3, H4, H5, H6, H7, H8, G2, G7],
            ],
        },
        // 3
        CoordToFeature {
            n_squares: 10,
            feature_coord: [
                [A1, H1, A2, B2, C2, D2, E2, F2, G2, H2],
                [A8, A1, B8, B7, B6, B5, B4, B3, B2, B1],
                [H8, A8, H7, G7, F7, E7, D7, C7, B7, A7],
                [H1, H8, G1, G2, G3, G4, G5, G6, G7, G8],
            ],
        },
        // 4
        CoordToFeature {
            n_squares: 8,
            feature_coord: [
                [A3, B3, C3, D3, E3, F3, G3, H3, NO_COORD, NO_COORD],
                [C8, C7, C6, C5, C4, C3, C2, C1, NO_COORD, NO_COORD],
                [H6, G6, F6, E6, D6, C6, B6, A6, NO_COORD, NO_COORD],
                [F1, F2, F3, F4, F5, F6, F7, F8, NO_COORD, NO_COORD],
            ],
        },
        // 5
        CoordToFeature {
            n_squares: 8,
            feature_coord: [
                [A4, B4, C4, D4, E4, F4, G4, H4, NO_COORD, NO_COORD],
                [D8, D7, D6, D5, D4, D3, D2, D1, NO_COORD, NO_COORD],
                [H5, G5, F5, E5, D5, C5, B5, A5, NO_COORD, NO_COORD],
                [E1, E2, E3, E4, E5, E6, E7, E8, NO_COORD, NO_COORD],
            ],
        },
        // 6
        CoordToFeature {
            n_squares: 9,
            feature_coord: [
                [A1, B1, C1, A2, B2, C2, A3, B3, C3, NO_COORD],
                [A8, A7, A6, B8, B7, B6, C8, C7, C6, NO_COORD],
                [H8, G8, F8, H7, G7, F7, H6, G6, F6, NO_COORD],
                [H1, H2, H3, G1, G2, G3, F1, F2, F3, NO_COORD],
            ],
        },
        // 7
        CoordToFeature {
            n_squares: 10,
            feature_coord: [
                [A1, B1, C1, D1, A2, B2, C2, A3, B3, A4],
                [A8, A7, A6, A5, B8, B7, B6, C8, C7, D8],
                [H8, G8, F8, E8, H7, G7, F7, H6, G6, H5],
                [H1, H2, H3, H4, G1, G2, G3, F1, F2, E1],
            ],
        },
        // 8
        CoordToFeature {
            n_squares: 9,
            feature_coord: [
                [A1, B1, E1, A2, B2, D2, C3, B4, A5, NO_COORD],
                [A8, A7, A4, B8, B7, B5, C6, D7, E8, NO_COORD],
                [H8, G8, D8, H7, G7, E7, F6, G5, H4, NO_COORD],
                [H1, H2, H5, G1, G2, G4, F3, E2, D1, NO_COORD],
            ],
        },
        // 9
        CoordToFeature {
            n_squares: 6,
            feature_coord: [
                [
                    F1, E2, D3, C4, B5, A6, NO_COORD, NO_COORD, NO_COORD, NO_COORD,
                ],
                [
                    A3, B4, C5, D6, E7, F8, NO_COORD, NO_COORD, NO_COORD, NO_COORD,
                ],
                [
                    C8, D7, E6, F5, G4, H3, NO_COORD, NO_COORD, NO_COORD, NO_COORD,
                ],
                [
                    H6, G5, F4, E3, D2, C1, NO_COORD, NO_COORD, NO_COORD, NO_COORD,
                ],
            ],
        },
        // 10
        CoordToFeature {
            n_squares: 7,
            feature_coord: [
                [G1, F2, E3, D4, C5, B6, A7, NO_COORD, NO_COORD, NO_COORD],
                [A2, B3, C4, D5, E6, F7, G8, NO_COORD, NO_COORD, NO_COORD],
                [B8, C7, D6, E5, F4, G3, H2, NO_COORD, NO_COORD, NO_COORD],
                [H7, G6, F5, E4, D3, C2, B1, NO_COORD, NO_COORD, NO_COORD],
            ],
        },
        // 11
        CoordToFeature {
            n_squares: 8,
            feature_coord: [
                [H1, G2, F3, E4, D5, C6, B7, A8, NO_COORD, NO_COORD],
                [A1, B2, C3, D4, E5, F6, G7, H8, NO_COORD, NO_COORD],
                [A8, B7, C6, D5, E4, F3, G2, H1, NO_COORD, NO_COORD],
                [H8, G7, F6, E5, D4, C3, B2, A1, NO_COORD, NO_COORD],
            ],
        },
    ];
    #[test]
    fn generated_coordinates_match_the_previous_hand_written_table() {
        assert_eq!(FEATURE_COORD, EXPECTED_FEATURE_COORD);
    }

    #[test]
    fn builds_all_patterns_with_the_expected_number_of_squares() {
        let expected = [10, 10, 10, 8, 8, 9, 10, 9, 6, 7, 8];

        for (pattern, expected) in FEATURE_COORD.iter().zip(expected) {
            assert_eq!(pattern.n_squares, expected);
        }
    }

    #[test]
    fn unused_coordinate_slots_contain_the_sentinel() {
        for pattern in FEATURE_COORD {
            for rotation in pattern.feature_coord {
                for &coord in &rotation[..pattern.n_squares as usize] {
                    assert!(coord < N_BOARD_SQUARES as u8);
                }
                for &coord in &rotation[pattern.n_squares as usize..] {
                    assert_eq!(coord, NO_COORD);
                }
            }
        }
    }
}
