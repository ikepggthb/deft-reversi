#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import random
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable

import numpy as np
import torch
from torch import nn
from torch.utils.data import ConcatDataset, DataLoader, Dataset, Subset


RD_MAGIC = b"RDGBBVAL1\n"
RD_RECORD_SIZE = 18
RD_DTYPE = np.dtype([("own", "<u8"), ("opponent", "<u8"), ("value", "<i2")])

N_BOARD_SQUARES = 64
NNUE_ACTIVATION_SCALE = 4096
NNUE_ACTIVATION_MAX = NNUE_ACTIVATION_SCALE - 1
NNUE_DEFAULT_WEIGHT_SCALE = 4096
NNUE_DENSE_LAYER_SIZES = [32, 32]

NNUE_STONE_FEATURES = N_BOARD_SQUARES * 2
NNUE_LEGAL_MOVE_FEATURES = N_BOARD_SQUARES * 2
NNUE_QUADRANT_COUNT = 4
NNUE_QUADRANT_BUCKETS = 17
NNUE_QUADRANT_FEATURES = NNUE_QUADRANT_COUNT * NNUE_QUADRANT_BUCKETS
NNUE_QUADRANT_PARITY_BUCKETS = 2
NNUE_QUADRANT_PARITY_FEATURES = NNUE_QUADRANT_COUNT * NNUE_QUADRANT_PARITY_BUCKETS
NNUE_GLOBAL_PARITY_FEATURES = 2
NNUE_TOWER_COUNT = 8

NNUE_STONE_OFFSET = 0
NNUE_LEGAL_MOVE_OFFSET = NNUE_STONE_OFFSET + NNUE_STONE_FEATURES
NNUE_QUADRANT_OFFSET = NNUE_LEGAL_MOVE_OFFSET + NNUE_LEGAL_MOVE_FEATURES
NNUE_QUADRANT_PARITY_OFFSET = NNUE_QUADRANT_OFFSET + NNUE_QUADRANT_FEATURES
NNUE_GLOBAL_PARITY_OFFSET = NNUE_QUADRANT_PARITY_OFFSET + NNUE_QUADRANT_PARITY_FEATURES
NNUE_PATTERN_OFFSET = NNUE_GLOBAL_PARITY_OFFSET + NNUE_GLOBAL_PARITY_FEATURES

MASK64 = (1 << 64) - 1
HORIZONTAL_MASK = 0x7E7E7E7E7E7E7E7E
PAD_FEATURE_ID = -1


@dataclass(frozen=True)
class NnueConfig:
    acc_size: int = 256
    pattern_set: str = "v2"
    share_rotations: bool = False


NNUE_CONFIG = NnueConfig()


def square(x: int, y: int) -> int:
    return y * 8 + x


A1, B1, C1, D1, E1, F1, G1, H1 = [square(x, 0) for x in range(8)]
A2, B2, C2, D2, E2, F2, G2, H2 = [square(x, 1) for x in range(8)]
A3, B3, C3, D3, E3, F3, G3, H3 = [square(x, 2) for x in range(8)]
A4, B4, C4, D4, E4, F4, G4, H4 = [square(x, 3) for x in range(8)]
A5, B5, C5, D5, E5, F5, G5, H5 = [square(x, 4) for x in range(8)]
A6, B6, C6, D6, E6, F6, G6, H6 = [square(x, 5) for x in range(8)]
A7, B7, C7, D7, E7, F7, G7, H7 = [square(x, 6) for x in range(8)]
A8, B8, C8, D8, E8, F8, G8, H8 = [square(x, 7) for x in range(8)]
NO_COORD = 64


def rotate_square(pos: int, turns: int) -> int:
    result = pos
    for _ in range(turns):
        x = result % 8
        y = result // 8
        result = x * 8 + (7 - y)
    return result


def rot(n_squares: int, squares: list[int], turns: int) -> tuple[int, list[int]]:
    return n_squares, [rotate_square(pos, turns) for pos in squares[:n_squares]]


BASE_PATTERNS: list[tuple[int, list[int], int]] = [
    (8, [C1, D1, E1, F1, C2, D2, E2, F2], 4),
    (8, [A1, B1, C1, D1, E1, F1, G1, H1], 4),
    (8, [A2, B2, C2, D2, E2, F2, G2, H2], 4),
    (8, [A3, B3, C3, D3, E3, F3, G3, H3], 4),
    (8, [A4, B4, C4, D4, E4, F4, G4, H4], 4),
    (9, [A1, B1, C1, A2, B2, C2, A3, B3, C3], 4),
    (4, [D1, C2, B3, A4], 4),
    (5, [E1, D2, C3, B4, A5], 4),
    (6, [F1, E2, D3, C4, B5, A6], 4),
    (7, [G1, F2, E3, D4, C5, B6, A7], 4),
]


def selected_base_patterns(pattern_set: str) -> list[tuple[int, list[int], int]]:
    if pattern_set == "v2":
        return BASE_PATTERNS
    if pattern_set == "no-corner2x5":
        return BASE_PATTERNS
    if pattern_set == "v1":
        return BASE_PATTERNS
    raise ValueError(f"unsupported pattern_set: {pattern_set}")


def build_pattern_instances(pattern_set: str = "v2") -> list[tuple[int, list[int], int]]:
    instances: list[tuple[int, list[int], int]] = []
    for pattern_type, (n_squares, squares, rotations) in enumerate(selected_base_patterns(pattern_set)):
        for turns in range(rotations):
            n_rot, squares_rot = rot(n_squares, squares, turns)
            instances.append((n_rot, squares_rot, pattern_type))
    instances.append((8, [A1, B2, C3, D4, E5, F6, G7, H8], 10))
    instances.append((8, [H1, G2, F3, E4, D5, C6, B7, A8], 10))
    if pattern_set in {"v2", "no-corner2x5"}:
        for pattern_type, (n_squares, squares, rotations) in enumerate(
            [
                (10, [A1, B1, C1, D1, E1, F1, G1, H1, B2, G2], 4),
                (10, [A1, B1, C1, D1, E1, A2, B2, C2, D2, E2], 4),
                (10, [A1, A2, A3, A4, A5, B1, B2, B3, B4, B5], 4),
            ],
            start=11,
        ):
            if pattern_set == "no-corner2x5" and pattern_type in {12, 13}:
                continue
            for turns in range(rotations):
                n_rot, squares_rot = rot(n_squares, squares, turns)
                instances.append((n_rot, squares_rot, pattern_type))
    return instances


POW3 = [1]
for _ in range(16):
    POW3.append(POW3[-1] * 3)
BIT_SHIFTS = np.arange(N_BOARD_SQUARES, dtype=np.uint64)
QUADRANT_SQUARES = [
    np.array([square(x, y) for y in range(0, 4) for x in range(0, 4)], dtype=np.int64),
    np.array([square(x, y) for y in range(0, 4) for x in range(4, 8)], dtype=np.int64),
    np.array([square(x, y) for y in range(4, 8) for x in range(0, 4)], dtype=np.int64),
    np.array([square(x, y) for y in range(4, 8) for x in range(4, 8)], dtype=np.int64),
]

