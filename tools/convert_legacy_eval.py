#!/usr/bin/env python3
"""旧エンジン (deft-reversi-engine on `main`) のパターン評価 JSON を、
現行エンジンが読める legacy-pattern JSON へ変換する。

旧形式:
    {"version": ..., "n_deta_set": ..., "n_iteration": ...,
     "eval": [[EvaluationScores x 31] x 2]}

    - `eval[parity][phase]` で引く
    - `parity = board.empties_count() % 2`
    - `phase  = board.move_count() / 2`   (N_PHASE = 31)
    - EvaluationScores = {"pattern_eval": [[i16] x 11],
                          "mobility_eval": [i16 x 128],
                          "const_eval": i16}

新形式 (`file.rs::read_legacy_pattern_json` が受け付ける形):
    {"evaluator": {"phases": [PhaseData x 60]}}

    - `phases[move_count.min(59)]` で引く   (N_PHASES = 60)
    - PhaseData = {"pattern_weights": [[i16] x 11],
                   "mobility_weights": [i16 x 128],
                   "bias": i16}

変換の根拠:

  * 盤面は常に 60 手で埋まるため `empties = 60 - move_count` であり、
    `empties % 2 == move_count % 2` が成り立つ。
    よって旧の (parity, phase) は move_count を 2 で割った商と余りに
    分解したものであり、move_count と 1 対 1 に対応する。
    したがって `phases[p] = eval[p % 2][p // 2]` が厳密な対応となる。

  * 特徴量 index の作り方 (`idx = idx * 3 + color`、
    color = empty:0 / opponent:1 / player:2、パターン内の座標順)、
    パターン定義 11 個とその座標順、mobility の index (64 + 自分の合法手数
    - 相手の合法手数) と要素数 128、スコアスケール 128 と丸め方は
    新旧で一致しているため、重みはそのまま転用できる。

  * 旧の phase 30 (move_count 60、= 終局) は新側では phase 59 に
    clamp されるため使用されない。

使い方:
    python3 tools/convert_legacy_eval.py data/eval/eval.json data/eval/eval-legacy.json
"""

import argparse
import json
import sys

N_PHASES_NEW = 60
N_PHASE_OLD = 31
N_PARITY_OLD = 2
N_PATTERNS = 11
N_MOBILITY = 128

# POW3[n_squares] for each pattern, in PATTERN_DEFINITIONS order.
# n_squares = [10, 10, 10, 8, 8, 9, 10, 9, 6, 7, 8]
PATTERN_TABLE_SIZES = [59049, 59049, 59049, 6561, 6561, 19683, 59049, 19683, 729, 2187, 6561]


def fail(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def check_phase(scores, where):
    pattern_eval = scores["pattern_eval"]
    if len(pattern_eval) != N_PATTERNS:
        fail(f"{where}: pattern_eval must have {N_PATTERNS} patterns, got {len(pattern_eval)}")
    for i, (weights, expected) in enumerate(zip(pattern_eval, PATTERN_TABLE_SIZES)):
        if len(weights) != expected:
            fail(f"{where}: pattern_eval[{i}] must have {expected} weights, got {len(weights)}")
    if len(scores["mobility_eval"]) != N_MOBILITY:
        fail(
            f"{where}: mobility_eval must have {N_MOBILITY} weights, "
            f"got {len(scores['mobility_eval'])}"
        )


def convert(old):
    old_eval = old["eval"]
    if len(old_eval) != N_PARITY_OLD:
        fail(f"eval must have {N_PARITY_OLD} parity entries, got {len(old_eval)}")
    for parity, phases in enumerate(old_eval):
        if len(phases) != N_PHASE_OLD:
            fail(f"eval[{parity}] must have {N_PHASE_OLD} phases, got {len(phases)}")

    new_phases = []
    for move_count in range(N_PHASES_NEW):
        parity = move_count % 2
        old_phase = move_count // 2
        scores = old_eval[parity][old_phase]
        check_phase(scores, f"eval[{parity}][{old_phase}]")
        new_phases.append(
            {
                "pattern_weights": scores["pattern_eval"],
                "mobility_weights": scores["mobility_eval"],
                "bias": scores["const_eval"],
            }
        )

    return {"evaluator": {"phases": new_phases}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", help="旧形式の eval.json")
    parser.add_argument("output", help="出力先 (現行エンジンが読める JSON)")
    args = parser.parse_args()

    with open(args.input, "r", encoding="utf-8") as f:
        old = json.load(f)

    new = convert(old)

    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(new, f, separators=(",", ":"))

    print(f"converted {args.input} -> {args.output} ({N_PHASES_NEW} phases)")


if __name__ == "__main__":
    main()
