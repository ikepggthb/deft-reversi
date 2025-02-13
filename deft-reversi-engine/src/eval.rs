/*

-----------------------
Pattern 1:
 
X.XXXX.X
..XXXX..
........
........
........
........
........
........

------------------
Pattern 2:
 
XXXXXXXX
.X....X.
........
........
........
........
........
........

-----------------------
Pattern 3:
 
X......X
XXXXXXXX
........
........
........
........
........
........

-----------------------
Pattern 4:
 
........
........
XXXXXXXX
........
........
........
........
........


-----------------------
Pattern 5:
 
........
........
........
XXXXXXXX
........
........
........
........

-----------------------
Pattern 6:
 
XXX.....
XXX.....
XXX.....
........
........
........
........
........

-------------------------
Pattern 7:
 
XXXX....
XXX.....
XX......
X.......
........
........
........
........

-------------------------
Pattern 8:
 
XX..X...
XX.X....
..X.....
.X......
X.......
........
........
........

-------------------------
Pattern 9:
 
.....X..
....X...
...X....
..X.....
.X......
X.......
........
........

-------------------------
Pattern 10:
 
......X.
.....X..
....X...
...X....
..X.....
.X......
X.......
........

-------------------------
Pattern 11:
 
.......X
......X.
.....X..
....X...
...X....
..X.....
.X......
X.......


2

3*2 + 1

3^2*2 + 3 * 1

...


-------------------------
*/

use std::fs::File;
use std::fs;
use serde::{Deserialize, Serialize};
use std::io::*;


use crate::board::*;

pub mod evaluator_const {
    use crate::board::*;
    pub const P3: [i32; 11] = [1, 3, 9, 27, 81, 243, 729, 2187, 6561, 19683, 59049];

    pub const MAX_PATTERN_SQUARE: usize = 10;
    pub const N_ROTATION: usize = 4;
    pub const N_PATTERN: usize = 11;
    pub const N_FEAUTURE: usize = N_PATTERN * 4;

    pub const SCORE_RATE: i32 = 128;
    pub const SCORE_MAX : i32 = 64;

    pub struct CoordToFeature {
        pub n_square: u8,
        pub feature_coord: [[u8;MAX_PATTERN_SQUARE];N_ROTATION]
    }

