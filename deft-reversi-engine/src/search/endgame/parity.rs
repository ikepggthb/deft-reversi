//! Shared quadrant geometry and parity updates.
use crate::Board;

pub(in crate::search) const QUADRANT_MASKS: [u64; 4] = [
    0x000000000f0f0f0f,
    0x00000000f0f0f0f0,
    0x0f0f0f0f00000000,
    0xf0f0f0f000000000,
];
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
pub(super) const SQUARE_PARITY: [u8; 64] = {
    let mut ids = [0; 64];
    let mut square = 0;
    while square < 64 {
        ids[square] = 1 << (((square >> 2) & 1) | ((square >> 4) & 2));
        square += 1;
    }
    ids
};
pub(super) fn tail_parity(board: Board) -> u8 {
    let empty = !(board.player_bits() | board.opponent_bits());
    QUADRANT_MASKS
        .iter()
        .enumerate()
        .fold(0, |parity, (i, mask)| {
            parity | (((empty & mask).count_ones() as u8 & 1) << i)
        })
}

pub(in crate::search) fn odd_regions(board: Board) -> u64 {
    let empty = !(board.player_bits() | board.opponent_bits());
    QUADRANT_MASKS
        .into_iter()
        .filter(|region| (empty & region).count_ones() % 2 == 1)
        .fold(0, |bits, region| bits | region)
}

pub(super) const QUADRANT_ID: [i32; 64] = {
    let mut ids = [0; 64];
    let mut square = 0;
    while square < 64 {
        ids[square] = SQUARE_PARITY[square] as i32;
        square += 1;
    }
    ids
};
