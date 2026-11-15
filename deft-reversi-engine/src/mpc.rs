//! The ProbCut rules of the legacy main branch, for models converted from
//! it. Every other model carries its own calibration in its DPR2 file.
use crate::search::ProbCutRule;

/// Shallow depth for each midgame depth, from the reference engine.
const SHALLOW_BY_DEPTH: [u8; 61] = [
    0, 0, 0, 0, 0, 1, 2, 1, 2, 1, 2, 3, 4, 3, 4, 5, 6, 5, 6, 5, 6, 5, 6, 5, 6, 7, 8, 7, 8, 7, 8, 7,
    8, 7, 8, 7, 8, 7, 8, 7, 8, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10, 9,
    10,
];

/// `c[0] + c[1]·empties + c[2]·depth + c[3]·shallow depth`, the reference
/// engine's midgame regression.
fn linear(c: [f64; 4], empties: f64, depth: f64, shallow: f64) -> f64 {
    c[0] + c[1] * empties + c[2] * depth + c[3] * shallow
}

/// The midgame regression of the reference engine and the discrete
/// final-search table from main at 5a17bdce.
pub(crate) fn legacy_main_rules() -> Vec<ProbCutRule> {
    const SLOPE: [f64; 4] = [0.997868, -0.000399, -0.000590, 0.003595];
    const INTERCEPT: [f64; 4] = [-0.345286, -0.000993, -0.097065, 0.264205];
    const DEVIATION: [f64; 4] = [3.887029, -0.043874, 0.323397, -0.609174];
    let mut rules = Vec::new();
    for depth in 4..=60u8 {
        let shallow_depth = SHALLOW_BY_DEPTH[usize::from(depth)];
        for empties in depth..=60u8 {
            let at = |c| linear(c, empties.into(), depth.into(), shallow_depth.into());
            let (slope, intercept, deviation) = (at(SLOPE), at(INTERCEPT), at(DEVIATION));
            if slope > 0.0 {
                rules.push(ProbCutRule {
                    exact: false,
                    depth,
                    empties,
                    shallow_depth,
                    slope,
                    intercept,
                    deviation,
                });
            }
        }
    }
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
            shallow_depth = match (empties >= 29, empties % 2 == 0) {
                (true, true) => 8,
                (true, false) => 7,
                (false, true) => 6,
                (false, false) => 5,
            };
        }
        rules.push(ProbCutRule {
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
        let mid = rules.iter().filter(|r| !r.exact).count();
        assert_eq!(rules.len(), mid + 49);
    }
}