QUADRANT_MASKS = [
    sum(1 << square(x, y) for y in range(0, 4) for x in range(0, 4)),
    sum(1 << square(x, y) for y in range(0, 4) for x in range(4, 8)),
    sum(1 << square(x, y) for y in range(4, 8) for x in range(0, 4)),
    sum(1 << square(x, y) for y in range(4, 8) for x in range(4, 8)),
]


def configure_nnue(config: NnueConfig) -> None:
    global NNUE_CONFIG, NNUE_ACCUMULATOR_SIZE, NNUE_PAIRWISE_SIZE_PER_VIEW, NNUE_DENSE_INPUT_SIZE
    global PATTERN_INSTANCES, PATTERN_INSTANCE_OFFSETS, NNUE_PATTERN_FEATURES, NNUE_INPUT_SIZE
    global PADDING_FEATURE_ID, NNUE_MAX_ACTIVE_FEATURES_PER_VIEW, MAX_PATTERN_SQUARES
    global BITBOARD_FEATURE_COLUMNS, NNUE_BITBOARD_FEATURE_SLOTS, PATTERN_SQUARE_MATRIX
    global PATTERN_COEFF_MATRIX, PATTERN_REVERSE_COEFF_MATRIX, PATTERN_CANONICALIZE_REVERSE
    global PATTERN_OFFSET_ARRAY

    if config.acc_size not in {64, 128, 256}:
        raise ValueError(f"acc_size must be one of 64,128,256: {config.acc_size}")
    NNUE_CONFIG = config
    NNUE_ACCUMULATOR_SIZE = config.acc_size
    NNUE_PAIRWISE_SIZE_PER_VIEW = NNUE_ACCUMULATOR_SIZE // 2
    NNUE_DENSE_INPUT_SIZE = NNUE_PAIRWISE_SIZE_PER_VIEW * 2

    PATTERN_INSTANCES = build_pattern_instances(config.pattern_set)
    PATTERN_INSTANCE_OFFSETS = []
    type_offsets: dict[int, int] = {}
    pattern_offset = 0
    for n_squares, _, pattern_type in PATTERN_INSTANCES:
        if config.share_rotations:
            if pattern_type not in type_offsets:
                type_offsets[pattern_type] = pattern_offset
                pattern_offset += POW3[n_squares]
            PATTERN_INSTANCE_OFFSETS.append(type_offsets[pattern_type])
        else:
            PATTERN_INSTANCE_OFFSETS.append(pattern_offset)
            pattern_offset += POW3[n_squares]
    NNUE_PATTERN_FEATURES = pattern_offset
    NNUE_INPUT_SIZE = NNUE_PATTERN_OFFSET + NNUE_PATTERN_FEATURES
    PADDING_FEATURE_ID = NNUE_INPUT_SIZE
    NNUE_MAX_ACTIVE_FEATURES_PER_VIEW = 192 + 4 + 4 + 1 + len(PATTERN_INSTANCES)

    MAX_PATTERN_SQUARES = max(n_squares for n_squares, _, _ in PATTERN_INSTANCES)
    BITBOARD_FEATURE_COLUMNS = NNUE_STONE_OFFSET + np.arange(N_BOARD_SQUARES, dtype=np.int64)
    BITBOARD_FEATURE_COLUMNS = np.concatenate(
        [
            BITBOARD_FEATURE_COLUMNS,
            NNUE_STONE_OFFSET + N_BOARD_SQUARES + np.arange(N_BOARD_SQUARES, dtype=np.int64),
            NNUE_LEGAL_MOVE_OFFSET + np.arange(N_BOARD_SQUARES, dtype=np.int64),
            NNUE_LEGAL_MOVE_OFFSET + N_BOARD_SQUARES + np.arange(N_BOARD_SQUARES, dtype=np.int64),
        ]
    )
    NNUE_BITBOARD_FEATURE_SLOTS = (
        NNUE_MAX_ACTIVE_FEATURES_PER_VIEW
        - NNUE_QUADRANT_COUNT
        - NNUE_QUADRANT_COUNT
        - 1
        - len(PATTERN_INSTANCES)
    )
    PATTERN_SQUARE_MATRIX = np.zeros((len(PATTERN_INSTANCES), MAX_PATTERN_SQUARES), dtype=np.int64)
    PATTERN_COEFF_MATRIX = np.zeros((len(PATTERN_INSTANCES), MAX_PATTERN_SQUARES), dtype=np.int64)
    PATTERN_REVERSE_COEFF_MATRIX = np.zeros((len(PATTERN_INSTANCES), MAX_PATTERN_SQUARES), dtype=np.int64)
    PATTERN_CANONICALIZE_REVERSE = np.zeros(len(PATTERN_INSTANCES), dtype=bool)
    for pattern_idx, (n_squares, squares, pattern_type) in enumerate(PATTERN_INSTANCES):
        PATTERN_SQUARE_MATRIX[pattern_idx, :n_squares] = np.array(squares[:n_squares], dtype=np.int64)
        PATTERN_COEFF_MATRIX[pattern_idx, :n_squares] = np.array(
            [POW3[n_squares - 1 - idx] for idx in range(n_squares)],
            dtype=np.int64,
        )
        PATTERN_REVERSE_COEFF_MATRIX[pattern_idx, :n_squares] = np.array(
            [POW3[idx] for idx in range(n_squares)],
            dtype=np.int64,
        )
        PATTERN_CANONICALIZE_REVERSE[pattern_idx] = config.share_rotations and pattern_type == 10
    PATTERN_OFFSET_ARRAY = np.array(PATTERN_INSTANCE_OFFSETS, dtype=np.int64)


configure_nnue(NnueConfig())
assert len(PATTERN_INSTANCES) == 54
assert NNUE_PATTERN_FEATURES == 944_622
assert NNUE_INPUT_SIZE == 944_956
assert NNUE_MAX_ACTIVE_FEATURES_PER_VIEW == 255


def transform_square(pos: int, transform: int) -> int:
    x = pos % 8
    y = pos // 8
    if transform >= 4:
        x = 7 - x
    for _ in range(transform & 3):
        x, y = 7 - y, x
    return square(x, y)


SYMMETRY_MAPS = np.array(
    [[transform_square(pos, transform) for pos in range(64)] for transform in range(8)],
    dtype=np.uint64,
)


def transform_bitboards_np(boards: np.ndarray, transforms: np.ndarray) -> np.ndarray:
    boards = boards.astype(np.uint64, copy=False)
    output = np.zeros_like(boards, dtype=np.uint64)
    bits = bit_matrix(boards)
    for transform in range(8):
        rows = transforms == transform
        if not np.any(rows):
            continue
        mapped = SYMMETRY_MAPS[transform]
        shifted = (bits[rows].astype(np.uint64) << mapped[None, :])
        output[rows] = np.bitwise_or.reduce(shifted, axis=1)
    return output


def u64(value: int) -> int:
    return value & MASK64


