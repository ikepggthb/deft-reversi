#!/usr/bin/env python3
"""mpc-collect の出力から MpcConfig JSON をフィットする(numpy のみ使用)。

使い方:
  python3 tools/fit_mpc_nnue.py --collect-dir data/log/mpc-s2-collect --out data/log/mpc-s2.json

入力形式(deft-reversi-cli mpc-collect の出力、3行ヘッダ+CSV5列):
  n_empties, search_level, search_depth_for_prob_cut, search_score, search_score_for_prob_cut
  - mpc_lv{d}_serach.txt   : eval モード(中盤)
  - mpc_perfect_e{e}.txt   : final モード(終盤完全読み)

手順(旧 tools/linear_regression_for_mpc_*.py と同じ2段回帰):
  1. グループごと(eval: (depth, empties) / final: empties)に
     deep = a*shallow + b + e を単回帰し a, b, e_std を得る
  2. グループの (a, b, e_std) を説明変数の線形モデルに重み付き最小二乗で回帰
     eval:  [1, empties, depth, mpc_depth]      (parity 項 0)
     final: [1, empties, mpc_depth, parity, parity*empties, parity*mpc_depth]
"""

import argparse
import glob
import json
import os
import re
import sys

import numpy as np

MIN_GROUP = 15  # 単回帰に使う最小サンプル数
EMPTIES_BIN = 6  # eval モードの empties ビン幅(高レベルの少数データを束ねる)

# e_std は実測 σ をそのまま採用する(信頼度ラベルの統計的正しさを最優先)。
# 大胆さの調節は engine 側の selectivity(z)とプローブ深さマッピングで行う。


def e_std_profile_scale(_depth):
    return 1.0

# deft-reversi-engine/src/search/mpc/config.rs の Default と同一
SEARCH_LV_BY_DEPTH = [
    0, 0, 0, 0, 0, 1, 2, 1, 2, 1, 2, 3, 4, 3, 4, 5, 6, 5, 6, 5, 6, 5, 6, 5, 6, 7,
    8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10,
    9, 10, 9, 10, 9, 10, 9, 10, 9, 10,
]


def search_lv_by_empties_default():
    v = [0] * 61
    fixed = {12: 4, 13: 3, 14: 4, 15: 3, 16: 4, 17: 5, 18: 6, 19: 5, 20: 6}
    for k, x in fixed.items():
        v[k] = x
    for e in range(21, 25):
        v[e] = 5 if e % 2 == 1 else 6
    for e in range(25, 61):
        v[e] = 7 if e % 2 == 1 else 8
    return v


def read_rows(path):
    rows = []
    with open(path) as f:
        for i, line in enumerate(f):
            if i < 3:
                continue
            line = line.strip()
            if not line:
                continue
            rows.append([int(x) for x in line.split(",")])
    return np.array(rows, dtype=np.float64)


def group_regress(deep, shallow):
    """deep = a*shallow + b の単回帰。(a, b, e_std, n) を返す。"""
    n = len(deep)
    var = shallow.var()
    if var < 1e-12:
        return None
    a = ((shallow - shallow.mean()) * (deep - deep.mean())).sum() / (n * var)
    b = deep.mean() - a * shallow.mean()
    e = deep - (a * shallow + b)
    return a, b, e.std(ddof=1), n


def weighted_lstsq(X, y, w):
    sw = np.sqrt(w)
    coef, *_ = np.linalg.lstsq(X * sw[:, None], y * sw, rcond=None)
    pred = X @ coef
    ss_res = (w * (y - pred) ** 2).sum()
    ss_tot = (w * (y - np.average(y, weights=w)) ** 2).sum()
    r2 = 1 - ss_res / ss_tot if ss_tot > 0 else float("nan")
    return coef, r2


def regression_json(coef_map):
    base = {
        "constant": 0.0, "empties": 0.0, "depth": 0.0, "mpc_depth": 0.0,
        "parity": 0.0, "parity_times_empties": 0.0, "parity_times_mpc_depth": 0.0,
    }
    base.update(coef_map)
    return base


