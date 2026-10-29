"""Independent coordinate-rule exhaustive check of the tiny v1 solved fixture.

Usage: python3 verify_tiny_book.py tiny.dbook
Not a production book reader. Refuses large files and positions above 8 empties.
"""
import hashlib
import io
import struct
import sys
from functools import lru_cache
from pathlib import Path

DIRECTIONS = [(x, y) for x in (-1, 0, 1) for y in (-1, 0, 1) if x or y]


def flips(player, opponent, square):
    if (player | opponent) >> square & 1:
        return 0
    result = 0
    for dx, dy in DIRECTIONS:
        x, y = square % 8 + dx, square // 8 + dy
        ray = 0
        while 0 <= x < 8 and 0 <= y < 8 and opponent >> (y * 8 + x) & 1:
            ray |= 1 << (y * 8 + x)
            x, y = x + dx, y + dy
        if 0 <= x < 8 and 0 <= y < 8 and player >> (y * 8 + x) & 1:
            result |= ray
    return result


def moves(player, opponent):
    return [(s, f) for s in range(64) if (f := flips(player, opponent, s))]


def child(player, opponent, square, flipped):
    return opponent ^ flipped, player ^ flipped ^ (1 << square)


@lru_cache(maxsize=100000)
def solve(player, opponent):
    assert 64 - (player | opponent).bit_count() <= 8
    legal = moves(player, opponent)
    if legal:
        return max(-solve(*child(player, opponent, s, f)) for s, f in legal)
    if moves(opponent, player):
        return -solve(opponent, player)
    diff = player.bit_count() - opponent.bit_count()
    return diff + ((diff > 0) - (diff < 0)) * (64 - (player | opponent).bit_count())


data = Path(sys.argv[1]).read_bytes()
assert len(data) <= 1024 * 1024
assert data[:8] == b'DFTBK001'
assert struct.unpack('<IIQ', data[8:24]) == (1, 1, len(data) - 56)
assert hashlib.sha256(data[56:]).digest() == data[24:56]
stream = io.BytesIO(data[56:])
stream.read(32)
stream.read(struct.unpack('<I', stream.read(4))[0])
positions, candidates = struct.unpack('<QQ', stream.read(16))


def value():
    if stream.read(1) == b'\0':
        return None
    return struct.unpack('<hBBBBB', stream.read(7))


checked = 0
for _ in range(positions):
    p, o = struct.unpack('<QQ', stream.read(16))
    count, complete = struct.unpack('<BB', stream.read(2))
    v = value()
    assert v is not None and complete and v[3] == 1 and v[0] == solve(p, o)
    for _ in range(count):
        action, registered = struct.unpack('<BB', stream.read(2))
        v = value()
        assert v is not None and v[3] == 1
        expected = -solve(o, p) if action == 64 else -solve(*child(p, o, action, flips(p, o, action)))
        assert v[0] == expected
        checked += 1
assert checked == candidates
print(f'PASS: {positions} positions, {checked} candidate scores match exhaustive coordinate search')
