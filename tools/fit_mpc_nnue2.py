#!/usr/bin/env python3
"""プローブ深さスイープ収集から σ(deep, probe) 表を作り、
指定マッピングで MpcConfig JSON をフィットする(numpy のみ)。

使い方:
  # 1) σ 表を眺める
  python3 tools/fit_mpc_nnue2.py --collect-dir data/log/mpc-s2-sweep --table
  # 2) マッピングを指定してフィット
  python3 tools/fit_mpc_nnue2.py --collect-dir data/log/mpc-s2-sweep \
      --eval-mapping "4:0,5:1,6:2,7:3,8:4,9:5,10:6,11:7,12:8,13:9" \
      --final-mapping "12:6,13:5,14:6,15:7,16:8,17:7,18:8,19:9,20:8,21:9,22:10" \
      --out mpc.json

e_std は実測 σ をそのまま使う(信頼度ラベルの統計的正しさを最優先)。
"""

import argparse
import glob
import json
import os
import re
import sys

import numpy as np

MIN_GROUP = 15
EMPTIES_BIN = 6

# 旧デフォルトの search_lv_by_depth(外挿の形状テンプレートに使う)
OLD_LV_BY_DEPTH = [
    0, 0, 0, 0, 0, 1, 2, 1, 2, 1, 2, 3, 4, 3, 4, 5, 6, 5, 6, 5, 6, 5, 6, 5, 6, 7,
    8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 7, 8, 9, 10, 9, 10, 9, 10, 9, 10, 9, 10,
    9, 10, 9, 10, 9, 10, 9, 10, 9, 10,
]


def old_lv_by_empties():
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
            if i < 3 or not line.strip():
                continue
            rows.append([int(x) for x in line.split(",")])
    return np.array(rows, dtype=np.float64)


def load_eval_rows(collect_dir):
    all_rows = []
    for path in sorted(glob.glob(os.path.join(collect_dir, "mpc_lv*_serach.txt"))):
        r = read_rows(path)
        if r.size:
            all_rows.append(r)
    if not all_rows:
        sys.exit("no eval sweep files")
    return np.vstack(all_rows)


def load_final_rows(collect_dir):
    out = {}
    for path in sorted(glob.glob(os.path.join(collect_dir, "mpc_perfect_e*.txt"))):
        e = int(re.search(r"_e(\d+)\.txt$", path).group(1))
        r = read_rows(path)
        if r.size:
            out[e] = r
    if not out:
        sys.exit("no final sweep files")
    return out


def group_regress(deep, shallow):
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


def sigma_of(rows, d, md):
    """(d, md) の全データ一括の a/b/σ(empties 混合)。表表示用。"""
    m = (rows[:, 1] == d) & (rows[:, 2] == md)
    g = rows[m]
    if len(g) < MIN_GROUP:
        return None
    return group_regress(g[:, 3], g[:, 4])


def print_eval_table(rows):
    depths = sorted(set(rows[:, 1].astype(int)))
    print("== eval σ(deep d, probe md)  (全 empties 一括)")
    for d in depths:
        mds = sorted(set(rows[rows[:, 1] == d][:, 2].astype(int)))
        parts = []
        for md in mds:
            r = sigma_of(rows, d, md)
            if r:
                parts.append(f"md{md}:{r[2]:.2f}(n={r[3]})")
        print(f"  d={d}: " + "  ".join(parts))


def print_final_table(final_rows):
    print("== final σ(empties e, probe lv)")
    for e, rows in sorted(final_rows.items()):
        lvs = sorted(set(rows[:, 2].astype(int)))
        parts = []
        for lv in lvs:
            m = rows[:, 2] == lv
            r = group_regress(rows[m, 3], rows[m, 4])
            if r:
                parts.append(f"lv{lv}:{r[2]:.2f}(n={r[3]})")
        print(f"  e={e}: " + "  ".join(parts))


def parse_mapping(s):
    out = {}
    for part in s.split(","):
        k, v = part.split(":")
        out[int(k)] = int(v)
    return out


def regression_json(coef_map):
    base = {
        "constant": 0.0, "empties": 0.0, "depth": 0.0, "mpc_depth": 0.0,
        "parity": 0.0, "parity_times_empties": 0.0, "parity_times_mpc_depth": 0.0,
    }
    base.update(coef_map)
    return base


