pub mod bitboard;
/// The four corner squares.
pub(crate) const CORNERS: u64 = 0x8100000000000081;
mod flip;
mod moves;
pub mod pos;

pub(crate) mod last;

pub(crate) mod stability;

mod edge_stability;
