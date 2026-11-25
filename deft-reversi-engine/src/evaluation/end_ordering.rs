//! 既存の 44 個のパターンテーブルを流用した、実験的な終盤の着手順序付け（ムーブオーダリング）評価です。
//! これらの重み自体は順序付け専用に学習されたものではありません。
use super::*;

impl PatternModel {
    /// パターンテーブルだけを用いた、着手順序付け用の評価値を計算します。
    ///
    /// 合法手数（モビリティ）は呼び出し側で既に把握しているため、
    /// ここでは計算コストを抑える目的で双方の合法手再生成を意図的に省いています。
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
    /// 順序付け評価では、子局面の「現在の手番視点」のインデックスだけがあれば十分です。
    /// 実際に探索する子ノードに対してだけ、両視点の完全な差分更新を行います。
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
