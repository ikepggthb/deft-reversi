#!/usr/bin/env python3
"""Prove a structural alias in deft's current local-TT hash; no timing claims."""
import json
from pathlib import Path

MASK = (1 << 64) - 1


def rotate32(x):
    return ((x << 32) | (x >> 32)) & MASK


def deft_key(p, o):
    return p ^ rotate32(o)


def deft_index(p, o):
    return ((deft_key(p, o) * 0x9e3779b97f4a7c15) & MASK) >> 53


def egar_index(p, o):
    return (((p * 0x9dda1c54cfe6b6e9) & MASK)
            ^ ((o * 0xa2e6c0300831e05a) & MASK)) >> 54


def main():
    source = (Path(__file__).resolve().parents[3]
              / 'deft-reversi-engine/src/search/tail.rs').read_text()
    for token in ['board.player_bits() ^ board.opponent_bits().rotate_left(32)',
                  '.wrapping_mul(0x9e3779b97f4a7c15)', '(hash >> 53) as u16']:
        assert token in source, 'hash changed; update this reproduction'
    empty = 0x3f
    p = (0x5555555555555555 & ~empty) & ~(1 << 52)
    o = MASK ^ (p | empty)
    delta = (1 << 20) | (1 << 52)
    p2, o2 = p ^ delta, o ^ delta
    assert delta == rotate32(delta)
    assert p & o == p2 & o2 == 0
    assert (p | o) == (p2 | o2) == (MASK ^ empty)
    assert p.bit_count() == p2.bit_count()
    assert deft_key(p, o) == deft_key(p2, o2)
    assert deft_index(p, o) == deft_index(p2, o2)
    assert egar_index(p, o) != egar_index(p2, o2)
    print(json.dumps({
        'empties': empty.bit_count(),
        'board_a': {'player': f'{p:016x}', 'opponent': f'{o:016x}'},
        'board_b': {'player': f'{p2:016x}', 'opponent': f'{o2:016x}'},
        'changed_squares_zero_based': [20, 52],
        'same_player_disc_count': p.bit_count(),
        'equal_full_pre_multiply_keys': f'{deft_key(p, o):016x}',
        'deft_indices': [deft_index(p, o), deft_index(p2, o2)],
        'egar_indices': [egar_index(p, o), egar_index(p2, o2)],
        'scope': 'Constructed valid disjoint bitboards, not a proof of reachability or collision frequency in actual search. Egar comparison uses its enabled multiplication path, not disabled CRC32C.'
    }, indent=2))


if __name__ == '__main__':
    main()