def get_moves(player: int, opponent: int) -> int:
    p = u64(player)
    o = u64(opponent)
    m_o = o & HORIZONTAL_MASK

    flip7 = m_o & u64(p << 7)
    flip9 = m_o & u64(p << 9)
    flip8 = o & u64(p << 8)
    flip1 = m_o & u64(p << 1)

    flip7 |= m_o & u64(flip7 << 7)
    flip9 |= m_o & u64(flip9 << 9)
    flip8 |= o & u64(flip8 << 8)
    moves = u64(m_o + flip1)

    pre7 = m_o & u64(m_o << 7)
    pre9 = m_o & u64(m_o << 9)
    pre8 = o & u64(o << 8)

    flip7 |= pre7 & u64(flip7 << 14)
    flip9 |= pre9 & u64(flip9 << 18)
    flip8 |= pre8 & u64(flip8 << 16)

    flip7 |= pre7 & u64(flip7 << 14)
    flip9 |= pre9 & u64(flip9 << 18)
    flip8 |= pre8 & u64(flip8 << 16)

    moves |= u64(flip7 << 7)
    moves |= u64(flip9 << 9)
    moves |= u64(flip8 << 8)

    flip7 = m_o & (p >> 7)
    flip9 = m_o & (p >> 9)
    flip8 = o & (p >> 8)
    flip1 = m_o & (p >> 1)

    flip7 |= m_o & (flip7 >> 7)
    flip9 |= m_o & (flip9 >> 9)
    flip8 |= o & (flip8 >> 8)
    flip1 |= m_o & (flip1 >> 1)

    pre7 >>= 7
    pre9 >>= 9
    pre8 >>= 8
    pre1 = m_o & (m_o >> 1)

    flip7 |= pre7 & (flip7 >> 14)
    flip9 |= pre9 & (flip9 >> 18)
    flip8 |= pre8 & (flip8 >> 16)
    flip1 |= pre1 & (flip1 >> 2)

    flip7 |= pre7 & (flip7 >> 14)
    flip9 |= pre9 & (flip9 >> 18)
    flip8 |= pre8 & (flip8 >> 16)
    flip1 |= pre1 & (flip1 >> 2)

    moves |= flip7 >> 7
    moves |= flip9 >> 9
    moves |= flip8 >> 8
    moves |= flip1 >> 1
    return u64(moves & ~(p | o))


def get_moves_np(player: np.ndarray, opponent: np.ndarray) -> np.ndarray:
    p = player.astype(np.uint64, copy=False)
    o = opponent.astype(np.uint64, copy=False)
    m_o = o & np.uint64(HORIZONTAL_MASK)

    flip7 = m_o & (p << np.uint64(7))
    flip9 = m_o & (p << np.uint64(9))
    flip8 = o & (p << np.uint64(8))
    flip1 = m_o & (p << np.uint64(1))

    flip7 |= m_o & (flip7 << np.uint64(7))
    flip9 |= m_o & (flip9 << np.uint64(9))
    flip8 |= o & (flip8 << np.uint64(8))
    moves = m_o + flip1

    pre7 = m_o & (m_o << np.uint64(7))
    pre9 = m_o & (m_o << np.uint64(9))
    pre8 = o & (o << np.uint64(8))

    flip7 |= pre7 & (flip7 << np.uint64(14))
    flip9 |= pre9 & (flip9 << np.uint64(18))
    flip8 |= pre8 & (flip8 << np.uint64(16))

    flip7 |= pre7 & (flip7 << np.uint64(14))
    flip9 |= pre9 & (flip9 << np.uint64(18))
    flip8 |= pre8 & (flip8 << np.uint64(16))

    moves |= flip7 << np.uint64(7)
    moves |= flip9 << np.uint64(9)
    moves |= flip8 << np.uint64(8)

    flip7 = m_o & (p >> np.uint64(7))
    flip9 = m_o & (p >> np.uint64(9))
    flip8 = o & (p >> np.uint64(8))
    flip1 = m_o & (p >> np.uint64(1))

    flip7 |= m_o & (flip7 >> np.uint64(7))
    flip9 |= m_o & (flip9 >> np.uint64(9))
    flip8 |= o & (flip8 >> np.uint64(8))
    flip1 |= m_o & (flip1 >> np.uint64(1))

    pre7 >>= np.uint64(7)
    pre9 >>= np.uint64(9)
    pre8 >>= np.uint64(8)
    pre1 = m_o & (m_o >> np.uint64(1))

    flip7 |= pre7 & (flip7 >> np.uint64(14))
    flip9 |= pre9 & (flip9 >> np.uint64(18))
    flip8 |= pre8 & (flip8 >> np.uint64(16))
    flip1 |= pre1 & (flip1 >> np.uint64(2))

    flip7 |= pre7 & (flip7 >> np.uint64(14))
    flip9 |= pre9 & (flip9 >> np.uint64(18))
    flip8 |= pre8 & (flip8 >> np.uint64(16))
    flip1 |= pre1 & (flip1 >> np.uint64(2))

    moves |= flip7 >> np.uint64(7)
    moves |= flip9 >> np.uint64(9)
    moves |= flip8 >> np.uint64(8)
    moves |= flip1 >> np.uint64(1)
    return moves & ~(p | o)


def iter_bits(bitboard: int) -> Iterable[int]:
    bits = u64(bitboard)
    while bits:
        lsb = bits & -bits
        yield lsb.bit_length() - 1
        bits &= bits - 1


def append_bitboard_features(features: list[int], offset: int, bitboard: int) -> None:
    for pos in iter_bits(bitboard):
        features.append(offset + pos)


def pattern_code(instance: tuple[int, list[int], int], own: int, opponent: int) -> int:
    code = 0
    reverse_code = 0
    n_squares, squares, pattern_type = instance
    for idx, pos in enumerate(squares[:n_squares]):
        bit = 1 << pos
        if own & bit:
            digit = 1
        elif opponent & bit:
            digit = 2
        else:
            digit = 0
        code = code * 3 + digit
        reverse_code += digit * POW3[idx]
    if NNUE_CONFIG.share_rotations and pattern_type == 10:
        return min(code, reverse_code)
    return code


def active_features_for_view(own: int, opponent: int, own_moves: int, opponent_moves: int) -> list[int]:
    features: list[int] = []
    append_bitboard_features(features, NNUE_STONE_OFFSET, own)
    append_bitboard_features(features, NNUE_STONE_OFFSET + N_BOARD_SQUARES, opponent)
    append_bitboard_features(features, NNUE_LEGAL_MOVE_OFFSET, own_moves)
    append_bitboard_features(features, NNUE_LEGAL_MOVE_OFFSET + N_BOARD_SQUARES, opponent_moves)
    occupied = own | opponent
    total_empty = (N_BOARD_SQUARES - occupied.bit_count())
    for idx, mask in enumerate(QUADRANT_MASKS):
        empty_count = ((mask & ~occupied) & MASK64).bit_count()
        features.append(NNUE_QUADRANT_OFFSET + idx * NNUE_QUADRANT_BUCKETS + empty_count)
        features.append(NNUE_QUADRANT_PARITY_OFFSET + idx * NNUE_QUADRANT_PARITY_BUCKETS + (empty_count & 1))
    features.append(NNUE_GLOBAL_PARITY_OFFSET + (total_empty & 1))
    for idx, instance in enumerate(PATTERN_INSTANCES):
        features.append(NNUE_PATTERN_OFFSET + PATTERN_INSTANCE_OFFSETS[idx] + pattern_code(instance, own, opponent))
    return features


def active_feature_views(own: int, opponent: int) -> np.ndarray:
    own_moves = get_moves(own, opponent)
    opponent_moves = get_moves(opponent, own)
    views = [
        active_features_for_view(own, opponent, own_moves, opponent_moves),
        active_features_for_view(opponent, own, opponent_moves, own_moves),
    ]
    output = np.full((2, NNUE_MAX_ACTIVE_FEATURES_PER_VIEW), PADDING_FEATURE_ID, dtype=np.int64)
    for view_idx, features in enumerate(views):
        output[view_idx, : len(features)] = features
    return output