def fit_eval(rows, mapping):
    groups = []
    for d, md in mapping.items():
        m = (rows[:, 1] == d) & (rows[:, 2] == md)
        r = rows[m]
        if not len(r):
            print(f"warn: eval mapping d={d} md={md} にデータなし", file=sys.stderr)
            continue
        bins = r[:, 0] // EMPTIES_BIN
        for b in np.unique(bins):
            g = r[bins == b]
            if len(g) < MIN_GROUP:
                continue
            reg = group_regress(g[:, 3], g[:, 4])
            if reg is None:
                continue
            a, bb, s, n = reg
            groups.append((g[:, 0].mean(), d, md, a, bb, s, n))
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
    print(f"eval groups: {len(g)}")
    return out


def fit_final(final_rows, mapping):
    groups = []
    for e, lv in mapping.items():
        rows = final_rows.get(e)
        if rows is None:
            print(f"warn: final mapping e={e} にデータなし", file=sys.stderr)
            continue
        m = rows[:, 2] == lv
        g = rows[m]
        if len(g) < MIN_GROUP:
            print(f"warn: final e={e} lv={lv} n={len(g)} 不足", file=sys.stderr)
            continue
        reg = group_regress(g[:, 3], g[:, 4])
        if reg is None:
            continue
        a, b, s, n = reg
        groups.append((e, lv, e % 2, a, b, s, n))
        print(f"final e{e} lv{lv}: a={a:.5f} b={b:.5f} e_std={s:.4f} n={n}")
    g = np.array(groups)
    # 交互作用項は入れない(グループ数 ~11 に対し 6 パラメータは過学習し、
    # データ範囲外で b/e_std が暴れて終盤探索が不安定化するため)。
    X = np.column_stack([np.ones(len(g)), g[:, 0], g[:, 1], g[:, 2]])
    w = g[:, 6]
    out = {}
    for name, col in (("a", 3), ("b", 4), ("e_std", 5)):
        coef, r2 = weighted_lstsq(X, g[:, col], w)
        print(f"final {name}: const={coef[0]:.6f} empties={coef[1]:.6f} "
              f"mpc_depth={coef[2]:.6f} parity={coef[3]:.6f}  (R2={r2:.3f})")
        out[name] = regression_json({
            "constant": coef[0], "empties": coef[1], "mpc_depth": coef[2],
            "parity": coef[3],
        })
    return out


def build_lv_by_depth(mapping):
    """収集範囲は指定値、範囲外は旧デフォルトの形状を最大深収集点との差で平行移動。"""
    v = list(OLD_LV_BY_DEPTH)
    d_max = max(mapping)
    shift = mapping[d_max] - OLD_LV_BY_DEPTH[d_max]
    for d in range(len(v)):
        if d in mapping:
            v[d] = mapping[d]
        elif d > d_max:
            v[d] = max(0, min(d - 2, OLD_LV_BY_DEPTH[d] + shift))
    return v


def build_lv_by_empties(mapping):
    v = old_lv_by_empties()
    e_max = max(mapping)
    shift = mapping[e_max] - v[e_max]
    for e in range(len(v)):
        if e in mapping:
            v[e] = mapping[e]
        elif e > e_max and v[e] > 0:
            v[e] = max(1, min(e - 2, v[e] + shift))
    return v


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--collect-dir", required=True)
    ap.add_argument("--table", action="store_true")
    ap.add_argument("--eval-mapping")
    ap.add_argument("--final-mapping")
    ap.add_argument("--out")
    args = ap.parse_args()

    eval_rows = load_eval_rows(args.collect_dir)
    final_rows = load_final_rows(args.collect_dir)

    if args.table:
        print_eval_table(eval_rows)
        print_final_table(final_rows)
        return

    if not (args.eval_mapping and args.final_mapping and args.out):
        sys.exit("--eval-mapping / --final-mapping / --out が必要(または --table)")

    em = parse_mapping(args.eval_mapping)
    fm = parse_mapping(args.final_mapping)
    eval_reg = fit_eval(eval_rows, em)
    final_reg = fit_final(final_rows, fm)

    config = {
        "eval_search": {
            "search_lv_by_depth": build_lv_by_depth(em),
            "a": eval_reg["a"], "b": eval_reg["b"], "e_std": eval_reg["e_std"],
        },
        "final_search": {
            "search_lv_by_empties": build_lv_by_empties(fm),
            "a": final_reg["a"], "b": final_reg["b"], "e_std": final_reg["e_std"],
        },
    }
    with open(args.out, "w") as f:
        json.dump(config, f, indent=2)
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
