#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import random
from pathlib import Path


def load_train_module(script_path: Path):
    spec = importlib.util.spec_from_file_location("train_nnue_torch", script_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {script_path}")
    module = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(module)
        return module
    except ModuleNotFoundError:
        return ScalarNnue()


class FeatureViews:
    def __init__(self, rows: list[list[int]], padding: int) -> None:
        self.rows = rows
        self.padding = padding

    def __getitem__(self, idx: int):
        class Row:
            def __init__(self, values: list[int]) -> None:
                self.values = values

            def tolist(self) -> list[int]:
                return self.values

        return Row(self.rows[idx])


class ScalarNnue:
    N_BOARD_SQUARES = 64
    NNUE_ACCUMULATOR_SIZE = 256
    NNUE_ACTIVATION_MAX = 4095
    NNUE_DEFAULT_WEIGHT_SCALE = 4096
    NNUE_DENSE_LAYER_SIZES = [32, 32]
    NNUE_TOWER_COUNT = 8
    NNUE_PAIRWISE_SIZE_PER_VIEW = 128
    NNUE_DENSE_INPUT_SIZE = 256
    NNUE_STONE_OFFSET = 0
    NNUE_LEGAL_MOVE_OFFSET = 128
    NNUE_QUADRANT_OFFSET = 256
    NNUE_QUADRANT_BUCKETS = 17
    NNUE_QUADRANT_PARITY_OFFSET = 324
    NNUE_QUADRANT_PARITY_BUCKETS = 2
    NNUE_GLOBAL_PARITY_OFFSET = 332
    NNUE_PATTERN_OFFSET = 334
    NNUE_MAX_ACTIVE_FEATURES_PER_VIEW = 255
    MASK64 = (1 << 64) - 1
    HORIZONTAL_MASK = 0x7E7E7E7E7E7E7E7E

    def __init__(self) -> None:
        self.PATTERN_INSTANCES = self.build_pattern_instances()
        pow3 = [1]
        for _ in range(16):
            pow3.append(pow3[-1] * 3)
        self.POW3 = pow3
        self.PATTERN_INSTANCE_OFFSETS = []
        offset = 0
        for n_squares, _ in self.PATTERN_INSTANCES:
            self.PATTERN_INSTANCE_OFFSETS.append(offset)
            offset += self.POW3[n_squares]
        self.NNUE_PATTERN_FEATURES = offset
        self.NNUE_INPUT_SIZE = self.NNUE_PATTERN_OFFSET + offset
        self.PADDING_FEATURE_ID = self.NNUE_INPUT_SIZE
        self.QUADRANT_MASKS = [
            sum(1 << self.square(x, y) for y in range(0, 4) for x in range(0, 4)),
            sum(1 << self.square(x, y) for y in range(0, 4) for x in range(4, 8)),
            sum(1 << self.square(x, y) for y in range(4, 8) for x in range(0, 4)),
            sum(1 << self.square(x, y) for y in range(4, 8) for x in range(4, 8)),
        ]

    def square(self, x: int, y: int) -> int:
        return y * 8 + x

    def rotate_square(self, pos: int, turns: int) -> int:
        for _ in range(turns):
            x = pos % 8
            y = pos // 8
            pos = x * 8 + (7 - y)
        return pos

    def rot(self, n_squares: int, squares: list[int], turns: int) -> tuple[int, list[int]]:
        return n_squares, [self.rotate_square(pos, turns) for pos in squares[:n_squares]]

    def build_pattern_instances(self) -> list[tuple[int, list[int]]]:
        s = self.square
        patterns = [
            (8, [s(2, 0), s(3, 0), s(4, 0), s(5, 0), s(2, 1), s(3, 1), s(4, 1), s(5, 1)], 4),
            (8, [s(x, 0) for x in range(8)], 4),
            (8, [s(x, 1) for x in range(8)], 4),
            (8, [s(x, 2) for x in range(8)], 4),
            (8, [s(x, 3) for x in range(8)], 4),
            (9, [s(x, y) for y in range(3) for x in range(3)], 4),
            (4, [s(3, 0), s(2, 1), s(1, 2), s(0, 3)], 4),
            (5, [s(4, 0), s(3, 1), s(2, 2), s(1, 3), s(0, 4)], 4),
            (6, [s(5, 0), s(4, 1), s(3, 2), s(2, 3), s(1, 4), s(0, 5)], 4),
            (7, [s(6, 0), s(5, 1), s(4, 2), s(3, 3), s(2, 4), s(1, 5), s(0, 6)], 4),
        ]
        instances = []
        for n_squares, squares, rotations in patterns:
            for turns in range(rotations):
                instances.append(self.rot(n_squares, squares, turns))
        instances.append((8, [s(i, i) for i in range(8)]))
        instances.append((8, [s(7 - i, i) for i in range(8)]))
        for n_squares, squares, rotations in [
            (10, [s(x, 0) for x in range(8)] + [s(1, 1), s(6, 1)], 4),
            (10, [s(x, 0) for x in range(5)] + [s(x, 1) for x in range(5)], 4),
            (10, [s(0, y) for y in range(5)] + [s(1, y) for y in range(5)], 4),
        ]:
            for turns in range(rotations):
                instances.append(self.rot(n_squares, squares, turns))
        return instances

    def u64(self, value: int) -> int:
        return value & self.MASK64

    def get_moves(self, player: int, opponent: int) -> int:
        p = self.u64(player)
        o = self.u64(opponent)
        moves = 0
        for pos in self.iter_bits(p):
            x0, y0 = pos % 8, pos // 8
            for dx, dy in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]:
                x, y = x0 + dx, y0 + dy
                seen = False
                while 0 <= x < 8 and 0 <= y < 8:
                    bit = 1 << (y * 8 + x)
                    if o & bit:
                        seen = True
                    elif seen and not ((p | o) & bit):
                        moves |= bit
                        break
                    else:
                        break
                    x += dx
                    y += dy
        return moves

    def iter_bits(self, bitboard: int):
        bits = self.u64(bitboard)
        while bits:
            lsb = bits & -bits
            yield lsb.bit_length() - 1
            bits &= bits - 1

    def active_feature_views(self, own: int, opponent: int) -> FeatureViews:
        own_moves = self.get_moves(own, opponent)
        opponent_moves = self.get_moves(opponent, own)
        rows = [
            self.active_features_for_view(own, opponent, own_moves, opponent_moves),
            self.active_features_for_view(opponent, own, opponent_moves, own_moves),
        ]
        return FeatureViews([row + [self.PADDING_FEATURE_ID] * (self.NNUE_MAX_ACTIVE_FEATURES_PER_VIEW - len(row)) for row in rows], self.PADDING_FEATURE_ID)

    def active_features_for_view(self, own: int, opponent: int, own_moves: int, opponent_moves: int) -> list[int]:
        features = []
        for offset, bitboard in [
            (self.NNUE_STONE_OFFSET, own),
            (self.NNUE_STONE_OFFSET + 64, opponent),
            (self.NNUE_LEGAL_MOVE_OFFSET, own_moves),
            (self.NNUE_LEGAL_MOVE_OFFSET + 64, opponent_moves),
        ]:
            for pos in self.iter_bits(bitboard):
                features.append(offset + pos)
        occupied = own | opponent
        total_empty = 64 - occupied.bit_count()
        for idx, mask in enumerate(self.QUADRANT_MASKS):
            empty_count = ((mask & ~occupied) & self.MASK64).bit_count()
            features.append(self.NNUE_QUADRANT_OFFSET + idx * self.NNUE_QUADRANT_BUCKETS + empty_count)
            features.append(self.NNUE_QUADRANT_PARITY_OFFSET + idx * self.NNUE_QUADRANT_PARITY_BUCKETS + (empty_count & 1))
        features.append(self.NNUE_GLOBAL_PARITY_OFFSET + (total_empty & 1))
        for idx, instance in enumerate(self.PATTERN_INSTANCES):
            features.append(self.NNUE_PATTERN_OFFSET + self.PATTERN_INSTANCE_OFFSETS[idx] + self.pattern_code(instance, own, opponent))
        return features

    def pattern_code(self, instance: tuple[int, list[int]], own: int, opponent: int) -> int:
        code = 0
        n_squares, squares = instance
        for pos in squares[:n_squares]:
            bit = 1 << pos
            code = code * 3 + (1 if own & bit else 2 if opponent & bit else 0)
        return code