def fit_eval(collect_dir):
    groups = []  # (empties, depth, mpc_depth, a, b, e_std, n)
    for path in sorted(glob.glob(os.path.join(collect_dir, "mpc_lv*_serach.txt"))):
        rows = read_rows(path)
        if rows.size == 0:
            continue
        bins = rows[:, 0] // EMPTIES_BIN
        for (bin_id, depth) in np.unique(np.column_stack([bins, rows[:, 1]]), axis=0):
            m = (bins == bin_id) & (rows[:, 1] == depth)
            g = rows[m]
            if len(g) < MIN_GROUP:
                continue
            r = group_regress(g[:, 3], g[:, 4])
            if r is None:
                continue
            a, b, e_std, n = r
            e_std *= e_std_profile_scale(depth)
            empties = g[:, 0].mean()
            mpc_depth = g[0, 2]
            groups.append((empties, depth, mpc_depth, a, b, e_std, n))
    if not groups:
        sys.exit("eval: no groups with enough samples")
    g = np.array(groups)
    X = np.column_stack([np.ones(len(g)), g[:, 0], g[:, 1], g[:, 2]])
    w = g[:, 6]
    out = {}
    for name, col in (("a", 3), ("b", 4), ("e_std", 5)):
        coef, r2 = weighted_lstsq(X, g[:, col], w)
        print(f"eval {name}: const={coef[0]:.6f} empties={coef[1]:.6f} "
              f"depth={coef[2]:.6f} mpc_depth={coef[3]:.6f}  (R2={r2:.3f})")
        out[name] = regression_json({
            "constant": coef[0], "empties": coef[1],
            "depth": coef[2], "mpc_depth": coef[3],
        })
    print(f"eval groups: {len(g)} (samples min={int(w.min())} max={int(w.max())})")
    return out


def fit_final(collect_dir):
    groups = []  # (empties, mpc_depth, parity, a, b, e_std, n)
    for path in sorted(glob.glob(os.path.join(collect_dir, "mpc_perfect_e*.txt"))):
        e = int(re.search(r"_e(\d+)\.txt$", path).group(1))
        rows = read_rows(path)
        if len(rows) < MIN_GROUP:
            print(f"final e{e}: only {len(rows)} rows, skipped", file=sys.stderr)
            continue
        r = group_regress(rows[:, 3], rows[:, 4])
        if r is None:
            continue
        a, b, e_std, n = r
        mpc_depth = rows[0, 2]
        groups.append((e, mpc_depth, e % 2, a, b, e_std, n))
        print(f"final e{e}: a={a:.5f} b={b:.5f} e_std={e_std:.4f} n={n}")
    if not groups:
        sys.exit("final: no groups with enough samples")
    g = np.array(groups)
    X = np.column_stack([
        np.ones(len(g)), g[:, 0], g[:, 1], g[:, 2],
        g[:, 2] * g[:, 0], g[:, 2] * g[:, 1],
    ])
    w = g[:, 6]
    out = {}
    for name, col in (("a", 3), ("b", 4), ("e_std", 5)):
        coef, r2 = weighted_lstsq(X, g[:, col], w)
        print(f"final {name}: const={coef[0]:.6f} empties={coef[1]:.6f} "
              f"mpc_depth={coef[2]:.6f} parity={coef[3]:.6f} "
              f"p*emp={coef[4]:.6f} p*mpc={coef[5]:.6f}  (R2={r2:.3f})")
        out[name] = regression_json({
            "constant": coef[0], "empties": coef[1], "mpc_depth": coef[2],
            "parity": coef[3], "parity_times_empties": coef[4],
            "parity_times_mpc_depth": coef[5],
        })
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--collect-dir", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    eval_reg = fit_eval(args.collect_dir)
    final_reg = fit_final(args.collect_dir)

    config = {
        "eval_search": {
            "search_lv_by_depth": SEARCH_LV_BY_DEPTH,
            "a": eval_reg["a"], "b": eval_reg["b"], "e_std": eval_reg["e_std"],
        },
        "final_search": {
            "search_lv_by_empties": search_lv_by_empties_default(),
            "a": final_reg["a"], "b": final_reg["b"], "e_std": final_reg["e_std"],
        },
    }
    with open(args.out, "w") as f:
        json.dump(config, f, indent=2)
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
