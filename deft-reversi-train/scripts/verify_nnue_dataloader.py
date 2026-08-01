#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import random
import sys
import time
from pathlib import Path

import numpy as np


def load_train_module(script_path: Path):
    spec = importlib.util.spec_from_file_location("train_nnue_torch", script_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {script_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def assert_equivalent(module, owns: np.ndarray, opponents: np.ndarray) -> None:
    batch_features, batch_empties = module.active_feature_views_batch(owns, opponents)
    for idx, (own, opponent) in enumerate(zip(owns.tolist(), opponents.tolist())):
        scalar_features = module.active_feature_views(int(own), int(opponent))
        for view in range(2):
            scalar_set = set(
                scalar_features[view][scalar_features[view] != module.PADDING_FEATURE_ID].tolist()
            )
            batch_set = set(
                batch_features[idx, view][batch_features[idx, view] != module.PADDING_FEATURE_ID].tolist()
            )
            if scalar_set != batch_set:
                missing = sorted(scalar_set - batch_set)[:16]
                extra = sorted(batch_set - scalar_set)[:16]
                raise AssertionError(
                    f"feature mismatch sample={idx} view={view} missing={missing} extra={extra}"
                )
        expected_empties = module.N_BOARD_SQUARES - (int(own) | int(opponent)).bit_count()
        if int(batch_empties[idx]) != expected_empties:
            raise AssertionError(
                f"empties mismatch sample={idx} expected={expected_empties} actual={int(batch_empties[idx])}"
            )


def assert_layout(module) -> None:
    if module.NNUE_CONFIG != module.NnueConfig():
        return
    assert len(module.PATTERN_INSTANCES) == 54
    assert module.NNUE_PATTERN_FEATURES == 944_622
    assert module.NNUE_INPUT_SIZE == 944_956
    assert module.NNUE_MAX_ACTIVE_FEATURES_PER_VIEW == 255


def assert_symmetry(module, owns: np.ndarray, opponents: np.ndarray) -> None:
    transforms = np.arange(8, dtype=np.uint8)
    sample_owns = np.resize(owns[:8], 8).astype(np.uint64, copy=False)
    sample_opponents = np.resize(opponents[:8], 8).astype(np.uint64, copy=False)
    for transform in range(8):
        t = np.full(sample_owns.shape, transform, dtype=np.uint8)
        transformed_own = module.transform_bitboards_np(sample_owns, t)
        transformed_opponent = module.transform_bitboards_np(sample_opponents, t)
        moves = module.get_moves_np(sample_owns, sample_opponents)
        transformed_moves = module.transform_bitboards_np(moves, t)
        actual_moves = module.get_moves_np(transformed_own, transformed_opponent)
        if not np.array_equal(transformed_moves, actual_moves):
            raise AssertionError(f"move symmetry mismatch transform={transform}")
    for transform in transforms:
        inverse = 0
        for candidate in transforms:
            positions = np.arange(64)
            mapped = [
                module.transform_square(module.transform_square(int(pos), int(transform)), int(candidate))
                for pos in positions
            ]
            if mapped == positions.tolist():
                inverse = int(candidate)
                break
        t = np.full(sample_owns.shape, int(transform), dtype=np.uint8)
        inv = np.full(sample_owns.shape, inverse, dtype=np.uint8)
        restored = module.transform_bitboards_np(module.transform_bitboards_np(sample_owns, t), inv)
        if not np.array_equal(restored, sample_owns):
            raise AssertionError(f"inverse symmetry mismatch transform={int(transform)}")


def pattern_feature_set(module, features: np.ndarray) -> set[int]:
    return {
        int(feature)
        for feature in features.tolist()
        if module.NNUE_PATTERN_OFFSET <= int(feature) < module.PADDING_FEATURE_ID
    }


def assert_shared_rotation_patterns(module, owns: np.ndarray, opponents: np.ndarray) -> None:
    if not module.NNUE_CONFIG.share_rotations:
        return
    sample_owns = np.resize(owns[:16], 16).astype(np.uint64, copy=False)
    sample_opponents = np.resize(opponents[:16], 16).astype(np.uint64, copy=False)
    base_features, _ = module.active_feature_views_batch(sample_owns, sample_opponents)
    base_sets = [pattern_feature_set(module, row[0]) for row in base_features]
    for transform in range(1, 4):
        t = np.full(sample_owns.shape, transform, dtype=np.uint8)
        transformed_own = module.transform_bitboards_np(sample_owns, t)
        transformed_opponent = module.transform_bitboards_np(sample_opponents, t)
        transformed_features, _ = module.active_feature_views_batch(transformed_own, transformed_opponent)
        for idx, expected in enumerate(base_sets):
            actual = pattern_feature_set(module, transformed_features[idx, 0])
            if actual != expected:
                raise AssertionError(
                    f"shared rotation pattern mismatch sample={idx} rotation={transform}"
                )


def random_positions(count: int, seed: int) -> tuple[np.ndarray, np.ndarray]:
    rng = random.Random(seed)
    owns = np.empty(count, dtype=np.uint64)
    opponents = np.empty(count, dtype=np.uint64)
    for idx in range(count):
        own = rng.getrandbits(64)
        opponent = rng.getrandbits(64) & ~own
        owns[idx] = own
        opponents[idx] = opponent
    return owns, opponents


def real_positions(module, data_root: Path, limit: int) -> tuple[np.ndarray, np.ndarray]:
    owns: list[int] = []
    opponents: list[int] = []
    for path in sorted(data_root.glob("phase_*/train.rd")):
        dataset = module.RdNnueDataset(path)
        remaining = limit - len(owns)
        if remaining <= 0:
            break
        take = min(remaining, len(dataset.records))
        owns.extend(dataset.records["own"][:take].tolist())
        opponents.extend(dataset.records["opponent"][:take].tolist())
    if len(owns) < limit:
        raise RuntimeError(f"only found {len(owns)} real samples under {data_root}, requested {limit}")
    return np.array(owns, dtype=np.uint64), np.array(opponents, dtype=np.uint64)


def benchmark(module, owns: np.ndarray, opponents: np.ndarray, batch_size: int) -> tuple[float, float]:
    started = time.perf_counter()
    for own, opponent in zip(owns.tolist(), opponents.tolist()):
        module.active_feature_views(int(own), int(opponent))
    scalar_seconds = time.perf_counter() - started

    started = time.perf_counter()
    for start in range(0, len(owns), batch_size):
        end = min(len(owns), start + batch_size)
        module.active_feature_views_batch(owns[start:end], opponents[start:end])
    batch_seconds = time.perf_counter() - started
    return len(owns) / scalar_seconds, len(owns) / batch_seconds


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--script", type=Path, default=Path(__file__).with_name("train_nnue_torch.py"))
    parser.add_argument("--data", type=Path, default=Path("egaroucid_rd_data_set"))
    parser.add_argument("--random-samples", type=int, default=20_000)
    parser.add_argument("--real-samples", type=int, default=20_000)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--batch-size", type=int, default=4096)
    parser.add_argument("--acc-size", type=int, choices=[64, 128, 256], default=256)
    parser.add_argument("--pattern-set", choices=["v2", "no-corner2x5", "v1"], default="v2")
    parser.add_argument("--share-rotations", action="store_true")
    args = parser.parse_args()

    module = load_train_module(args.script)
    module.configure_nnue(
        module.NnueConfig(
            acc_size=args.acc_size,
            pattern_set=args.pattern_set,
            share_rotations=args.share_rotations,
        )
    )
    assert_layout(module)
    print(
        f"config=ok acc_size={module.NNUE_ACCUMULATOR_SIZE} pattern_set={module.NNUE_CONFIG.pattern_set} "
        f"share_rotations={module.NNUE_CONFIG.share_rotations} input_size={module.NNUE_INPUT_SIZE}",
        flush=True,
    )

    random_owns, random_opponents = random_positions(args.random_samples, args.seed)
    assert_symmetry(module, random_owns, random_opponents)
    print("symmetry=ok", flush=True)
    assert_shared_rotation_patterns(module, random_owns, random_opponents)
    if module.NNUE_CONFIG.share_rotations:
        print("shared_rotation_patterns=ok", flush=True)
    assert_equivalent(module, random_owns, random_opponents)
    print(f"random_equivalence=ok samples={args.random_samples}", flush=True)

    real_owns, real_opponents = real_positions(module, args.data, args.real_samples)
    assert_equivalent(module, real_owns, real_opponents)
    print(f"real_equivalence=ok samples={args.real_samples}", flush=True)

    bench_owns = np.concatenate([random_owns, real_owns])
    bench_opponents = np.concatenate([random_opponents, real_opponents])
    scalar_speed, batch_speed = benchmark(module, bench_owns, bench_opponents, args.batch_size)
    print(
        f"benchmark_samples={len(bench_owns)} scalar_samples_per_sec={scalar_speed:.1f} "
        f"batch_samples_per_sec={batch_speed:.1f} speedup={batch_speed / max(1e-9, scalar_speed):.2f}x",
        flush=True,
    )


if __name__ == "__main__":
    main()