def make_move(module, own: int, opponent: int, square: int) -> tuple[int, int]:
    flip = 0
    x0 = square % 8
    y0 = square // 8
    for dx, dy in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]:
        x = x0 + dx
        y = y0 + dy
        line = 0
        while 0 <= x < 8 and 0 <= y < 8:
            bit = 1 << (y * 8 + x)
            if opponent & bit:
                line |= bit
            elif own & bit:
                flip |= line
                break
            else:
                break
            x += dx
            y += dy
    move_bit = 1 << square
    return opponent ^ flip, own ^ (flip | move_bit)


def legal_squares(module, own: int, opponent: int) -> list[int]:
    return list(module.iter_bits(module.get_moves(own, opponent)))


def sample_boards(module) -> list[dict[str, object]]:
    own = (1 << 35) | (1 << 28)
    opponent = (1 << 27) | (1 << 36)
    boards = [{"name": "initial", "own": own, "opponent": opponent}]

    scripted = [19, 18, 17, 26]
    for idx, move in enumerate(scripted, start=1):
        own, opponent = make_move(module, own, opponent, move)
        boards.append({"name": f"scripted_{idx}", "own": own, "opponent": opponent})

    rng = random.Random(20260211)
    for idx in range(5):
        own = (1 << 35) | (1 << 28)
        opponent = (1 << 27) | (1 << 36)
        for _ in range(6 + idx * 5):
            moves = legal_squares(module, own, opponent)
            if not moves:
                own, opponent = opponent, own
                moves = legal_squares(module, own, opponent)
                if not moves:
                    break
            own, opponent = make_move(module, own, opponent, rng.choice(moves))
        boards.append({"name": f"random_{idx}", "own": own, "opponent": opponent})
    return boards


