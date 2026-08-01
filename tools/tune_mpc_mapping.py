#!/usr/bin/env python3
"""MPC プローブ深さ対応表の自動チューニング(座標降下)。

各候補マッピングについて:
  1. fit_mpc_nnue2.py で回帰を再フィット(σ は実測値のまま)
  2. 健全性チェック: マッピング上の各 (d, md) セルで
     回帰モデルの σ / 実測 σ が [0.80, 1.25] に収まること(外れたら不採用)
  3. set-mpc で S2 bin に埋め込み、チューニング用ベンチの実時間を計測
1深さずつ最良の md を採用して次へ。結果は CSV に逐次記録。

使い方:
  python3 tools/tune_mpc_mapping.py --out-dir data/log/mpc-tune
"""

import argparse
import csv
import glob
import json
import os
import subprocess
import sys
import time

import numpy as np

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SWEEP_DIR = os.path.join(REPO, "data/log/mpc-s2-sweep")
BASE_BIN = os.path.join(REPO, "runs/small-S2-full-acc128/nnue-eval.bin")
CLI = os.path.join(REPO, "target/release/deft-reversi-cli")
TRAIN = os.path.join(REPO, "target/release/deft-reversi-train")
FIT = os.path.join(REPO, "tools/fit_mpc_nnue2.py")

# チューニング用ベンチ: fforum 74-79(空き 30+、中盤ソルバー主体)を level 14 で
TUNE_PROBLEMS_RANGE = (14, 20)  # fforum-60-79.obf の 15..20 行目(#74-79)
BENCH_LEVEL = 14
BENCH_TIMEOUT = 480  # 秒。超えたら不採用扱い

# 初期マッピングと候補(パリティ一致、ギャップ 4/6/8)
INITIAL = {4: 0, 5: 1, 6: 2, 7: 3, 8: 4, 9: 5, 10: 4, 11: 5, 12: 6, 13: 7}
CANDIDATES = {
    6: [0, 2], 7: [1, 3], 8: [0, 2, 4], 9: [1, 3, 5],
    10: [2, 4, 6], 11: [3, 5, 7], 12: [4, 6, 8], 13: [3, 5, 7],
}
FINAL_MAPPING = "12:4,13:3,14:4,15:3,16:4,17:5,18:6,19:5,20:5,21:5,22:6"

# --stage final 用: eval マッピングは stage1 の最適解に固定し、
# search_lv_by_empties を座標降下する
EVAL_MAPPING_TUNED = {4: 0, 5: 1, 6: 2, 7: 1, 8: 2, 9: 1, 10: 4, 11: 3, 12: 4, 13: 3}
FINAL_INITIAL = {12: 4, 13: 3, 14: 4, 15: 3, 16: 4, 17: 5, 18: 6,
                 19: 5, 20: 5, 21: 5, 22: 6}
FINAL_CANDIDATES = {
    12: [2, 6], 13: [5, 7], 14: [2, 6], 15: [5, 7], 16: [2, 6],
    17: [3, 7], 18: [4, 8], 19: [3, 7], 20: [3, 7], 21: [3, 7, 9], 22: [4, 8],
}
# 終盤チューニング用ベンチ: fforum #60-65(空き 24-27、Final ソルバー主体)
FINAL_TUNE_RANGE = (0, 6)


def read_rows(path):
    rows = []
    with open(path) as f:
        for i, line in enumerate(f):
            if i < 3 or not line.strip():
                continue
            rows.append([int(x) for x in line.split(",")])
    return np.array(rows, dtype=np.float64)


def load_sweep():
    all_rows = [read_rows(p) for p in
                sorted(glob.glob(os.path.join(SWEEP_DIR, "mpc_lv*_serach.txt")))]
    return np.vstack([r for r in all_rows if r.size])


def actual_sigma(rows, d, md):
    m = (rows[:, 1] == d) & (rows[:, 2] == md)
    g = rows[m]
    if len(g) < 30:
        return None
    a = np.polyfit(g[:, 4], g[:, 3], 1)
    res = g[:, 3] - (a[0] * g[:, 4] + a[1])
    # empties 平均も返す(モデル評価用)
    return res.std(ddof=1), g[:, 0].mean()


def mapping_str(mapping):
    return ",".join(f"{k}:{v}" for k, v in sorted(mapping.items()))


def model_sigma(config, e, d, md):
    c = config["eval_search"]["e_std"]
    return (c["constant"] + c["empties"] * e + c["depth"] * d
            + c["mpc_depth"] * md)


def load_final_sigma():
    """mpc_perfect_e*.txt から (e, lv) → 実測 σ の辞書を作る。"""
    import re
    out = {}
    for path in sorted(glob.glob(os.path.join(SWEEP_DIR, "mpc_perfect_e*.txt"))):
        e = int(re.search(r"_e(\d+)\.txt$", path).group(1))
        r = read_rows(path)
        if not r.size:
            continue
        for lv in np.unique(r[:, 2].astype(int)):
            g = r[r[:, 2] == lv]
            if len(g) < 30:
                continue
            a = np.polyfit(g[:, 4], g[:, 3], 1)
            res = g[:, 3] - (a[0] * g[:, 4] + a[1])
            out[(e, int(lv))] = res.std(ddof=1)
    return out


def model_final_sigma(config, e, lv):
    c = config["final_search"]["e_std"]
    return (c["constant"] + c["empties"] * e + c["mpc_depth"] * lv
            + c["parity"] * (e % 2))


