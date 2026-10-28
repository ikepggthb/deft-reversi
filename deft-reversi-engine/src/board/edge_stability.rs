//! Edge stability table and enumeration ported from the reference engine.
use std::sync::LazyLock;

static EDGE_STABILITY: LazyLock<[u8; 256 * 256]> = LazyLock::new(|| {
    let mut table = [0u8; 256 * 256];
    for p in 0..256 {
        for o in 0..256 {
            table[p * 256 + o] = if p & o != 0 {
                0
            } else {
                find_edge_stable(p as i32, o as i32, p as i32) as u8
            };
        }
    }
    table
});

#[inline(always)]
fn x_to_bit(x: i32) -> i32 {
    1 << x
}

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
    // Resolve LazyLock once, then use the immutable table for all four edges.
    let table = &*EDGE_STABILITY;
    table[((player & 0xff) * 256 + (opponent & 0xff)) as usize] as u64
        | ((table[(((player >> 56) & 0xff) * 256 + ((opponent >> 56) & 0xff)) as usize] as u64)
            << 56)
        | unpack_a2a7(table[pack_a1a8(player) * 256 + pack_a1a8(opponent)])
        | unpack_h2h7(table[pack_h1h8(player) * 256 + pack_h1h8(opponent)])
}
