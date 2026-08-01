use std::sync::LazyLock;

pub static EDGE_STABILITY: LazyLock<[u8; 256 * 256]> = LazyLock::new(|| {
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

pub fn get_stable_edge(player: u64, opponent: u64) -> u64 {
    EDGE_STABILITY[((player & 0xff) * 256 + (opponent & 0xff)) as usize] as u64
        | ((EDGE_STABILITY[(((player >> 56) & 0xff) * 256 + ((opponent >> 56) & 0xff)) as usize]
            as u64)
            << 56)
        | unpack_a2a7(EDGE_STABILITY[pack_a1a8(player) * 256 + pack_a1a8(opponent)])
        | unpack_h2h7(EDGE_STABILITY[pack_h1h8(player) * 256 + pack_h1h8(opponent)])
}

pub fn get_full_lines(disc: u64, full: &mut [u64; 4]) -> u64 {
    let (mut h, mut v, mut l7, mut l9, mut r7, mut r9) = (disc, disc, disc, disc, disc, disc);

    h &= h >> 1;
    h &= h >> 2;
    h &= h >> 4;
    full[0] = (h & 0x0101_0101_0101_0101) * 0xff;

    v &= (v >> 8) | (v << 56);
    v &= (v >> 16) | (v << 48);
    v &= (v >> 32) | (v << 32);
    full[1] = v;

    l7 &= 0xff01_0101_0101_0101 | (l7 >> 7);
    r7 &= 0x8080_8080_8080_80ff | (r7 << 7);
    l7 &= 0xffff_0303_0303_0303 | (l7 >> 14);
    r7 &= 0xc0c0_c0c0_c0c0_ffff | (r7 << 14);
    l7 &= 0xffff_ffff_0f0f_0f0f | (l7 >> 28);
    r7 &= 0xf0f0_f0f0_ffff_ffff | (r7 << 28);
    full[2] = l7 & r7;

    l9 &= 0xff80_8080_8080_8080 | (l9 >> 9);
    r9 &= 0x0101_0101_0101_01ff | (r9 << 9);
    l9 &= 0xffff_c0c0_c0c0_c0c0 | (l9 >> 18);
    r9 &= 0x0303_0303_0303_ffff | (r9 << 18);
    full[3] = l9 & r9 & (0x0f0f_0f0f_f0f0_f0f0 | (l9 >> 36) | (r9 << 36));

    full[0] & full[1] & full[2] & full[3]
}

pub fn get_stable_by_contact(central_mask: u64, previous_stable: u64, full: &[u64; 4]) -> u64 {
    let mut old_stable = 0;
    let mut stable = previous_stable;
    while stable != old_stable {
        old_stable = stable;
        let stable_h = (stable >> 1) | (stable << 1) | full[0];
        let stable_v = (stable >> 8) | (stable << 8) | full[1];
        let stable_d7 = (stable >> 7) | (stable << 7) | full[2];
        let stable_d9 = (stable >> 9) | (stable << 9) | full[3];
        stable |= stable_h & stable_v & stable_d7 & stable_d9 & central_mask;
    }
    stable
}

pub fn get_stable_discs(player: u64, opponent: u64) -> u64 {
    let disc = player | opponent;
    let central_mask = player & 0x007e_7e7e_7e7e_7e00;
    let mut full = [0; 4];
    let mut stable = get_stable_edge(player, opponent);
    stable |= get_full_lines(disc, &mut full) & central_mask;
    get_stable_by_contact(central_mask, stable, &full)
}

pub fn get_stability(player: u64, opponent: u64) -> i32 {
    get_stable_discs(player, opponent).count_ones() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::board::Board;
    use crate::search::final_search::solve_score::solve_score;

    #[test]
    fn edge_stability_basic_patterns() {
        assert_eq!(EDGE_STABILITY[0], 0);
        assert_eq!(EDGE_STABILITY[0x01 * 256], 0x01);
        assert_eq!(EDGE_STABILITY[0x80 * 256], 0x80);
        assert_eq!(EDGE_STABILITY[0xff * 256], 0xff);
        assert_eq!(EDGE_STABILITY[0x7e * 256], 0x00);
        assert_eq!(EDGE_STABILITY[0x81 * 256], 0x81);
    }

    fn next_pseudo_random(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
    }

    fn make_board_with_empties(rng: &mut u64, n_empties: u32) -> Board {
        let mut empties = 0u64;
        while empties.count_ones() < n_empties {
            let pos = (next_pseudo_random(rng) % 64) as u32;
            empties |= 1u64 << pos;
        }
        let player = next_pseudo_random(rng) & !empties;
        Board {
            player,
            opponent: !player & !empties,
        }
    }

    fn brute_force(board: &Board) -> i32 {
        let moves = board.moves();
        if moves == 0 {
            if board.opponent_moves() == 0 {
                return solve_score(board);
            }
            return -brute_force(&board.passed());
        }
        let mut best = -64;
        let mut bits = moves;
        while bits != 0 {
            let m = bits & bits.wrapping_neg();
            bits ^= m;
            best = best.max(-brute_force(&board.make_move(m)));
        }
        best
    }

    #[test]
    fn opponent_stability_gives_score_upper_bound() {
        let mut rng = 0x57ab_2e1d_8614_d3c0;
        for _ in 0..200 {
            let n_empties = 6 + (next_pseudo_random(&mut rng) % 4) as u32;
            let board = make_board_with_empties(&mut rng, n_empties);
            let upper = 64 - 2 * get_stability(board.opponent, board.player);
            assert!(
                brute_force(&board) <= upper,
                "player={:#018x} opponent={:#018x} upper={upper}",
                board.player,
                board.opponent
            );
        }
    }
}