    pub const FEATURE_COORD:  [CoordToFeature;N_PATTERN] = 
        [
            // 1
            CoordToFeature {
                n_square: 10,
                feature_coord:[ 
                    [A1, C1, D1, E1, F1, H1, C2, D2, E2, F2],
                    [A8, A6, A5, A4, A3, A1, B6, B5, B4, B3],
                    [H8, F8, E8, D8, C8, A8, F7, E7, D7, C7],
                    [H1, H3, H4, H5, H6, H8, G3, G4, G5, G6]
                ],
            },
            // 2
            CoordToFeature {
                n_square: 10,
                feature_coord:[
                    [A1, B1, C1, D1, E1, F1, G1, H1, B2, G2],
                    [A8 ,A7, A6, A5, A4, A3, A2, A1, B7, B2], 
                    [H8, G8, F8, E8, D8, C8, B8, A8, G7, B7],
                    [H1, H2, H3, H4, H5, H6, H7, H8, G2, G7]
                ],
            },
            // 3
            CoordToFeature {
                n_square: 10,
                feature_coord:[
                    [A1, H1, A2, B2, C2, D2, E2, F2, G2, H2],
                    [A8, A1, B8, B7, B6, B5, B4, B3, B2, B1],
                    [H8, A8, H7, G7, F7, E7, D7, C7, B7, A7],
                    [H1, H8, G1, G2, G3, G4, G5, G6, G7, G8]
                ]
            },
            // 4
            CoordToFeature {
                n_square: 8,
                feature_coord:[
                    [A3, B3, C3, D3, E3, F3, G3, H3, NO_COORD, NO_COORD],
                    [C8, C7, C6, C5, C4, C3, C2, C1, NO_COORD, NO_COORD],
                    [H6, G6, F6, E6, D6, C6, B6, A6, NO_COORD, NO_COORD],
                    [F1, F2, F3, F4, F5, F6, F7, F8, NO_COORD, NO_COORD]
                ]
            },
            // 5
            CoordToFeature {
                n_square: 8,
                feature_coord:[ 
                    [A4, B4, C4, D4, E4, F4, G4, H4, NO_COORD, NO_COORD],
                    [D8, D7, D6, D5, D4, D3, D2, D1, NO_COORD, NO_COORD],
                    [H5, G5, F5, E5, D5, C5, B5, A5, NO_COORD, NO_COORD],
                    [E1, E2, E3, E4, E5, E6, E7, E8, NO_COORD, NO_COORD]
                ]
            },
            // 6
            CoordToFeature {
                n_square: 9,
                feature_coord:[
                    [A1, B1, C1, A2, B2, C2, A3, B3, C3, NO_COORD],
                    [A8, A7, A6, B8, B7, B6, C8, C7, C6, NO_COORD],
                    [H8, G8, F8, H7, G7, F7, H6, G6, F6, NO_COORD],
                    [H1, H2, H3, G1, G2, G3, F1, F2, F3, NO_COORD]
                ]
            },
            // 7
            CoordToFeature {
                n_square: 10,
                feature_coord:[
                    [A1, B1, C1, D1, A2, B2, C2, A3, B3, A4],
                    [A8, A7, A6, A5, B8, B7, B6, C8, C7, D8],
                    [H8, G8, F8, E8, H7, G7, F7, H6, G6, H5],
                    [H1, H2, H3, H4, G1, G2, G3, F1, F2, E1]
                ]
            },
            // 8
            CoordToFeature {
                n_square: 9,
                feature_coord:[
                    [A1, B1, E1, A2, B2, D2, C3, B4, A5, NO_COORD],
                    [A8, A7, A4, B8, B7, B5, C6, D7, E8, NO_COORD],
                    [H8, G8, D8, H7, G7, E7, F6, G5, H4, NO_COORD],
                    [H1, H2, H5, G1, G2, G4, F3, E2, D1, NO_COORD]
                ]
            },
            // 9
            CoordToFeature {
                n_square: 6,
                feature_coord:[
                    [F1, E2, D3, C4, B5, A6, NO_COORD, NO_COORD, NO_COORD, NO_COORD],
                    [A3, B4, C5, D6, E7, F8, NO_COORD, NO_COORD, NO_COORD, NO_COORD],
                    [C8, D7, E6, F5, G4, H3, NO_COORD, NO_COORD, NO_COORD, NO_COORD],
                    [H6, G5, F4, E3, D2, C1, NO_COORD, NO_COORD, NO_COORD, NO_COORD]
                ]
            },
            // 10
            CoordToFeature {
                n_square: 7,
                feature_coord:[
                    [G1, F2, E3, D4, C5, B6, A7, NO_COORD, NO_COORD, NO_COORD],
                    [A2, B3, C4, D5, E6, F7, G8, NO_COORD, NO_COORD, NO_COORD],
                    [B8, C7, D6, E5, F4, G3, H2, NO_COORD, NO_COORD, NO_COORD],
                    [H7, G6, F5, E4, D3, C2, B1, NO_COORD, NO_COORD, NO_COORD]
                ]
            },
            // 11
            CoordToFeature {
                n_square: 8,
                feature_coord:[
                    [H1, G2, F3, E4, D5, C6, B7, A8, NO_COORD, NO_COORD],
                    [A1, B2, C3, D4, E5, F6, G7, H8, NO_COORD, NO_COORD],
                    [A8, B7, C6, D5, E4, F3, G2, H1, NO_COORD, NO_COORD],
                    [H8, G7, F6, E5, D4, C3, B2, A1, NO_COORD, NO_COORD]
                ]
            }
        ];

    pub const N_FEATURE_POSITIONS: [usize; N_PATTERN] = [
        P3[FEATURE_COORD[0].n_square as usize] as usize,
        P3[FEATURE_COORD[2].n_square as usize] as usize,
        P3[FEATURE_COORD[1].n_square as usize] as usize,
        P3[FEATURE_COORD[3].n_square as usize] as usize,
        P3[FEATURE_COORD[4].n_square as usize] as usize,
        P3[FEATURE_COORD[5].n_square as usize] as usize,
        P3[FEATURE_COORD[6].n_square as usize] as usize,
        P3[FEATURE_COORD[7].n_square as usize] as usize,
        P3[FEATURE_COORD[8].n_square as usize] as usize,
        P3[FEATURE_COORD[9].n_square as usize] as usize,
        P3[FEATURE_COORD[10].n_square as usize] as usize,
        ];
    
    pub const N_FEATURE_MAX: usize = P3[10] as usize;
    pub const N_MOBILITY_MAX: usize = 128;
    pub const N_MOBILITY_BASE: usize = 64;
    pub const N_PHASE: usize = 31;
    pub const SCORE_INF: i32 = i8::MAX as i32;
    
}

use evaluator_const::*;

#[derive(Serialize, Deserialize, Clone)]
pub struct EvaluationScores {
    pub pattern_eval: Vec<Vec<i16>>,
    pub mobility_eval: Vec<i16>,
    pub const_eval: i16
}

#[derive(Serialize, Deserialize)]
pub struct Evaluator {
    pub version: String,
    pub n_deta_set: i32,
    pub n_iteration: i32,
    pub eval: Vec<Vec<EvaluationScores>>,
    #[serde(skip)]
    pub feature_bit: [[u16; N_ROTATION]; N_PATTERN],
}


