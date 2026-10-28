//! MPC configuration and regression equations ported from the reference engine.
use serde::{Deserialize, Serialize};
use std::io;

use crate::model::invalid_data;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct MpcParams {
    pub lv: i32,
    pub a: f64,
    pub b: f64,
    pub e_std: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct MpcRegression {
    pub constant: f64,
    pub empties: f64,
    pub depth: f64,
    pub mpc_depth: f64,
    pub parity: f64,
    pub parity_times_empties: f64,
    pub parity_times_mpc_depth: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EvalSearchMpcConfig {
    pub search_lv_by_depth: Vec<i32>,
    pub a: MpcRegression,
    pub b: MpcRegression,
    pub e_std: MpcRegression,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FinalSearchMpcConfig {
    pub search_lv_by_empties: Vec<i32>,
    pub a: MpcRegression,
    pub b: MpcRegression,
    pub e_std: MpcRegression,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MpcConfig {
    pub eval_search: EvalSearchMpcConfig,
    pub final_search: FinalSearchMpcConfig,
}

impl Default for MpcConfig {
    fn default() -> Self {
        let mut search_lv_by_empties = vec![0; 61];
        search_lv_by_empties[12] = 4;
        search_lv_by_empties[13] = 3;
        search_lv_by_empties[14] = 4;
        search_lv_by_empties[15] = 3;
        search_lv_by_empties[16] = 4;
        search_lv_by_empties[17] = 5;
        search_lv_by_empties[18] = 6;
        search_lv_by_empties[19] = 5;
        search_lv_by_empties[20] = 6;
        for (empties, depth) in search_lv_by_empties
            .iter_mut()
            .enumerate()
            .take(25)
            .skip(21)
        {
            *depth = if empties % 2 == 1 { 5 } else { 6 };
        }
        for (empties, depth) in search_lv_by_empties.iter_mut().enumerate().skip(25) {
            *depth = if empties % 2 == 1 { 7 } else { 8 };
        }

        Self {
            eval_search: EvalSearchMpcConfig {
                search_lv_by_depth: vec![
                    0, 0, 0, 0, 0, 1, 2, 1, 2, 1, 2, 3, 4, 3, 4, 5, 6, 5, 6, 5, 6, 5, 6, 5, 6, 7,
                    8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10,
                    9, 10, 9, 10, 9, 10, 9, 10, 9, 10,
                ],
                a: MpcRegression {
                    constant: 0.997868,
                    empties: -0.000399,
                    depth: -0.000590,
                    mpc_depth: 0.003595,
                    parity: 0.0,
                    parity_times_empties: 0.0,
                    parity_times_mpc_depth: 0.0,
                },
                b: MpcRegression {
                    constant: -0.345286,
                    empties: -0.000993,
                    depth: -0.097065,
                    mpc_depth: 0.264205,
                    parity: 0.0,
                    parity_times_empties: 0.0,
                    parity_times_mpc_depth: 0.0,
                },
                e_std: MpcRegression {
                    constant: 3.887029,
                    empties: -0.043874,
                    depth: 0.323397,
                    mpc_depth: -0.609174,
                    parity: 0.0,
                    parity_times_empties: 0.0,
                    parity_times_mpc_depth: 0.0,
                },
            },
            final_search: FinalSearchMpcConfig {
                search_lv_by_empties,
                a: MpcRegression {
                    constant: 0.9332684220312124,
                    empties: 0.004543430692204453,
                    depth: 0.0,
                    mpc_depth: 0.012922499320745613,
                    parity: 0.015489820174500074,
                    parity_times_empties: -0.0005654444826787643,
                    parity_times_mpc_depth: -0.0011278528627353126,
                },
                b: MpcRegression {
                    constant: -0.5126261068418335,
                    empties: 0.04424483505280841,
                    depth: 0.0,
                    mpc_depth: 0.18159084239998452,
                    parity: -1.8427413014936342,
                    parity_times_empties: -0.004710675039834301,
                    parity_times_mpc_depth: -0.10638397369827086,
                },
                e_std: MpcRegression {
                    constant: 6.920480361241869,
                    empties: -0.1028348971879949,
                    depth: 0.0,
                    mpc_depth: -0.2988172382876506,
                    parity: 0.09523292183381482,
                    parity_times_empties: 0.01370717289362976,
                    parity_times_mpc_depth: -0.027986687523608764,
                },
            },
        }
    }
}

impl MpcRegression {
    pub fn evaluate(&self, empties: f64, depth: f64, mpc_depth: f64, parity: f64) -> f64 {
        self.constant
            + self.empties * empties
            + self.depth * depth
            + self.mpc_depth * mpc_depth
            + self.parity * parity
            + self.parity_times_empties * parity * empties
            + self.parity_times_mpc_depth * parity * mpc_depth
    }
}

impl EvalSearchMpcConfig {
    pub fn params(&self, depth: i32, empties: i32) -> MpcParams {
        let mpc_depth = self.search_lv_by_depth[depth as usize];
        let empties = empties as f64;
        let depth = depth as f64;
        let mpc_depth_f64 = mpc_depth as f64;

        MpcParams {
            lv: mpc_depth,
            a: self.a.evaluate(empties, depth, mpc_depth_f64, 0.0),
            b: self.b.evaluate(empties, depth, mpc_depth_f64, 0.0),
            e_std: self.e_std.evaluate(empties, depth, mpc_depth_f64, 0.0),
        }
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.search_lv_by_depth.len() != 61 {
            return Err(invalid_data(format!(
                "eval_search.search_lv_by_depth must be 61, got {}",
                self.search_lv_by_depth.len()
            )));
        }
        for &depth in &self.search_lv_by_depth {
            if !(0..=60).contains(&depth) {
                return Err(invalid_data(format!(
                    "eval_search.search_lv_by_depth values must be in 0..=60, got {depth}"
                )));
            }
        }
        for depth in 4..=60 {
            if self.search_lv_by_depth[depth] >= depth as i32 {
                return Err(invalid_data(format!(
                    "eval_search confirmation depth must be less than search depth {depth}"
                )));
            }
        }
        Ok(())
    }
}

impl FinalSearchMpcConfig {
    pub fn params(&self, empties: i32) -> MpcParams {
        let idx = empties as usize;
        let lv = self.search_lv_by_empties[idx];
        let empties = empties as f64;
        let mpc_depth = lv as f64;
        let parity = if idx % 2 == 1 { 1.0 } else { 0.0 };

        MpcParams {
            lv,
            a: self.a.evaluate(empties, 0.0, mpc_depth, parity),
            b: self.b.evaluate(empties, 0.0, mpc_depth, parity),
            e_std: self.e_std.evaluate(empties, 0.0, mpc_depth, parity),
        }
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.search_lv_by_empties.len() != 61 {
            return Err(invalid_data(format!(
                "final_search.search_lv_by_empties must be 61, got {}",
                self.search_lv_by_empties.len()
            )));
        }
        for &lv in &self.search_lv_by_empties {
            if !(0..=60).contains(&lv) {
                return Err(invalid_data(format!(
                    "final_search search level must be in 0..=60, got {lv}"
                )));
            }
        }
        for empties in 12..=60 {
            if self.search_lv_by_empties[empties] >= empties as i32 {
                return Err(invalid_data(format!(
                    "final_search confirmation depth must be less than empties {empties}"
                )));
            }
        }
        Ok(())
    }
}

impl MpcConfig {
    pub fn validate(&self) -> io::Result<()> {
        self.eval_search.validate()?;
        self.final_search.validate()?;
        Ok(())
    }
}

/// The discrete final-search table from main at 5a17bdce. The older model
/// does not embed MPC parameters; callers must opt in for that model.
pub(crate) fn legacy_main_rules() -> Vec<crate::ProbCutRule> {
    let mut rules = MpcConfig::default().rules();
    rules.retain(|r| !r.exact);
    let measured = [
        (
            4,
            1.0124526095640112,
            0.48931748030193045,
            4.914869788936018,
        ),
        (
            3,
            1.0133739578163794,
            -1.7543169526476112,
            5.389238400539998,
        ),
        (
            4,
            1.0139957691780979,
            0.44804332300410943,
            5.086939241974191,
        ),
        (
            3,
            1.0196101764153223,
            -1.8127583017145918,
            5.446378502556965,
        ),
        (4, 1.0240411920303725, 0.495222385942482, 5.08401904278953),
        (
            5,
            1.0253130521132232,
            -1.6594124445791587,
            4.817367363807651,
        ),
        (6, 1.0281382671934618, 0.7327216969440862, 4.589407020617368),
        (
            5,
            1.0316652919636162,
            -1.8229371340268825,
            4.810056307416734,
        ),
        (6, 1.0368717822009714, 0.8963184321993207, 4.575580119070469),
    ];
    for empties in 12..=60u8 {
        let (mut shallow_depth, slope, intercept, deviation) =
            measured[usize::from((empties - 12).min(8))];
        let (slope, intercept, deviation) = if empties > 20 && empties % 2 == 1 {
            (measured[7].1, measured[7].2, measured[7].3)
        } else {
            (slope, intercept, deviation)
        };
        if empties > 20 {
            shallow_depth = if empties >= 29 {
                if empties % 2 == 0 { 8 } else { 7 }
            } else if empties % 2 == 0 {
                6
            } else {
                5
            };
        }
        rules.push(crate::ProbCutRule {
            exact: true,
            depth: empties,
            empties,
            shallow_depth,
            slope,
            intercept,
            deviation,
        });
    }
    rules
}

impl MpcConfig {
    pub(crate) fn rules(&self) -> Vec<crate::ProbCutRule> {
        let mut rules = Vec::new();
        for exact in [false, true] {
            for depth in (if exact { 12 } else { 4 })..=60u8 {
                for empties in depth..=60u8 {
                    if exact && empties != depth {
                        continue;
                    }
                    let params = if exact {
                        self.final_search.params(i32::from(empties))
                    } else {
                        self.eval_search
                            .params(i32::from(depth), i32::from(empties))
                    };
                    if params.lv < 0
                        || !params.a.is_finite()
                        || params.a <= 0.0
                        || !params.b.is_finite()
                        || !params.e_std.is_finite()
                    {
                        continue;
                    }
                    rules.push(crate::ProbCutRule {
                        exact,
                        depth,
                        empties,
                        shallow_depth: params.lv as u8,
                        slope: params.a,
                        intercept: params.b,
                        deviation: params.e_std,
                    });
                }
            }
        }
        rules
    }
}

#[cfg(test)]
mod legacy_tests {
    use super::*;

    #[test]
    fn main_final_table_boundaries_and_reused_coefficients() {
        let rules = legacy_main_rules();
        let entry = |n| *rules.iter().find(|r| r.exact && r.empties == n).unwrap();
        assert!(!rules.iter().any(|r| r.exact && r.empties < 12));
        assert_eq!(
            (entry(12).shallow_depth, entry(12).slope),
            (4, 1.0124526095640112)
        );
        assert_eq!(
            (entry(19).shallow_depth, entry(19).intercept),
            (5, -1.8229371340268825)
        );
        assert_eq!(entry(21).slope, entry(19).slope);
        assert_eq!(entry(28).slope, entry(20).slope);
        assert_eq!(
            (
                entry(28).shallow_depth,
                entry(29).shallow_depth,
                entry(30).shallow_depth
            ),
            (6, 7, 8)
        );
        assert_eq!(entry(59).slope, entry(19).slope);
        assert_eq!(entry(60).slope, entry(20).slope);
    }
}
