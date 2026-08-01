use crate::file::invalid_data;
use std::io;

use super::evaluator_const::SCORE_SCALE;

#[inline(always)]
pub(crate) fn div_round_to_disc_score(mut score: i32) -> i32 {
    if score > 0 {
        score += SCORE_SCALE / 2;
    } else if score < 0 {
        score -= SCORE_SCALE / 2;
    }
    score / SCORE_SCALE
}

pub(crate) fn fixed_vec<T, const N: usize>(values: Vec<T>, name: &str) -> io::Result<[T; N]> {
    values
        .try_into()
        .map_err(|values: Vec<T>| invalid_data(format!("{name} must be {N}, got {}", values.len())))
}
