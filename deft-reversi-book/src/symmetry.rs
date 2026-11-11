//! 盤面と着手を同じ回転・反転へ変換する。点数の符号と手番の色は変えない。
//! 正規化はKey::newから使い、対局へ返す着手は逆変換する。
use super::*;
/// Same transform order as Board::canonical. Never swaps the two colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symmetry(u8);
impl Symmetry {
    pub fn all() -> [Self; 8] {
        std::array::from_fn(|i| Self(i as u8))
    }
    pub fn board(self, b: Board) -> Board {
        b.symmetries()[self.0 as usize]
    }
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
