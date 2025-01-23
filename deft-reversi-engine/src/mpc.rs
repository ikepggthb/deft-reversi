use crate::{
    board::*,
    search::*,
    eval_search::*,
    eval::evaluator_const::SCORE_MAX,
};

pub enum ProbCutResult {
    Cut(i32),
    Fail
}

pub struct Selectivity {
    value: f64,
    pub percent: i32
}

pub const SELECTIVITY_LV_MAX: i32 = 6;
pub const NO_MPC: i32 = SELECTIVITY_LV_MAX;
pub const N_SELECTIVITY_LV: usize = 7;

// 0 ~ 6
// 0 : 68%
// 1 : 77%
// 2 : 85%
// 3 : 95%
// 4 : 98%
// 5 : 99%
// 6 : No MPC
pub const SELECTIVITY: [Selectivity;N_SELECTIVITY_LV] =
[    
    Selectivity {
        // level: 0,
        value: 1.0,
        percent: 68
    },
    Selectivity{
        // level: 1,
        value: 1.20,
        percent: 77
    },

    Selectivity {
        // level: 2,
        value: 1.43953,
        percent: 85
    },

    Selectivity {
        // level: 3,
        value: 1.960,
        percent: 95
    },
    Selectivity {
       //  level: 4,
        value: 2.326,
        percent: 98
    },
    Selectivity {
        // level: 5,
        value: 2.576,
        percent: 99
    },
    Selectivity {
        // level: 6,
        value: 0.0,
        percent: 100
    },

];

pub struct MpcParams {
    pub lv    : i32,
        a     : f64,
        b     : f64,
        e_std : f64
}

