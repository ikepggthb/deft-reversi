//! 盤面演算の中核モジュールです。
//!
//! 64 ビット整数を使ったビットボード表現により、合法手の生成、石の反転、
//! および確定石（一度置かれると最後まで裏返らない石）の判定を高速に行います。

pub mod bitboard;
/// 四隅（A1, H1, A8, H8）を表すビットマスクです。
pub(crate) const CORNERS: u64 = 0x8100000000000081;
mod flip;
mod moves;
pub mod pos;

pub(crate) mod last;

pub(crate) mod stability;

mod edge_stability;
