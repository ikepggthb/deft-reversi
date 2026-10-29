use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Square(u8);
impl Square {
    pub const fn new(index: u8) -> Option<Self> {
        if index < 64 { Some(Self(index)) } else { None }
    }
    /// The square of the lowest set bit of a non-empty mask.
    pub(crate) const fn lowest(mask: u64) -> Self {
        debug_assert!(mask != 0);
        Self(mask.trailing_zeros() as u8)
    }
    /// `x` is the file a..h and `y` the rank 1..8, both from zero.
    pub const fn from_xy(x: u8, y: u8) -> Option<Self> {
        if x < 8 && y < 8 {
            Some(Self(y * 8 + x))
        } else {
            None
        }
    }
    pub const fn x(self) -> u8 {
        self.0 % 8
    }
    pub const fn y(self) -> u8 {
        self.0 / 8
    }
    pub const fn index(self) -> u8 {
        self.0
    }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Place(Square),
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
    /// A square such as `f5`, or `pass`; case is ignored.
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

/// A set of squares, such as the legal moves of a position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Moves(pub(crate) u64);
impl Moves {
    /// A1 is the least significant bit.
    pub const fn bits(self) -> u64 {
        self.0
    }
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
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
        self.0 &= self.0 - 1;
        Some(square)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        // Not `self.len()`: on `&mut Moves` that is `ExactSizeIterator::len`,
        // which calls this function.
        let len = self.0.count_ones() as usize;
        (len, Some(len))
    }
}
impl ExactSizeIterator for Moves {}