pub const PERFECT_SEARCH_MPC_SEARCH_PARAMS: [Option<MpcParams>; 61] = [
    None, // 存在しない
    None, // 残り1マス
    None, // 2
    None, // 3
    None, // 4
    None, // Some(MpcParams {lv: 0, a: 1.0074043224061988, b: 0.1117791373894379, e_std: 5.0233053841493716 }),
    None, // Some(MpcParams {lv: 0, a: 1.0112166361063544, b: -0.14253698534316595, e_std: 5.297556997020212 }),
    None, //Some(MpcParams {lv: 1, a: 1.005571548442915, b: -1.126906934250505, e_std: 5.25480233054599 }),
    None, //Some(MpcParams {lv: 2, a: 1.0062276059335247, b: -0.014437236487932381, e_std: 4.941724779937074 }),
    None, //Some(MpcParams {lv: 1, a: 1.0065756943172985, b: -1.682073302318966, e_std: 5.725485180912335 }),
    None, //Some(MpcParams {lv: 2, a: 1.0097842599977092, b: 0.2906818429547857, e_std: 5.396171804842368 }),
    None, // Some(MpcParams {lv: 3, a: 1.0093185115319934, b: -1.6145334082191898, e_std: 5.22706268772541 }),
    Some(MpcParams {lv: 4, a: 1.0124526095640112, b: 0.48931748030193045, e_std: 4.914869788936018 }),
    Some(MpcParams {lv: 3, a: 1.0133739578163794, b: -1.7543169526476112, e_std: 5.389238400539998 }),
    Some(MpcParams {lv: 4, a: 1.0139957691780979, b: 0.44804332300410943, e_std: 5.086939241974191 }),
    Some(MpcParams {lv: 3, a: 1.0196101764153223, b: -1.8127583017145918, e_std: 5.446378502556965 }),
    Some(MpcParams {lv: 4, a: 1.0240411920303725, b: 0.495222385942482, e_std: 5.08401904278953 }),
    Some(MpcParams {lv: 5, a: 1.0253130521132232, b: -1.6594124445791587, e_std: 4.817367363807651 }),
    Some(MpcParams {lv: 6, a: 1.0281382671934618, b: 0.7327216969440862, e_std: 4.589407020617368 }),
    Some(MpcParams {lv: 5, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 6, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), // 20
    // ここからの、統計的なパラメータの計算はされていない。
    // 空きマスが20のときの値を流用している。
    Some(MpcParams {lv: 5, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 6, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 5, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 6, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 5, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 6, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 5, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 6, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
    Some(MpcParams {lv: 7, a: 1.0316652919636162, b: -1.8229371340268825, e_std: 4.810056307416734 }),
    Some(MpcParams {lv: 8, a: 1.0368717822009714, b: 0.8963184321993207, e_std: 4.575580119070469 }), 
];


const MPC_START_LEVEL_EVAL_SEARCH: i32 = 4;
pub const EVAL_SEARCH_MPC_SEARCH_LV: [i32; 61] = [
    0,
    0, 0, 0, 0, 1, 2, 1, 2, 1, 2,
    3, 4, 3, 4, 5, 6, 5, 6, 5, 6,
    5, 6, 5, 6, 7, 8, 7, 8, 7, 8,
    7, 8, 7, 8, 7, 8, 7, 8, 7, 8,
    9, 10, 9, 10, 9, 10, 9, 10, 9, 10,
    9, 10, 9, 10, 9, 10, 9, 10, 9, 10
];

fn gen_eval_search_mpc_params(lv_i32: i32, n_empties: i32) -> MpcParams
{
    let mpc_lv_i32 = EVAL_SEARCH_MPC_SEARCH_LV[lv_i32 as usize];
    let lv = lv_i32 as f64;
    let mpc_lv = mpc_lv_i32 as f64;
    let n_empties = n_empties as f64;

    let a 
        =  0.997868 
        + -0.000399 * n_empties 
        + -0.000590 * lv 
        +  0.003595 * mpc_lv;

    let b 
        = -0.345286 
        + -0.000993 * n_empties 
        + -0.097065 * lv 
        +  0.264205 * mpc_lv;

    let e_std 
        =  3.887029 
        + -0.043874 * n_empties
        +  0.323397 * lv 
        + -0.609174 * mpc_lv;

    MpcParams { lv: mpc_lv_i32, a, b, e_std}
}

#[inline(always)]
pub fn eval_search_mpc(
    board          : &Board,
    alpha          : i32,
    beta           : i32,
    lv             : i32,
    search         : &mut Search
) -> ProbCutResult
{
    if lv < MPC_START_LEVEL_EVAL_SEARCH {
        return  ProbCutResult::Fail;
    }
    let n_empties = board.empties_count();
    let mpc_params = gen_eval_search_mpc_params(lv, n_empties);

    multi_prob_cut(board, alpha, beta, &mpc_params, search)
}

#[inline(always)]
pub fn perfect_search_mpc(
    board          : &Board,
    alpha          : i32,
    beta           : i32,
    search         : &mut Search
) -> ProbCutResult
{
    let n_empties = board.empties_count();
    let mpc_params = match &PERFECT_SEARCH_MPC_SEARCH_PARAMS[n_empties as usize] {
        Some(params) => { params },
        None                     => { return ProbCutResult::Fail }
    };
    multi_prob_cut(board, alpha, beta, mpc_params, search)
}

#[inline(always)]
pub fn multi_prob_cut(
    board            : &Board,
    alpha            : i32,
    beta             : i32,
    mpc_params       : &MpcParams, 
    search           : &mut Search
) -> ProbCutResult 
{
    if alpha >= SCORE_MAX {return ProbCutResult::Cut(alpha)}
    if beta <= -SCORE_MAX {return ProbCutResult::Cut(beta)}

    if search.selectivity_lv == NO_MPC { // no selectivity
        return ProbCutResult::Fail;
    }

    #[cfg(debug_assertions)]
    if search.selectivity_lv > SELECTIVITY_LV_MAX {
        {panic!();}
    }
    
    let mpc_search_lv : i32 = mpc_params.lv;
    let a             : f64 = mpc_params.a;
    let b             : f64 = mpc_params.b;
    let z             : f64 = SELECTIVITY[search.selectivity_lv as usize].value;
    let e_allow       : f64 = z * mpc_params.e_std;

    // V = av + b ± e;

    // v = (V - b ± e)/a

    // upper cut (beta)
    let upper = ((beta as f64 - b + e_allow)/a).ceil() as i32;
    if upper < SCORE_MAX { 
        // nwll window serach => [upper - 1, upper]
        let v = nws_eval_0_selectivity_lv(board, upper-1, mpc_search_lv, search);
        if v >= upper {
            return ProbCutResult::Cut(beta);
        }        
    }
    
    // lower cut (alpha)
    let lower = ((alpha as f64 - b - e_allow)/a).floor() as i32;
    if lower > -SCORE_MAX { 
        // nwll window serach => [lower, lower + 1]
        let v = nws_eval_0_selectivity_lv(board, lower, mpc_search_lv, search);
        if v <= lower {
            return ProbCutResult::Cut(alpha);
        }
    }
    
    ProbCutResult::Fail
}

#[inline(always)]
fn nws_eval_0_selectivity_lv(board: &Board, alpha: i32, lv: i32, search : &mut Search) -> i32
{
    let main_search_selectivity_lv = search.selectivity_lv;
    search.selectivity_lv = NO_MPC;
    let score = nws_eval(board, alpha, lv, search);
    search.selectivity_lv = main_search_selectivity_lv;
    score
}