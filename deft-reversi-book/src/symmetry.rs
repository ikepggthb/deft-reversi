//! 盤面と着手を同じ回転・反転規則で変換するモジュール（点数の符号や手番の色は変えない）。
//!
//! リバーシの盤面は回転・反転によって 8 通りの対称形を持つため、Book では
//! 代表となる 1 つの向きに揃えて（正規化して）保存することで容量と探索を節約する。
//! 正規化は `Key::new` から使い、対局時に呼び出し側へ手を返すときは `inverse()` で元の向きに戻す。
use super::*;
/// `Board::canonical` と同じ変換順序の D4 対称変換（0..8）。手番の色は入れ替えない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symmetry(u8);
impl Symmetry {
    /// 全 8 通りの対称変換を返す。
    pub fn all() -> [Self; 8] {
        std::array::from_fn(|i| Self(i as u8))
    }
    /// 盤面 `b` にこの対称変換を適用する。
    pub fn board(self, b: Board) -> Board {
        b.symmetries()[self.0 as usize]
    }
    /// 着手 `a` にこの対称変換を適用する（パスはそのまま返す）。
    pub fn action(self, a: Action) -> Action {
        let Action::Place(s) = a else { return a };
        let (mut x, mut y) = (s.index() % 8, s.index() / 8);
        if self.0 & 1 != 0 {
            std::mem::swap(&mut x, &mut y);
        }
        match self.0 / 2 {
            1 => x = 7 - x,
            2 => y = 7 - y,
            3 => {
                x = 7 - x;
                y = 7 - y;
            }
            _ => {}
        }
        Action::Place(deft_reversi_engine::Square::new(y * 8 + x).unwrap())
    }
    /// この対称変換の逆変換を返す。
    pub fn inverse(self) -> Self {
        match self.0 {
            3 => Self(5),
            5 => Self(3),
            _ => self,
        }
    }
}
pub(super) fn canonical(b: Board) -> (Board, Symmetry) {
    let (i, b) = b
        .symmetries()
        .into_iter()
        .enumerate()
        .min_by_key(|(i, b)| (b.player_bits(), b.opponent_bits(), *i))
        .unwrap();
    (b, Symmetry(i as u8))
}