def sorted_features(module, own: int, opponent: int) -> list[list[int]]:
    views = module.active_feature_views(own, opponent)
    result = []
    for view in range(2):
        features = [
            int(value)
            for value in views[view].tolist()
            if int(value) != module.PADDING_FEATURE_ID
        ]
        result.append(sorted(features))
    return result


def round_div(value: int, divisor: int) -> int:
    if value > 0:
        return (value + divisor // 2) // divisor
    if value < 0:
        return -((-value + divisor // 2) // divisor)
    return 0


def random_model(module) -> dict[str, object]:
    rng = random.Random(314159)
    feature_ids = sorted(
        set(rng.randrange(module.NNUE_INPUT_SIZE) for _ in range(256))
        | set(range(0, 256))
        | {module.NNUE_PATTERN_OFFSET + rng.randrange(module.NNUE_PATTERN_FEATURES) for _ in range(256)}
    )
    input_bias = [rng.randint(-48, 48) for _ in range(module.NNUE_ACCUMULATOR_SIZE)]
    input_weights = {
        str(fid): [rng.randint(-16, 16) for _ in range(module.NNUE_ACCUMULATOR_SIZE)]
        for fid in feature_ids
    }
    towers = []
    for _ in range(module.NNUE_TOWER_COUNT):
        dense0 = {
            "weights": [
                [rng.randint(-8, 8) for _ in range(module.NNUE_DENSE_INPUT_SIZE)]
                for _ in range(module.NNUE_DENSE_LAYER_SIZES[0])
            ],
            "bias": [rng.randint(-32, 32) for _ in range(module.NNUE_DENSE_LAYER_SIZES[0])],
        }
        dense1 = {
            "weights": [
                [rng.randint(-8, 8) for _ in range(module.NNUE_DENSE_LAYER_SIZES[0])]
                for _ in range(module.NNUE_DENSE_LAYER_SIZES[1])
            ],
            "bias": [rng.randint(-32, 32) for _ in range(module.NNUE_DENSE_LAYER_SIZES[1])],
        }
        towers.append(
            {
                "dense_layers": [dense0, dense1],
                "output_weights": [rng.randint(-8, 8) for _ in range(module.NNUE_DENSE_LAYER_SIZES[1])],
                "output_bias": rng.randint(-32, 32),
            }
        )
    return {
        "weight_scale": module.NNUE_DEFAULT_WEIGHT_SCALE,
        "input_bias": input_bias,
        "input_weights": input_weights,
        "towers": towers,
    }


def model_weight(model: dict[str, object], key: str, feature_id: int, width: int) -> list[int]:
    return model[key].get(str(feature_id), [0] * width)


def evaluate_reference(module, model: dict[str, object], own: int, opponent: int) -> int:
    views = sorted_features(module, own, opponent)
    accs = []
    for features in views:
        acc = list(model["input_bias"])
        for feature_id in features:
            weights = model_weight(model, "input_weights", feature_id, module.NNUE_ACCUMULATOR_SIZE)
            for idx, weight in enumerate(weights):
                acc[idx] += int(weight)
        accs.append(acc)
    activation = []
    for acc in accs:
        for idx in range(module.NNUE_PAIRWISE_SIZE_PER_VIEW):
            a = max(0, min(module.NNUE_ACTIVATION_MAX, acc[idx]))
            b = max(0, min(module.NNUE_ACTIVATION_MAX, acc[idx + module.NNUE_PAIRWISE_SIZE_PER_VIEW]))
            activation.append((a * b) >> 12)
    empties = module.N_BOARD_SQUARES - (own | opponent).bit_count()
    bucket = min(empties // 8, module.NNUE_TOWER_COUNT - 1)
    tower = model["towers"][bucket]
    for layer in tower["dense_layers"]:
        next_activation = []
        for row, bias in zip(layer["weights"], layer["bias"]):
            raw = int(bias) + sum(int(a) * int(w) for a, w in zip(activation, row))
            next_activation.append(max(0, min(module.NNUE_ACTIVATION_MAX, round_div(raw, model["weight_scale"]))))
        activation = next_activation
    score = round_div(
        int(tower["output_bias"]) + sum(int(a) * int(w) for a, w in zip(activation, tower["output_weights"])),
        model["weight_scale"],
    )
    return max(-64, min(64, score))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--script", type=Path, default=Path(__file__).with_name("train_nnue_torch.py"))
    parser.add_argument("--fixture-dir", type=Path, default=Path("deft-reversi-engine/tests/fixtures"))
    args = parser.parse_args()

    module = load_train_module(args.script)
    args.fixture_dir.mkdir(parents=True, exist_ok=True)
    boards = sample_boards(module)
    feature_cases = []
    for board in boards:
        own = int(board["own"])
        opponent = int(board["opponent"])
        feature_cases.append(
            {
                "name": board["name"],
                "own": own,
                "opponent": opponent,
                "active_features": sorted_features(module, own, opponent),
            }
        )
    (args.fixture_dir / "nnue_v2_active_features.json").write_text(
        json.dumps({"cases": feature_cases}, indent=2) + "\n"
    )

    model = random_model(module)
    inference_boards = boards + sample_boards(module)[1:]
    inference_boards.extend(
        {
            "name": f"{board['name']}_passed",
            "own": int(board["opponent"]),
            "opponent": int(board["own"]),
        }
        for board in boards
    )
    inference_cases = []
    for board in inference_boards:
        own = int(board["own"])
        opponent = int(board["opponent"])
        inference_cases.append(
            {
                "name": board["name"],
                "own": own,
                "opponent": opponent,
                "expected": evaluate_reference(module, model, own, opponent),
            }
        )
    (args.fixture_dir / "nnue_v2_inference.json").write_text(
        json.dumps({"model": model, "cases": inference_cases}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
