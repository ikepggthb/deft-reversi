//! 盤面を 4 象限（4×4 の 4 領域）に分割し、各領域の空きマス数の偶奇性（パリティ）を計算・更新するユーティリティです。
//!
//! 終盤では、空きマスが奇数個残っている領域に先に打つと、自分がその領域の最後の手（手止まり）を取りやすくなります。
//! そのためパリティを手がかりに候補手を優先付けすると、探索の枝刈りが増えます。
use crate::Board;

/// 盤面の 4 象限（ビットボード上の 4×4 領域）を表すマスク配列です。
pub(in crate::search) const QUADRANT_MASKS: [u64; 4] = [
    0x000000000f0f0f0f,
    0x00000000f0f0f0f0,
    0x0f0f0f0f00000000,
    0xf0f0f0f000000000,
];
/// パリティビット（0..16）から奇数個空きがある象限のビットマスクを引くテスト用テーブルです。
#[cfg(test)]
pub(super) const PARITY_REGIONS: [u64; 16] = {
    let mut masks = [0; 16];
    let mut parity = 0;
    while parity < 16 {
        let mut region = 0;
        while region < 4 {
            if parity & (1 << region) != 0 {
                masks[parity] |= QUADRANT_MASKS[region];
            }
            region += 1;
        }
        parity += 1;
    }
    masks
};
/// 各マス（0..64）が属する象限の 1-hot ビット（`1 << quadrant`）です。XOR でパリティを差分更新できます。
pub(super) const SQUARE_PARITY: [u8; 64] = {
    let mut ids = [0; 64];
    let mut square = 0;
    while square < 64 {
        ids[square] = 1 << (((square >> 2) & 1) | ((square >> 4) & 2));
        square += 1;
    }
    ids
};
/// 現在の盤面から 4 象限の空きマス偶奇性を 4 ビット整数（0..16）として算出します。
pub(super) fn tail_parity(board: Board) -> u8 {
    let empty = !(board.player_bits() | board.opponent_bits());
    QUADRANT_MASKS
        .iter()
        .enumerate()
        .fold(0, |parity, (i, mask)| {
            parity | (((empty & mask).count_ones() as u8 & 1) << i)
        })
}

/// 奇数個の空きマスを含む象限のマスだけが立ったビットマスクを返します。
#[inline]
pub(in crate::search) fn odd_regions(board: Board) -> u64 {
    let empty = !(board.player_bits() | board.opponent_bits());
    QUADRANT_MASKS
        .into_iter()
        .filter(|region| (empty & region).count_ones() % 2 == 1)
        .fold(0, |bits, region| bits | region)
}

/// 各マス（0..64）の象限マスクです（`SQUARE_PARITY` の `i32` 版）。
pub(super) const QUADRANT_ID: [i32; 64] = {
    let mut ids = [0; 64];
    let mut square = 0;
    while square < 64 {
        ids[square] = SQUARE_PARITY[square] as i32;
        square += 1;
    }
    ids
};
