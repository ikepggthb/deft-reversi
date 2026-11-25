//! 盤面上のマス座標（[`Square`]）、着手（[`Action`]）、およびマスの集合を表すビットマスク（[`Moves`]）の基本型を定義します。
//!
//! 8×8 の盤面は 64 マスあるため、64 ビット符号なし整数（`u64`）の各ビットを 1 マスに対応させて
//! マス集合や盤面状態を高速に表現します（ビットボード表現）。

use std::{fmt, str::FromStr};

/// 盤面上の 1 マスを表すインデックス（0..64）です。
///
/// `index = y * 8 + x` の行優先レイアウトを採用しており、
/// A1 が 0（最下位ビット: LSB）、H8 が 63（最上位ビット: MSB）に対応します。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Square(u8);
impl Square {
    /// 0..64 の線形インデックスから [`Square`] を生成します。範囲外なら `None` を返します。
    pub const fn new(index: u8) -> Option<Self> {
        if index < 64 { Some(Self(index)) } else { None }
    }
    /// 空でないビットマスクのうち、最も小さい番号の立っているビット（最下位ビット）に対応するマスを返します。
    pub(crate) const fn lowest(mask: u64) -> Self {
        debug_assert!(mask != 0);
        Self(mask.trailing_zeros() as u8)
    }
    /// 0 始まりの列 `x`（a..h）と行 `y`（1..8）から [`Square`] を生成します。範囲外なら `None` を返します。
    pub const fn from_xy(x: u8, y: u8) -> Option<Self> {
        if x < 8 && y < 8 {
            Some(Self(y * 8 + x))
        } else {
            None
        }
    }
    /// 列インデックス（0..8、a..h に対応）を返します。
    pub const fn x(self) -> u8 {
        self.0 % 8
    }
    /// 行インデックス（0..8、1..8 に対応）を返します。
    pub const fn y(self) -> u8 {
        self.0 / 8
    }
    /// 0..64 の線形マスインデックスを返します。
    pub const fn index(self) -> u8 {
        self.0
    }
    /// このマスのみが 1 に立った `u64` ビットマスクを返します。
    pub const fn bit(self) -> u64 {
        1u64 << self.0
    }
}
impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            (b'A' + self.0 % 8) as char,
            (b'1' + self.0 / 8) as char
        )
    }
}
impl FromStr for Square {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let b = value.as_bytes();
        if b.len() != 2
            || !(b'a'..=b'h').contains(&b[0].to_ascii_lowercase())
            || !(b'1'..=b'8').contains(&b[1])
        {
            return Err("square must be A1..H8");
        }
        Ok(Self((b[1] - b'1') * 8 + b[0].to_ascii_lowercase() - b'a'))
    }
}
/// プレイヤーの 1 回の着手行動（石を置くか、パスするか）を表します。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// 指定したマスに石を置きます。
    Place(Square),
    /// 合法手がないため手番をパスします。
    Pass,
}
impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Place(square) => square.fmt(f),
            Self::Pass => f.write_str("pass"),
        }
    }
}
impl FromStr for Action {
    type Err = &'static str;
    /// `f5` のようなマス座標、または `pass` を解析します（大文字・小文字は区別しません）。
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("pass") {
            Ok(Self::Pass)
        } else {
            value
                .parse()
                .map(Self::Place)
                .map_err(|_| "action must be A1..H8 or pass")
        }
    }
}
impl From<Square> for Action {
    fn from(square: Square) -> Self {
        Self::Place(square)
    }
}

/// 局面の合法手など、盤面上のマス集合を表す 64 ビットマスクのラッパー型です。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Moves(pub(crate) u64);
impl Moves {
    /// 生の `u64` ビットマスクを返します。A1 が最下位ビット（LSB）に対応します。
    pub const fn bits(self) -> u64 {
        self.0
    }
    /// 集合に含まれるマス数を返します。
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }
    /// 集合が空かどうかを返します。
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    /// 指定したマスが集合に含まれるかを返します。
    pub const fn contains(self, square: Square) -> bool {
        self.0 & square.bit() != 0
    }
}
impl Iterator for Moves {
    type Item = Square;
    fn next(&mut self) -> Option<Square> {
        if self.0 == 0 {
            return None;
        }
        let square = Square::lowest(self.0);
        // `x & (x - 1)` で最下位の立っているビットだけを 0 にして次の候補へ進みます。
        self.0 &= self.0 - 1;
        Some(square)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        // `self.len()` ではなく直接 `count_ones()` を呼びます。
        // `&mut Moves` 上で `len()` を呼ぶと `ExactSizeIterator::len` に解決され、
        // それがこの `size_hint` を呼び出すため無限再帰になるのを防ぐためです。
        let len = self.0.count_ones() as usize;
        (len, Some(len))
    }
}
impl ExactSizeIterator for Moves {}