def build_and_check(eval_map, final_map, rows, final_sigma, tag, out_dir):
    """フィット→健全性チェック→bin 作成。OK なら bin パス、NG なら None。"""
    json_path = os.path.join(out_dir, f"mpc-{tag}.json")
    bin_path = os.path.join(out_dir, f"nnue-{tag}.bin")
    r = subprocess.run(
        [sys.executable, FIT, "--collect-dir", SWEEP_DIR,
         "--eval-mapping", mapping_str(eval_map),
         "--final-mapping", mapping_str(final_map), "--out", json_path],
        capture_output=True, text=True, cwd=REPO)
    if r.returncode != 0:
        print(f"  fit failed: {r.stderr.strip()[:200]}", flush=True)
        return None
    config = json.load(open(json_path))
    for d, md in eval_map.items():
        act = actual_sigma(rows, d, md)
        if act is None:
            print(f"  d={d} md={md}: 実測データ不足 → 不採用", flush=True)
            return None
        sigma, e_mean = act
        ratio = model_sigma(config, e_mean, d, md) / sigma
        if not (0.80 <= ratio <= 1.25):
            print(f"  d={d} md={md}: model/actual σ = {ratio:.2f} 域外 → 不採用",
                  flush=True)
            return None
    for e, lv in final_map.items():
        act = final_sigma.get((e, lv))
        if act is None:
            print(f"  final e={e} lv={lv}: 実測データ不足 → 不採用", flush=True)
            return None
        ratio = model_final_sigma(config, e, lv) / act
        if not (0.75 <= ratio <= 1.35):
            print(f"  final e={e} lv={lv}: model/actual σ = {ratio:.2f} 域外 → 不採用",
                  flush=True)
            return None
    r = subprocess.run(
        [TRAIN, "set-mpc", "--eval", BASE_BIN, "--mpc", json_path,
         "--out", bin_path], capture_output=True, text=True, cwd=REPO)
    if r.returncode != 0:
        print(f"  set-mpc failed: {r.stderr.strip()[:200]}", flush=True)
        return None
    return bin_path


def bench(bin_path, problems_range=TUNE_PROBLEMS_RANGE):
    """チューニングベンチの実時間(秒)。timeout/失敗は None。"""
    lo, hi = problems_range
    problems = os.path.join(REPO, "deft-reversi-cli/problem/fforum-60-79.obf")
    subset = "/tmp/deft-mpc-tune-subset.obf"
    with open(problems) as f, open(subset, "w") as out:
        for i, line in enumerate(f):
            if lo <= i < hi:
                out.write(line)
    t0 = time.monotonic()
    try:
        r = subprocess.run(
            [CLI, "--solve", subset, "-l", str(BENCH_LEVEL),
             "--eval-path", bin_path],
            capture_output=True, text=True, timeout=BENCH_TIMEOUT, cwd=REPO)
    except subprocess.TimeoutExpired:
        return None
    if r.returncode != 0 or "level 14" not in r.stdout:
        print(f"  bench output unexpected: rc={r.returncode}", flush=True)
        return None
    return time.monotonic() - t0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out-dir", default="data/log/mpc-tune")
    ap.add_argument("--stage", choices=["eval", "final"], default="eval")
    args = ap.parse_args()
    out_dir = os.path.join(REPO, args.out_dir)
    os.makedirs(out_dir, exist_ok=True)
    rows = load_sweep()
    final_sigma = load_final_sigma()

    log_path = os.path.join(out_dir, f"tuning_log_{args.stage}.csv")
    log = csv.writer(open(log_path, "a", buffering=1))
    log.writerow(["step", "mapping", "seconds"])

    if args.stage == "eval":
        mapping = dict(INITIAL)
        candidates = CANDIDATES
        fixed_final = {int(k): int(v) for k, v in
                       (p.split(":") for p in FINAL_MAPPING.split(","))}
        make = lambda m: (m, fixed_final)
        prange = TUNE_PROBLEMS_RANGE
    else:
        mapping = dict(FINAL_INITIAL)
        candidates = FINAL_CANDIDATES
        make = lambda m: (EVAL_MAPPING_TUNED, m)
        prange = FINAL_TUNE_RANGE

    print(f"stage={args.stage} initial: {mapping_str(mapping)}", flush=True)
    em, fm = make(mapping)
    bin_path = build_and_check(em, fm, rows, final_sigma, "init", out_dir)
    best_time = bench(bin_path, prange) if bin_path else None
    if best_time is None:
        sys.exit("initial mapping failed")
    print(f"initial bench: {best_time:.1f}s", flush=True)
    log.writerow(["init", mapping_str(mapping), f"{best_time:.1f}"])

    for d in sorted(candidates, reverse=True):
        for md in candidates[d]:
            if md == mapping[d]:
                continue
            cand = dict(mapping)
            cand[d] = md
            tag = f"{args.stage}-{d}-{md}"
            print(f"try {d} -> probe {md} ...", flush=True)
            em, fm = make(cand)
            bin_path = build_and_check(em, fm, rows, final_sigma, tag, out_dir)
            if bin_path is None:
                log.writerow([tag, mapping_str(cand), "rejected"])
                continue
            t = bench(bin_path, prange)
            log.writerow([tag, mapping_str(cand), f"{t:.1f}" if t else "timeout"])
            print(f"  -> {t:.1f}s" if t else "  -> timeout", flush=True)
            if t is not None and t < best_time:
                best_time = t
                mapping = cand
                print(f"  adopted (best={best_time:.1f}s)", flush=True)
    print(f"FINAL {args.stage} mapping: {mapping_str(mapping)}  bench={best_time:.1f}s",
          flush=True)
    em, fm = make(mapping)
    final_bin = build_and_check(em, fm, rows, final_sigma,
                                f"best-{args.stage}", out_dir)
    print(f"best bin: {final_bin}", flush=True)


if __name__ == "__main__":
    main()
