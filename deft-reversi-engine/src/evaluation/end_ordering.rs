//! Experimental endgame ordering using the existing 44 pattern tables.
//! These weights have not been trained specifically for ordering.
use super::*;

impl PatternModel {
    /// Pattern-only ordering score. Mobility is already available to the caller;
    /// this deliberately avoids generating either side's legal moves again.
    pub(crate) fn end_ordering_child_value(
        &self,
        board: Board,
        parent: &EvalState,
        bit: u64,
        flips: u64,
    ) -> i32 {
        let indices = parent.ordering_child_indices(bit, flips);
        let phase = usize::from(60u8.saturating_sub(board.empty_count())).min(59);
        let weights = &self.phases[phase];
        let mut raw = i32::from(weights.bias);
        let mut offset = 0;
        for (p, size) in PATTERN_TABLE_SIZES.iter().enumerate() {
            for r in 0..4 {
                raw += i32::from(weights.weights[offset + indices[p * 4 + r] as usize]);
            }
            offset += size;
        }
        ((raw + raw.signum() * 64) / 128).clamp(-64, 64)
    }
}

impl EvalState {
    // An ordering evaluation only needs the child's current viewpoint.
    // Preserve the full two-viewpoint update for children actually searched.
    pub(super) fn ordering_child_indices(&self, bit: u64, mut flips: u64) -> [u16; LANES] {
        let mut indices = self.swapped;
        for (index, d) in indices
            .iter_mut()
            .zip(&SQUARE_DELTA[bit.trailing_zeros() as usize])
        {
            *index = index.wrapping_add(*d);
        }
        while flips != 0 {
            let delta = &SQUARE_DELTA[flips.trailing_zeros() as usize];
            flips &= flips - 1;
            for (index, d) in indices.iter_mut().zip(delta) {
                *index = index.wrapping_sub(*d);
            }
        }
        indices
    }
}