def bit_matrix(boards: np.ndarray) -> np.ndarray:
    return ((boards[:, None] >> BIT_SHIFTS) & np.uint64(1)).astype(bool, copy=False)


def active_feature_views_batch(own: np.ndarray, opponent: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    own = own.astype(np.uint64, copy=False)
    opponent = opponent.astype(np.uint64, copy=False)
    own_moves = get_moves_np(own, opponent)
    opponent_moves = get_moves_np(opponent, own)
    features = np.full(
        (own.shape[0], 2, NNUE_MAX_ACTIVE_FEATURES_PER_VIEW),
        PADDING_FEATURE_ID,
        dtype=np.int64,
    )

    own_bits = bit_matrix(own)
    opponent_bits = bit_matrix(opponent)
    own_move_bits = bit_matrix(own_moves)
    opponent_move_bits = bit_matrix(opponent_moves)
    occupied_bits = own_bits | opponent_bits
    empties = (N_BOARD_SQUARES - occupied_bits.sum(axis=1)).astype(np.int16, copy=False)

    fill_feature_view(
        features[:, 0, :],
        own_bits,
        opponent_bits,
        own_move_bits,
        opponent_move_bits,
        occupied_bits,
    )
    fill_feature_view(
        features[:, 1, :],
        opponent_bits,
        own_bits,
        opponent_move_bits,
        own_move_bits,
        occupied_bits,
    )
    return features, empties


def fill_feature_view(
    output: np.ndarray,
    own_bits: np.ndarray,
    opponent_bits: np.ndarray,
    own_move_bits: np.ndarray,
    opponent_move_bits: np.ndarray,
    occupied_bits: np.ndarray,
) -> None:
    pattern_own = own_bits[:, PATTERN_SQUARE_MATRIX]
    pattern_opponent = opponent_bits[:, PATTERN_SQUARE_MATRIX]
    pattern_digits = np.where(pattern_own, 1, np.where(pattern_opponent, 2, 0)).astype(np.int64, copy=False)
    pattern_codes = (pattern_digits * PATTERN_COEFF_MATRIX[None, :, :]).sum(axis=2)
    if np.any(PATTERN_CANONICALIZE_REVERSE):
        reverse_codes = (pattern_digits * PATTERN_REVERSE_COEFF_MATRIX[None, :, :]).sum(axis=2)
        pattern_codes = np.where(
            PATTERN_CANONICALIZE_REVERSE[None, :],
            np.minimum(pattern_codes, reverse_codes),
            pattern_codes,
        )
    pattern_features = NNUE_PATTERN_OFFSET + PATTERN_OFFSET_ARRAY[None, :] + pattern_codes

    quadrant_features = np.empty((own_bits.shape[0], NNUE_QUADRANT_COUNT), dtype=np.int64)
    quadrant_parity_features = np.empty((own_bits.shape[0], NNUE_QUADRANT_COUNT), dtype=np.int64)
    for idx, squares in enumerate(QUADRANT_SQUARES):
        empty_count = (~occupied_bits[:, squares]).sum(axis=1)
        quadrant_features[:, idx] = NNUE_QUADRANT_OFFSET + idx * NNUE_QUADRANT_BUCKETS + empty_count
        quadrant_parity_features[:, idx] = (
            NNUE_QUADRANT_PARITY_OFFSET
            + idx * NNUE_QUADRANT_PARITY_BUCKETS
            + (empty_count & 1)
        )
    total_empty = N_BOARD_SQUARES - occupied_bits.sum(axis=1)
    global_parity_features = (NNUE_GLOBAL_PARITY_OFFSET + (total_empty & 1)).astype(np.int64, copy=False)

    bitboard_mask = np.concatenate(
        [own_bits, opponent_bits, own_move_bits, opponent_move_bits],
        axis=1,
    )
    bitboard_features = np.where(
        bitboard_mask,
        BITBOARD_FEATURE_COLUMNS[None, :],
        PADDING_FEATURE_ID,
    )
    output[:, :NNUE_BITBOARD_FEATURE_SLOTS] = np.sort(bitboard_features, axis=1)[
        :, :NNUE_BITBOARD_FEATURE_SLOTS
    ]
    output[
        :, NNUE_BITBOARD_FEATURE_SLOTS : NNUE_BITBOARD_FEATURE_SLOTS + NNUE_QUADRANT_COUNT
    ] = quadrant_features
    cursor = NNUE_BITBOARD_FEATURE_SLOTS + NNUE_QUADRANT_COUNT
    output[:, cursor : cursor + NNUE_QUADRANT_COUNT] = quadrant_parity_features
    cursor += NNUE_QUADRANT_COUNT
    output[:, cursor] = global_parity_features
    cursor += 1
    output[:, cursor:] = pattern_features


def collate_nnue_records(
    batch: list[dict[str, object]],
    augment_symmetry: bool = False,
    augment_rotations_only: bool = False,
) -> dict[str, torch.Tensor]:
    own = np.fromiter((record["own"] for record in batch), dtype=np.uint64, count=len(batch))
    opponent = np.fromiter((record["opponent"] for record in batch), dtype=np.uint64, count=len(batch))
    target = np.fromiter((record["target"] for record in batch), dtype=np.float32, count=len(batch))
    if augment_symmetry:
        transform_count = 4 if augment_rotations_only else 8
        transforms = np.random.randint(0, transform_count, size=len(batch), dtype=np.uint8)
        own = transform_bitboards_np(own, transforms)
        opponent = transform_bitboards_np(opponent, transforms)
    features, empties = active_feature_views_batch(own, opponent)
    return {
        "features": torch.from_numpy(features),
        "target": torch.from_numpy(target),
        "empties": torch.from_numpy(empties),
    }


def make_collate_nnue_records(augment_symmetry: bool, augment_rotations_only: bool = False):
    def collate(batch: list[dict[str, object]]) -> dict[str, torch.Tensor]:
        return collate_nnue_records(
            batch,
            augment_symmetry=augment_symmetry,
            augment_rotations_only=augment_rotations_only,
        )

    return collate


class RdNnueDataset(Dataset[dict[str, object]]):
    def __init__(self, path: Path) -> None:
        self.path = path
        with path.open("rb") as file:
            magic = file.read(len(RD_MAGIC))
        if magic != RD_MAGIC:
            raise ValueError(f"{path} does not start with {RD_MAGIC!r}")
        payload_size = path.stat().st_size - len(RD_MAGIC)
        if payload_size % RD_RECORD_SIZE != 0:
            raise ValueError(f"{path} payload size is not a multiple of {RD_RECORD_SIZE}")
        self.records = np.memmap(
            path,
            dtype=RD_DTYPE,
            mode="r",
            offset=len(RD_MAGIC),
            shape=(payload_size // RD_RECORD_SIZE,),
        )

    def __len__(self) -> int:
        return len(self.records)

    def __getitem__(self, idx: int) -> dict[str, object]:
        record = self.records[idx]
        return {
            "own": np.uint64(record["own"]),
            "opponent": np.uint64(record["opponent"]),
            "target": np.float32(record["value"]),
        }


class TorchNnue(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.input_weights = nn.Embedding(
            NNUE_INPUT_SIZE + 1,
            NNUE_ACCUMULATOR_SIZE,
            padding_idx=PADDING_FEATURE_ID,
            sparse=True,
        )
        self.input_bias = nn.Parameter(torch.zeros(NNUE_ACCUMULATOR_SIZE))
        with torch.no_grad():
            nn.init.uniform_(self.input_weights.weight[:NNUE_INPUT_SIZE], -8.0, 8.0)
            self.input_weights.weight[PADDING_FEATURE_ID].zero_()

        towers: list[nn.ModuleDict] = []
        for _ in range(NNUE_TOWER_COUNT):
            layers: list[nn.Linear] = []
            input_size = NNUE_DENSE_INPUT_SIZE
            for output_size in NNUE_DENSE_LAYER_SIZES:
                layer = nn.Linear(input_size, output_size)
                nn.init.uniform_(layer.weight, -16.0, 16.0)
                nn.init.zeros_(layer.bias)
                layers.append(layer)
                input_size = output_size
            output = nn.Linear(input_size, 1)
            nn.init.uniform_(output.weight, -2.0, 2.0)
            nn.init.zeros_(output.bias)
            towers.append(nn.ModuleDict({"layers": nn.ModuleList(layers), "output": output}))
        self.towers = nn.ModuleList(towers)

    def forward(self, features: torch.Tensor, buckets: torch.Tensor) -> torch.Tensor:
        accumulator = self.input_weights(features).sum(dim=2) + self.input_bias.view(1, 1, -1)
        clipped = accumulator.clamp(0.0, float(NNUE_ACTIVATION_MAX))
        pairwise = (
            clipped[..., :NNUE_PAIRWISE_SIZE_PER_VIEW]
            * clipped[..., NNUE_PAIRWISE_SIZE_PER_VIEW:]
            / NNUE_ACTIVATION_SCALE
        )
        tower_input = pairwise.flatten(1)

        output = torch.empty(features.shape[0], device=features.device, dtype=tower_input.dtype)
        for bucket in range(NNUE_TOWER_COUNT):
            mask = buckets == bucket
            if not bool(mask.any()):
                continue
            activation = tower_input[mask]
            tower = self.towers[bucket]
            for layer in tower["layers"]:
                activation = (layer(activation) / NNUE_DEFAULT_WEIGHT_SCALE).clamp(
                    0.0,
                    float(NNUE_ACTIVATION_MAX),
                )
            output[mask] = tower["output"](activation).squeeze(1) / NNUE_DEFAULT_WEIGHT_SCALE
        return output


def parse_phase_range(value: str) -> tuple[int, int]:
    if ".." in value:
        start, end = value.split("..", 1)
    elif "-" in value:
        start, end = value.split("-", 1)
    else:
        start = end = value
    start_i = int(start)
    end_i = int(end)
    if start_i > end_i:
        raise argparse.ArgumentTypeError(f"invalid phase range: {value}")
    return start_i, end_i


def make_dataset(
    data_root: Path,
    split: str,
    phase_range: tuple[int, int],
    limit: int | None,
    seed: int,
) -> tuple[Dataset, list[tuple[int, int, int]]]:
    phase_datasets: list[tuple[int, RdNnueDataset]] = []
    for phase in range(phase_range[0], phase_range[1] + 1):
        path = data_root / f"phase_{phase}" / f"{split}.rd"
        if path.exists():
            phase_datasets.append((phase, RdNnueDataset(path)))
    if not phase_datasets:
        raise ValueError(f"no {split}.rd files found under {data_root}")

    datasets: list[Dataset] = []
    stats: list[tuple[int, int, int]] = []
    if limit is not None:
        allocations = allocate_phase_balanced_limit([len(dataset) for _, dataset in phase_datasets], limit)
        rng = random.Random(seed)
        for (phase, dataset), take in zip(phase_datasets, allocations):
            if take <= 0:
                continue
            if take >= len(dataset):
                datasets.append(dataset)
            else:
                indices = np.array(sorted(rng.sample(range(len(dataset)), take)), dtype=np.int64)
                datasets.append(Subset(dataset, indices))
            stats.append((phase, take, len(dataset)))
    else:
        for phase, dataset in phase_datasets:
            datasets.append(dataset)
            stats.append((phase, len(dataset), len(dataset)))
    return ConcatDataset(datasets), stats


def allocate_phase_balanced_limit(capacities: list[int], limit: int) -> list[int]:
    total = min(limit, sum(capacities))
    if total <= 0:
        return [0 for _ in capacities]
    base, remainder = divmod(total, len(capacities))
    allocations = [min(capacity, base + (1 if idx < remainder else 0)) for idx, capacity in enumerate(capacities)]
    remaining = total - sum(allocations)
    while remaining > 0:
        progressed = False
        for idx, capacity in enumerate(capacities):
            if allocations[idx] >= capacity:
                continue
            allocations[idx] += 1
            remaining -= 1
            progressed = True
            if remaining == 0:
                break
        if not progressed:
            break
    return allocations


def format_phase_stats(stats: list[tuple[int, int, int]]) -> str:
    return ",".join(f"{phase}:{take}/{total}" for phase, take, total in stats)


class MetricsLogger:
    def __init__(self, log_dir: Path | None) -> None:
        self.log_dir = log_dir
        self.csv_file = None
        self.jsonl_file = None
        self.csv_writer = None
        self.empty_csv_file = None
        self.empty_jsonl_file = None
        self.empty_csv_writer = None
        if log_dir is not None:
            log_dir.mkdir(parents=True, exist_ok=True)
            csv_path = log_dir / "metrics.csv"
            write_header = not csv_path.exists() or csv_path.stat().st_size == 0
            self.csv_file = csv_path.open("a", newline="")
            self.jsonl_file = (log_dir / "metrics.jsonl").open("a")
            fieldnames = [
                "kind",
                "evaluator",
                "epoch",
                "total_epochs",
                "batch",
                "total_batches",
                "step",
                "samples",
                "total_samples",
                "avg_loss",
                "disc_mae",
                "last_loss",
                "last_disc_mae",
                "valid_loss",
                "valid_disc_mae",
                "lr",
                "speed",
                "elapsed_seconds",
                "eta_epoch_seconds",
                "eta_total_seconds",
            ]
            self.csv_writer = csv.DictWriter(self.csv_file, fieldnames=fieldnames)
            if write_header:
                self.csv_writer.writeheader()

            empty_csv_path = log_dir / "valid_by_empties.csv"
            write_empty_header = not empty_csv_path.exists() or empty_csv_path.stat().st_size == 0
            self.empty_csv_file = empty_csv_path.open("a", newline="")
            self.empty_jsonl_file = (log_dir / "valid_by_empties.jsonl").open("a")
            empty_fieldnames = [
                "kind",
                "evaluator",
                "epoch",
                "total_epochs",
                "empties",
                "stones",
                "samples",
                "valid_loss",
                "valid_disc_mae",
            ]
            self.empty_csv_writer = csv.DictWriter(self.empty_csv_file, fieldnames=empty_fieldnames)
            if write_empty_header:
                self.empty_csv_writer.writeheader()

    def write(self, event: dict[str, object]) -> None:
        if self.csv_writer is None or self.csv_file is None or self.jsonl_file is None:
            return
        self.csv_writer.writerow(event)
        self.csv_file.flush()
        self.jsonl_file.write(json.dumps(event, separators=(",", ":")) + "\n")
        self.jsonl_file.flush()

    def write_empty_metrics(self, events: list[dict[str, object]]) -> None:
        if self.empty_csv_writer is None or self.empty_csv_file is None or self.empty_jsonl_file is None:
            return
        for event in events:
            self.empty_csv_writer.writerow(event)
            self.empty_jsonl_file.write(json.dumps(event, separators=(",", ":")) + "\n")
        self.empty_csv_file.flush()
        self.empty_jsonl_file.flush()

    def close(self) -> None:
        if self.csv_file is not None:
            self.csv_file.close()
        if self.jsonl_file is not None:
            self.jsonl_file.close()
        if self.empty_csv_file is not None:
            self.empty_csv_file.close()
        if self.empty_jsonl_file is not None:
            self.empty_jsonl_file.close()


def format_duration(seconds: float) -> str:
    seconds_i = max(0, int(seconds))
    hours, rem = divmod(seconds_i, 3600)
    minutes, secs = divmod(rem, 60)
    if hours:
        return f"{hours}h{minutes:02d}m{secs:02d}s"
    if minutes:
        return f"{minutes}m{secs:02d}s"
    return f"{secs}s"


def lr_multiplier(step: int, total_steps: int, warmup_steps: int, schedule: str, final_lr_ratio: float) -> float:
    if warmup_steps > 0 and step < warmup_steps:
        return max(1, step + 1) / warmup_steps
    if schedule == "constant":
        return 1.0
    denom = max(1, total_steps - warmup_steps)
    progress = min(1.0, max(0.0, (step - warmup_steps) / denom))
    cosine = 0.5 * (1.0 + np.cos(np.pi * progress))
    return float(final_lr_ratio + (1.0 - final_lr_ratio) * cosine)


def set_optimizer_lr(optimizers: list[torch.optim.Optimizer], lr: float) -> None:
    for optimizer in optimizers:
        for group in optimizer.param_groups:
            group["lr"] = lr


def run_epoch(
    *,
    model: TorchNnue,
    loader: DataLoader,
    device: torch.device,
    dense_optimizer: torch.optim.Optimizer | None,
    sparse_optimizer: torch.optim.Optimizer | None,
    huber_delta: float,
    epoch: int,
    total_epochs: int,
    total_seen_samples: int,
    global_step: int,
    progress_every_steps: int,
    logger: MetricsLogger,
    lr: float,
    total_steps: int,
    lr_schedule: str,
    warmup_steps: int,
    final_lr_ratio: float,
    collect_by_empties: bool = False,
) -> tuple[float, float, int, int, dict[int, list[float]]]:
    training = dense_optimizer is not None and sparse_optimizer is not None
    model.train(training)
    started = time.monotonic()
    last_logged_at = started
    last_logged_samples = 0
    loss_sum_tensor = torch.zeros((), device=device)
    abs_error_sum_tensor = torch.zeros((), device=device)
    samples = 0
    last_loss = 0.0
    last_mae = 0.0
    total_batches = max(1, len(loader))
    by_empties: dict[int, list[float]] = {}

    for batch_idx, batch in enumerate(loader, start=1):
        features = batch["features"].to(device, non_blocking=True)
        buckets = (batch["empties"].to(device, non_blocking=True).long() // 8).clamp(max=7)
        target = batch["target"].to(device, non_blocking=True)
        with torch.set_grad_enabled(training):
            pred = model(features, buckets)
            loss = torch.nn.functional.smooth_l1_loss(pred, target, beta=huber_delta)
            if training:
                effective_lr = lr * lr_multiplier(
                    global_step,
                    total_steps,
                    warmup_steps,
                    lr_schedule,
                    final_lr_ratio,
                )
                set_optimizer_lr([dense_optimizer, sparse_optimizer], effective_lr)
                dense_optimizer.zero_grad(set_to_none=True)
                sparse_optimizer.zero_grad(set_to_none=True)
                loss.backward()
                dense_optimizer.step()
                sparse_optimizer.step()
                global_step += 1

        with torch.no_grad():
            batch_size = int(target.numel())
            per_sample_loss = torch.nn.functional.smooth_l1_loss(
                pred,
                target,
                beta=huber_delta,
                reduction="none",
            )
            per_sample_abs_error = (pred - target).abs()
            batch_loss_tensor = per_sample_loss.sum()
            batch_abs_error_tensor = per_sample_abs_error.sum()
            loss_sum_tensor += batch_loss_tensor.detach()
            abs_error_sum_tensor += batch_abs_error_tensor.detach()
            samples += batch_size
            if collect_by_empties and "empties" in batch:
                empties_values = batch["empties"].detach().cpu().tolist()
                losses = per_sample_loss.detach().cpu().tolist()
                abs_errors = per_sample_abs_error.detach().cpu().tolist()
                for empties, sample_loss, sample_abs_error in zip(empties_values, losses, abs_errors):
                    bucket = by_empties.setdefault(int(empties), [0.0, 0.0, 0.0])
                    bucket[0] += float(sample_loss)
                    bucket[1] += float(sample_abs_error)
                    bucket[2] += 1.0

        if training and progress_every_steps > 0 and global_step % progress_every_steps == 0:
            effective_lr = dense_optimizer.param_groups[0]["lr"] if dense_optimizer is not None else lr
            now = time.monotonic()
            elapsed = now - started
            interval_elapsed = max(1e-9, now - last_logged_at)
            interval_samples = samples - last_logged_samples
            speed = interval_samples / interval_elapsed
            batches_per_sec = batch_idx / max(1e-9, elapsed)
            remaining_batches = max(0, total_batches - batch_idx)
            eta_epoch = remaining_batches / max(1e-9, batches_per_sec)
            eta_total = eta_epoch + total_batches * max(0, total_epochs - epoch) / max(1e-9, batches_per_sec)
            avg_loss = float((loss_sum_tensor / max(1, samples)).detach().cpu())
            disc_mae = float((abs_error_sum_tensor / max(1, samples)).detach().cpu())
            last_loss = float((batch_loss_tensor / batch_size).detach().cpu())
            last_mae = float((batch_abs_error_tensor / batch_size).detach().cpu())
            total_samples = total_seen_samples + samples
            print(
                f"epoch={epoch:03d}/{total_epochs:03d} train batch={batch_idx}/{total_batches} "
                f"step={global_step} samples={samples} total_samples={total_samples} "
                f"avg_loss={avg_loss:.6f} disc_mae={disc_mae:.3f} "
                f"last_loss={last_loss:.6f} last_disc_mae={last_mae:.3f} "
                f"lr={effective_lr:.6f} speed={speed:.1f}/s elapsed={format_duration(elapsed)} "
                f"eta_epoch={format_duration(eta_epoch)} eta_total={format_duration(eta_total)}",
                flush=True,
            )
            logger.write(
                {
                    "kind": "train",
                    "evaluator": "nnue_torch",
                    "epoch": epoch,
                    "total_epochs": total_epochs,
                    "batch": batch_idx,
                    "total_batches": total_batches,
                    "step": global_step,
                    "samples": samples,
                    "total_samples": total_samples,
                    "avg_loss": avg_loss,
                    "disc_mae": disc_mae,
                    "last_loss": last_loss,
                    "last_disc_mae": last_mae,
                    "valid_loss": "",
                    "valid_disc_mae": "",
                    "lr": effective_lr,
                    "speed": speed,
                    "elapsed_seconds": elapsed,
                    "eta_epoch_seconds": eta_epoch,
                    "eta_total_seconds": eta_total,
                }
            )
            last_logged_at = now
            last_logged_samples = samples

    loss_sum = float(loss_sum_tensor.detach().cpu())
    abs_error_sum = float(abs_error_sum_tensor.detach().cpu())
    return loss_sum / max(1, samples), abs_error_sum / max(1, samples), samples, global_step, by_empties


def empty_metric_events(
    *,
    by_empties: dict[int, list[float]],
    epoch: int,
    total_epochs: int,
) -> list[dict[str, object]]:
    events: list[dict[str, object]] = []
    for empties, (loss_sum, abs_error_sum, count) in sorted(by_empties.items()):
        samples = int(count)
        events.append(
            {
                "kind": "valid_by_empties",
                "evaluator": "nnue_torch",
                "epoch": epoch,
                "total_epochs": total_epochs,
                "empties": empties,
                "stones": 64 - empties,
                "samples": samples,
                "valid_loss": loss_sum / max(1, samples),
                "valid_disc_mae": abs_error_sum / max(1, samples),
            }
        )
    return events


def format_empty_metric_summary(events: list[dict[str, object]]) -> str:
    if not events:
        return ""
    return ",".join(
        f"{int(event['empties'])}:{float(event['valid_disc_mae']):.2f}({int(event['samples'])})"
        for event in events
    )


def quantized_array(tensor: torch.Tensor, dtype: np.dtype) -> np.ndarray:
    limits = np.iinfo(dtype)
    return (
        tensor.detach()
        .cpu()
        .round()
        .clamp(float(limits.min), float(limits.max))
        .to(torch.int32)
        .numpy()
        .astype(dtype)
    )


def write_int_vector(file, values: np.ndarray) -> None:
    file.write("[")
    flat = values.reshape(-1)
    chunk_size = 16384
    wrote = False
    for start in range(0, flat.size, chunk_size):
        if wrote:
            file.write(",")
        file.write(",".join(map(str, flat[start : start + chunk_size].astype(np.int64, copy=False).tolist())))
        wrote = True
    file.write("]")


def write_int_matrix(file, matrix: np.ndarray) -> None:
    file.write("[")
    for row_idx, row in enumerate(matrix):
        if row_idx:
            file.write(",")
        write_int_vector(file, row)
    file.write("]")


def export_nnue_json(model: TorchNnue, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    input_weights = quantized_array(model.input_weights.weight[:NNUE_INPUT_SIZE], np.int16)
    input_bias = quantized_array(model.input_bias, np.int32)
    with path.open("w") as file:
        file.write("{")
        file.write(f'"input_size":{NNUE_INPUT_SIZE},')
        file.write(f'"acc_size":{NNUE_ACCUMULATOR_SIZE},')
        file.write(f'"pattern_set":{json.dumps(NNUE_CONFIG.pattern_set)},')
        file.write(f'"share_rotations":{json.dumps(NNUE_CONFIG.share_rotations)},')
        file.write(f'"accumulator_size":{NNUE_ACCUMULATOR_SIZE},')
        file.write(f'"activation_scale":{NNUE_ACTIVATION_SCALE},')
        file.write(f'"weight_scale":{NNUE_DEFAULT_WEIGHT_SCALE},')
        file.write(f'"tower_count":{NNUE_TOWER_COUNT},')
        file.write('"pairwise":true,')
        file.write('"input_weights":')
        write_int_matrix(file, input_weights)
        file.write(',"input_bias":')
        write_int_vector(file, input_bias)
        file.write(',"towers":[')
        for tower_idx, tower in enumerate(model.towers):
            if tower_idx:
                file.write(",")
            file.write('{"dense_layers":[')
            input_size = NNUE_DENSE_INPUT_SIZE
            for idx, layer in enumerate(tower["layers"]):
                if idx:
                    file.write(",")
                weights = quantized_array(layer.weight, np.int16)
                bias = quantized_array(layer.bias, np.int32)
                output_size = layer.out_features
                file.write("{")
                file.write(f'"input_size":{input_size},')
                file.write(f'"output_size":{output_size},')
                file.write('"weights":')
                write_int_matrix(file, weights)
                file.write(',"bias":')
                write_int_vector(file, bias)
                file.write("}")
                input_size = output_size
            output = tower["output"]
            output_weights = quantized_array(output.weight[0], np.int16)
            output_bias = quantized_array(output.bias, np.int32)
            file.write('],"output_weights":')
            write_int_vector(file, output_weights)
            file.write(f',"output_bias":{int(output_bias.reshape(-1)[0])}')
            file.write("}")
        file.write("]")
        file.write("}")


def save_checkpoint(path: Path, model: TorchNnue, dense_optimizer, sparse_optimizer, epoch: int, step: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    torch.save(
        {
            "model": model.state_dict(),
            "dense_optimizer": dense_optimizer.state_dict(),
            "sparse_optimizer": sparse_optimizer.state_dict(),
            "epoch": epoch,
            "step": step,
            "nnue_config": asdict(NNUE_CONFIG),
        },
        path,
    )


def load_checkpoint(path: Path, model: TorchNnue, dense_optimizer, sparse_optimizer, device: torch.device) -> tuple[int, int]:
    checkpoint = torch.load(path, map_location=device, weights_only=False)
    checkpoint_config = checkpoint.get("nnue_config", asdict(NnueConfig()))
    if checkpoint_config != asdict(NNUE_CONFIG):
        raise RuntimeError(
            f"checkpoint NNUE config mismatch: checkpoint={checkpoint_config} current={asdict(NNUE_CONFIG)}"
        )
    model.load_state_dict(checkpoint["model"])
    dense_optimizer.load_state_dict(checkpoint["dense_optimizer"])
    sparse_optimizer.load_state_dict(checkpoint["sparse_optimizer"])
    return int(checkpoint["epoch"]), int(checkpoint.get("step", 0))


def resolve_device(value: str) -> torch.device:
    if value == "auto":
        value = "cuda" if torch.cuda.is_available() else "cpu"
    device = torch.device(value)
    if device.type == "cuda" and not torch.cuda.is_available():
        raise RuntimeError("CUDA/ROCm device was requested but torch.cuda.is_available() is false")
    return device


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", type=Path, required=True)
    parser.add_argument("--out-json", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, default=None)
    parser.add_argument("--resume", type=Path, default=None)
    parser.add_argument("--epochs", type=int, default=3)
    parser.add_argument("--batch-size", type=int, default=1024)
    parser.add_argument("--lr", type=float, default=16.0)
    parser.add_argument("--lr-schedule", choices=["constant", "cosine"], default="cosine")
    parser.add_argument("--warmup-steps", type=int, default=1000)
    parser.add_argument("--final-lr-ratio", type=float, default=0.01)
    parser.add_argument("--huber-delta", type=float, default=4.0)
    parser.add_argument("--weight-decay", type=float, default=1e-6)
    parser.add_argument("--train-limit", type=int, default=None)
    parser.add_argument("--valid-limit", type=int, default=200_000)
    parser.add_argument("--phase-range", type=parse_phase_range, default=(12, 63))
    parser.add_argument("--num-workers", type=int, default=2)
    parser.add_argument("--prefetch-factor", type=int, default=4)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--device", default="auto")
    parser.add_argument("--log-dir", type=Path, default=None)
    parser.add_argument("--progress-every-steps", type=int, default=100)
    parser.add_argument("--save-every-epochs", type=int, default=1)
    parser.add_argument("--augment-symmetry", dest="augment_symmetry", action="store_true", default=True)
    parser.add_argument("--no-augment-symmetry", dest="augment_symmetry", action="store_false")
    parser.add_argument("--augment-rotations-only", action="store_true")
    parser.add_argument("--acc-size", type=int, choices=[64, 128, 256], default=256)
    parser.add_argument("--pattern-set", choices=["v2", "no-corner2x5", "v1"], default="v2")
    parser.add_argument("--share-rotations", action="store_true")
    args = parser.parse_args()

    configure_nnue(
        NnueConfig(
            acc_size=args.acc_size,
            pattern_set=args.pattern_set,
            share_rotations=args.share_rotations,
        )
    )
    torch.manual_seed(args.seed)
    device = resolve_device(args.device)
    train_set, train_phase_stats = make_dataset(
        args.data,
        "train",
        args.phase_range,
        args.train_limit,
        args.seed,
    )
    valid_set, valid_phase_stats = make_dataset(
        args.data,
        "valid",
        args.phase_range,
        args.valid_limit,
        args.seed + 1,
    )
    loader_kwargs = {
        "num_workers": args.num_workers,
        "pin_memory": device.type == "cuda",
    }
    if args.num_workers > 0:
        loader_kwargs["persistent_workers"] = True
        loader_kwargs["prefetch_factor"] = max(1, args.prefetch_factor)
    train_loader = DataLoader(
        train_set,
        batch_size=args.batch_size,
        shuffle=True,
        collate_fn=make_collate_nnue_records(args.augment_symmetry, args.augment_rotations_only),
        **loader_kwargs,
    )
    valid_loader = DataLoader(
        valid_set,
        batch_size=args.batch_size,
        shuffle=False,
        collate_fn=make_collate_nnue_records(False),
        **loader_kwargs,
    )

    model = TorchNnue().to(device)
    dense_params = [
        param
        for name, param in model.named_parameters()
        if name != "input_weights.weight"
    ]
    dense_optimizer = torch.optim.AdamW(dense_params, lr=args.lr, weight_decay=args.weight_decay)
    sparse_optimizer = torch.optim.SparseAdam([model.input_weights.weight], lr=args.lr)
    start_epoch = 1
    global_step = 0
    if args.resume is not None:
        start_epoch, global_step = load_checkpoint(
            args.resume,
            model,
            dense_optimizer,
            sparse_optimizer,
            device,
        )

    logger = MetricsLogger(args.log_dir)
    try:
        print(
            f"device={device} cuda_available={torch.cuda.is_available()} "
            f"train={len(train_set)} valid={len(valid_set)} "
            f"phase_range={args.phase_range[0]}..{args.phase_range[1]} "
            f"batch_size={args.batch_size} lr={args.lr} "
            f"acc_size={NNUE_ACCUMULATOR_SIZE} pattern_set={NNUE_CONFIG.pattern_set} "
            f"share_rotations={NNUE_CONFIG.share_rotations} input_size={NNUE_INPUT_SIZE}",
            flush=True,
        )
        print(f"train_phase_counts={format_phase_stats(train_phase_stats)}", flush=True)
        print(f"valid_phase_counts={format_phase_stats(valid_phase_stats)}", flush=True)
        if device.type == "cuda":
            print(f"gpu={torch.cuda.current_device()}:{torch.cuda.get_device_name(torch.cuda.current_device())}", flush=True)

        total_seen = 0
        total_steps = max(1, args.epochs * max(1, len(train_loader)))
        for epoch in range(start_epoch, args.epochs + 1):
            train_loss, train_mae, train_samples, global_step, _ = run_epoch(
                model=model,
                loader=train_loader,
                device=device,
                dense_optimizer=dense_optimizer,
                sparse_optimizer=sparse_optimizer,
                huber_delta=args.huber_delta,
                epoch=epoch,
                total_epochs=args.epochs,
                total_seen_samples=total_seen,
                global_step=global_step,
                progress_every_steps=args.progress_every_steps,
                logger=logger,
                lr=args.lr,
                total_steps=total_steps,
                lr_schedule=args.lr_schedule,
                warmup_steps=args.warmup_steps,
                final_lr_ratio=args.final_lr_ratio,
                collect_by_empties=False,
            )
            total_seen += train_samples
            with torch.no_grad():
                valid_loss, valid_mae, _, global_step, valid_by_empties = run_epoch(
                    model=model,
                    loader=valid_loader,
                    device=device,
                    dense_optimizer=None,
                    sparse_optimizer=None,
                    huber_delta=args.huber_delta,
                    epoch=epoch,
                    total_epochs=args.epochs,
                    total_seen_samples=total_seen,
                    global_step=global_step,
                    progress_every_steps=0,
                    logger=logger,
                    lr=args.lr,
                    total_steps=total_steps,
                    lr_schedule=args.lr_schedule,
                    warmup_steps=args.warmup_steps,
                    final_lr_ratio=args.final_lr_ratio,
                    collect_by_empties=True,
                )
            empty_events = empty_metric_events(
                by_empties=valid_by_empties,
                epoch=epoch,
                total_epochs=args.epochs,
            )
            logger.write_empty_metrics(empty_events)
            print(
                f"epoch={epoch:03d}/{args.epochs:03d} summary "
                f"train_loss={train_loss:.6f} train_disc_mae={train_mae:.3f} "
                f"valid_loss={valid_loss:.6f} valid_disc_mae={valid_mae:.3f} "
                f"samples={train_samples} total_samples={total_seen} step={global_step}",
                flush=True,
            )
            print(f"epoch={epoch:03d}/{args.epochs:03d} valid_by_empties={format_empty_metric_summary(empty_events)}", flush=True)
            logger.write(
                {
                    "kind": "summary",
                    "evaluator": "nnue_torch",
                    "epoch": epoch,
                    "total_epochs": args.epochs,
                    "batch": len(train_loader),
                    "total_batches": len(train_loader),
                    "step": global_step,
                    "samples": train_samples,
                    "total_samples": total_seen,
                    "avg_loss": train_loss,
                    "disc_mae": train_mae,
                    "last_loss": "",
                    "last_disc_mae": "",
                    "valid_loss": valid_loss,
                    "valid_disc_mae": valid_mae,
                    "lr": "",
                    "speed": "",
                    "elapsed_seconds": "",
                    "eta_epoch_seconds": 0.0,
                    "eta_total_seconds": "",
                }
            )
            if args.checkpoint is not None and epoch % max(1, args.save_every_epochs) == 0:
                save_checkpoint(args.checkpoint, model, dense_optimizer, sparse_optimizer, epoch + 1, global_step)

        export_nnue_json(model, args.out_json)
        print(f"wrote {args.out_json}", flush=True)
    finally:
        logger.close()


if __name__ == "__main__":
    main()