impl Default for EvaluationScores {
    fn default() -> Self {        
        Self{
            pattern_eval: vec![
                vec![0;N_FEATURE_POSITIONS[0]],
                vec![0;N_FEATURE_POSITIONS[1]],
                vec![0;N_FEATURE_POSITIONS[2]],
                vec![0;N_FEATURE_POSITIONS[3]],
                vec![0;N_FEATURE_POSITIONS[4]],
                vec![0;N_FEATURE_POSITIONS[5]],
                vec![0;N_FEATURE_POSITIONS[6]],
                vec![0;N_FEATURE_POSITIONS[7]],
                vec![0;N_FEATURE_POSITIONS[8]],
                vec![0;N_FEATURE_POSITIONS[9]],
                vec![0;N_FEATURE_POSITIONS[10]]
                ],
            mobility_eval: vec![0; N_MOBILITY_MAX],
            const_eval: 0,
        } 
    }
}

impl Default for Evaluator {
    fn default() -> Self {        
        Self{
            version: "0".to_string(),
            n_deta_set: 0,
            n_iteration: 0,
            eval: vec![vec![EvaluationScores::default();N_PHASE]; 2],
            feature_bit: [[0; N_ROTATION]; N_PATTERN],
        } 
    }
}



impl Evaluator {
    pub fn new() -> Self
    {
        Self::default()
    }


    #[inline(always)]
    pub fn clac_features(&mut self, board: &Board)
    {
        self.feature_bit = [[0; N_ROTATION]; N_PATTERN];
        
        let p: u64 = board.player;
        let o: u64 = board.opponent;
        
        for pattern in 0..N_PATTERN {
            let fbit = &mut self.feature_bit[pattern];
            for rotation in 0..N_ROTATION {
                for coord_i in 0..FEATURE_COORD[pattern].n_square {
                    let coord = FEATURE_COORD[pattern].feature_coord[rotation][coord_i as usize];

                    #[cfg(debug_assertions)]
                    if coord == NO_COORD {panic!()}

                    let color = 2 * (1 & p >> coord) + (1 & o >> coord);
                    fbit[rotation] = fbit[rotation] * 3u16 + color as u16;
                }
            }
        }
    }

    #[inline(always)]
    pub fn calc_eval(&self, board: &Board) -> i32
    {
        let move_count = board.move_count();
        let phase = move_count as usize / 2;

        let mut evaluation  = 0;
        
        // todo: eval の実装を見直す。
        // let eval_scores = &self.eval[board.next_turn][phase];
        let eval_scores = &self.eval[board.empties_count() as usize % 2][phase];
        for pattern in 0..N_PATTERN {
            // let e = &eval_scores.pattern_eval[pattern];
            // let f = &self.feature_bit[pattern];

            // for each rotaion
            evaluation += eval_scores.pattern_eval[pattern][self.feature_bit[pattern][0] as usize] as i32 
                            + eval_scores.pattern_eval[pattern][self.feature_bit[pattern][1] as usize] as i32
                            + eval_scores.pattern_eval[pattern][self.feature_bit[pattern][2] as usize] as i32
                            + eval_scores.pattern_eval[pattern][self.feature_bit[pattern][3] as usize] as i32;
        }

        let mobility = 
            N_MOBILITY_BASE 
            + board.moves().count_ones() as usize 
            - board.opponent_moves().count_ones() as usize;

        evaluation += eval_scores.mobility_eval[mobility] as i32;
        evaluation += eval_scores.const_eval as i32;

        evaluation
    }


    #[inline(always)]
    pub fn clac_features_eval(&mut self, board: &Board) -> i32{

        self.clac_features(board);
        let mut e = self.calc_eval(board);

        if e > 0 {e += SCORE_RATE/2;} else if e < 0 {e -= SCORE_RATE/2;}
        e /= SCORE_RATE;

        if e > SCORE_MAX {e = SCORE_MAX;} else if e < -SCORE_MAX {e = -SCORE_MAX;}     
        e
    }

    pub fn write_file(&self, path: &str) -> std::io::Result<()>
    {
        // serialized
        let serialized: String = serde_json::to_string(self).unwrap();

        // write
        let mut file = File::create(path)?;
        file.write_all(serialized.as_bytes())?;
        Ok(())
    }

    pub fn read_file(path : &str) -> std::io::Result<Evaluator>
    {
        let input = fs::read_to_string(path)?;
        let deserialized: Evaluator = serde_json::from_str(&input).unwrap();
        Ok(deserialized)
    }

    pub fn read_string(input: &str) -> std::io::Result<Evaluator>
    {
        let deserialized: Evaluator = serde_json::from_str(input).unwrap();
        Ok(deserialized)
    }
}
