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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Place(Square),
    Pass,
}
