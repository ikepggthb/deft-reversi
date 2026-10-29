//! Edge stability table and enumeration ported from the reference engine.

/// Built at compile time so no search pays for its initialization.
static EDGE_STABILITY: [u8; 256 * 256] = edge_stability_table();

/// One edge move by the player (`by_player`) or the opponent, with the
/// reference engine's one-dimensional flips.
const fn edge_move(mut p: u32, mut o: u32, x: u32, by_player: bool) -> (u32, u32) {
    let (mover, other) = if by_player {
        (&mut p, &mut o)
    } else {
        (&mut o, &mut p)
    };
    *mover |= 1 << x;
    if x > 1 {
        let mut y = x - 1;
        while y > 0 && *other & (1 << y) != 0 {
            y -= 1;
        }
        if *mover & (1 << y) != 0 {
            let mut y = x - 1;
            while y > 0 && *other & (1 << y) != 0 {
                *other ^= 1 << y;
                *mover ^= 1 << y;
                y -= 1;
            }
        }
    }
    if x < 6 {
        let mut y = x + 1;
        while y < 8 && *other & (1 << y) != 0 {
            y += 1;
        }
        if *mover & (1 << y) != 0 {
            let mut y = x + 1;
            while y < 8 && *other & (1 << y) != 0 {
                *other ^= 1 << y;
                *mover ^= 1 << y;
                y += 1;
            }
        }
    }
    (p, o)
}

/// A player disc is stable when it stays the player's after every sequence
/// of edge moves. Children have one empty fewer, so filling the table in
/// order of empties computes the reference recursion's intersection once
/// per edge position.
const fn edge_stability_table() -> [u8; 256 * 256] {
    let mut table = [0u8; 256 * 256];
    let mut empties = 0;
    while empties <= 8 {
        let mut p = 0u32;
        while p < 256 {
            let mut o = 0u32;
            while o < 256 {
                let empty = !(p | o) & 0xff;
                if p & o == 0 && empty.count_ones() == empties {
                    let mut stable = p;
                    let mut x = 0;
                    while x < 8 {
                        if empty & (1 << x) != 0 {
                            let (cp, co) = edge_move(p, o, x, true);
                            stable &= table[(cp * 256 + co) as usize] as u32;
                            let (cp, co) = edge_move(p, o, x, false);
                            stable &= table[(cp * 256 + co) as usize] as u32;
                        }
                        x += 1;
                    }
                    table[(p * 256 + o) as usize] = stable as u8;
                }
                o += 1;
            }
            p += 1;
        }
        empties += 1;
    }
    table
}

#[cfg(test)]
#[inline(always)]
fn x_to_bit(x: i32) -> i32 {
    1 << x
}

#[cfg(test)]
fn find_edge_stable(old_p: i32, old_o: i32, mut stable: i32) -> i32 {
    let empties = !(old_p | old_o) & 0xff;

    stable &= old_p;
    if stable == 0 || empties == 0 {
        return stable;
    }

    for x in 0..8 {
        if empties & x_to_bit(x) != 0 {
            let mut o = old_o;
            let mut p = old_p | x_to_bit(x);
            if x > 1 {
                let mut y = x - 1;
                while y > 0 && (o & x_to_bit(y)) != 0 {
                    y -= 1;
                }
                if p & x_to_bit(y) != 0 {
                    y = x - 1;
                    while y > 0 && (o & x_to_bit(y)) != 0 {
                        o ^= x_to_bit(y);
                        p ^= x_to_bit(y);
                        y -= 1;
                    }
                }
            }
            if x < 6 {
                let mut y = x + 1;
                while y < 8 && (o & x_to_bit(y)) != 0 {
                    y += 1;
                }
                if p & x_to_bit(y) != 0 {
                    y = x + 1;
                    while y < 8 && (o & x_to_bit(y)) != 0 {
                        o ^= x_to_bit(y);
                        p ^= x_to_bit(y);
                        y += 1;
                    }
                }
            }
            stable = find_edge_stable(p, o, stable);
            if stable == 0 {
                return stable;
            }

            p = old_p;
            o = old_o | x_to_bit(x);
            if x > 1 {
                let mut y = x - 1;
                while y > 0 && (p & x_to_bit(y)) != 0 {
                    y -= 1;
                }
                if o & x_to_bit(y) != 0 {
                    y = x - 1;
                    while y > 0 && (p & x_to_bit(y)) != 0 {
                        o ^= x_to_bit(y);
                        p ^= x_to_bit(y);
                        y -= 1;
                    }
                }
            }
            if x < 6 {
                let mut y = x + 1;
                while y < 8 && (p & x_to_bit(y)) != 0 {
                    y += 1;
                }
                if o & x_to_bit(y) != 0 {
                    y = x + 1;
                    while y < 8 && (p & x_to_bit(y)) != 0 {
                        o ^= x_to_bit(y);
                        p ^= x_to_bit(y);
                        y += 1;
                    }
                }
            }
            stable = find_edge_stable(p, o, stable);
            if stable == 0 {
                return stable;
            }
        }
    }

    stable
}

#[inline(always)]
fn unpack_a2a7(x: u8) -> u64 {
    ((x as u64) & 0x7e).wrapping_mul(0x0000_0408_1020_4080) & 0x0001_0101_0101_0100
}

#[inline(always)]
fn unpack_h2h7(x: u8) -> u64 {
    ((x as u64) & 0x7e).wrapping_mul(0x0002_0408_1020_4000) & 0x0080_8080_8080_8000
}

#[inline(always)]
fn pack_a1a8(x: u64) -> usize {
    ((x & 0x0101_0101_0101_0101).wrapping_mul(0x0102_0408_1020_4080) >> 56) as usize
}

#[inline(always)]
fn pack_h1h8(x: u64) -> usize {
    ((x & 0x8080_8080_8080_8080).wrapping_mul(0x0002_0408_1020_4081) >> 56) as usize
}

pub(super) fn get_stable_edge(player: u64, opponent: u64) -> u64 {
    let table = &EDGE_STABILITY;
    table[((player & 0xff) * 256 + (opponent & 0xff)) as usize] as u64
        | ((table[(((player >> 56) & 0xff) * 256 + ((opponent >> 56) & 0xff)) as usize] as u64)
            << 56)
        | unpack_a2a7(table[pack_a1a8(player) * 256 + pack_a1a8(opponent)])
        | unpack_h2h7(table[pack_h1h8(player) * 256 + pack_h1h8(opponent)])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compile_time_table_matches_reference_recursion() {
        for p in 0..256 {
            for o in 0..256 {
                let expected = if p & o != 0 {
                    0
                } else {
                    find_edge_stable(p, o, p) as u8
                };
                assert_eq!(
                    EDGE_STABILITY[(p * 256 + o) as usize],
                    expected,
                    "p={p:#x} o={o:#x}"
                );
            }
        }
    }
}
