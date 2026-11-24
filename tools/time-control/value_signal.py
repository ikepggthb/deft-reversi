#!/usr/bin/env python3
"""Can the clock tell, from what it sees, whether the next iteration will
play a better move? (stage 4-2, roadmap B1)

usage: value_signal.py table RUN_DIR REFER_DIR > pairs.tsv
       value_signal.py fit pairs.tsv
       value_signal.py positions RUN_DIR N_EXACT N_MID > positions.tsv
       value_signal.py gapmissing GAP_DIR REFER_DIR > missing.tsv
       value_signal.py gapfit GAP_DIR REFER_DIR

`table` writes one row per pair of consecutive finished iterations (k,
k+1) of the logged turns that refer_loss.py covers (normal games of
openings 200..299, the extended turns of extension games of 300..449):
features known when iteration k ends, and the label `better` = 1 if the
move of k+1 has less loss than the move of k (stage 3-2 reference), and
`gain` = loss(k) - loss(k+1).

`positions` samples searched positions of the normal games (openings
200..299) for the gap probe (probe/src/bin/gap.rs): N_EXACT with 20 to 28
empties (where the reference is exact) and N_MID with 29 to 44, each
position once, in a fixed random order (seed 20261124).

`gapmissing` lists, in refer_input.py's format, the moves of the gap
probe's output (GAP_DIR/out-*.tsv: each iteration's best move and the
second move of its all-moves search) that REFER_DIR does not have, for a
supplementary refer_log run into a subdirectory of REFER_DIR.

`gapfit` labels the probe's consecutive iterations as `table` does (the
next iteration's move has less loss) and compares, with 5-fold
cross-validation by position, logistic regressions on: legacy's
instability; what the logs have (best-move changes, score, depth, nodes);
that plus the free root counters of Progress::root (share of nodes off the
first move, fail-highs, aspiration fails); that plus the true gap to the
second move (which a game cannot afford). Separately for positions with at
most 28 empties (exact reference) and 29 to 44 with the next iteration at
most d22. Also: how often the all-moves second move is in fact better than
the iteration's best by the reference.

`fit` fits logistic regressions on feature sets of growing size, with
5-fold cross-validation by opening (all games of an opening in one fold),
and prints for the held-out predictions: AUC, the true positive rate at
false positive rates 0.10 and 0.15, and the mean gain of the turns flagged
at FPR 0.15. Stage 4-1 part 3 found that a signal with TPR 0.5 at FPR
0.15 in place of the instability cuts 0.04-0.16 discs per game and side;
legacy's instability >= 0.5 has TPR about 0.3 at FPR 0.16. Rows are also
broken down by transition and by condition for the largest set.
"""
import json, math, random, sys
from collections import defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from refer_input import positions, turns  # noqa: E402
from refer_loss import reference, NORMAL, EXTEND  # noqa: E402

TRS = ("mid", "entry", "stage", "exact")
STAGES = {"e85": 0, "e95": 1, "e98": 2, "e99": 3, "e100": 4}
HEAD = ("cond opening player empties k tag next tr better gain "
        "threads mobility opp_mobility regions odd_regions warm depth stage "
        "log_cost log_nodes log_ratio changed changes since_change inst "
        "score dscore pv_len log_pred_ratio").split()


def transition(a, b):
    if b[0] == "d":
        return "mid" if a[0] == "d" else "back"
    if a[0] == "d":
        return "entry"
    return "exact" if b == "e100" else "stage"


def table(run, refer):
    ref = reference(refer)
    print("\t".join(HEAD))
    for cond in NORMAL + EXTEND:
        out = Path(run) / f"{cond}.tsv"
        if not out.exists():
            continue
        first, last = (200, 299) if cond in NORMAL else (300, 449)
        records = {}
        for line in out.read_text().splitlines()[1:]:
            f = line.split("\t")
            records[(int(f[0]), f[1])] = f[6]
        for line in out.with_suffix(".logs.tsv").read_text().splitlines()[1:]:
            w = line.split("\t")
            opening, nb = int(w[0]), w[1]
            if not first <= opening <= last:
                continue
            pos = positions(records[(opening, nb)])
            for player, (name, a, b) in (("new", w[3:6]), ("old", w[6:9])):
                black = (player == "new") == (nb == "1")
                for its, t in turns(out.parent / cond / name, int(a), int(b)):
                    if not t["searched"] or t["after_extension"]:
                        continue
                    if cond in EXTEND and not t["extended"]:
                        continue
                    its = [i for i in its if i["best_move"]]
                    if len(its) < 2:
                        continue
                    bw = pos[(t["empties"], black)]
                    key = f"{bw[0]:016x}{bw[1]:016x}{'b' if black else 'w'}"
                    best, values = ref[key]
                    loss = [best - values[i["best_move"]] if i["best_move"] in values else None
                            for i in its]
                    warm = int(not (its[0]["kind"][0] == "d" and int(its[0]["kind"][1:]) <= 6))
                    changes, since = 0, 0
                    for k in range(len(its) - 1):
                        i, j = its[k], its[k + 1]
                        changed = int(k > 0 and its[k - 1]["best_move"] != i["best_move"])
                        changes += changed
                        since = 0 if changed else since + 1
                        if loss[k] is None or loss[k + 1] is None:
                            continue
                        tr = transition(i["kind"], j["kind"])
                        if tr == "back":
                            continue
                        prev = its[k - 1] if k > 0 else None
                        kind = i["kind"]
                        row = [cond, opening, player, t["empties"], k, kind, j["kind"], tr,
                               int(loss[k + 1] < loss[k]), loss[k] - loss[k + 1],
                               t["threads"], t["mobility"], t["opponent_mobility"],
                               t["regions"], t["odd_regions"], warm,
                               int(kind[1:]) if kind[0] == "d" else 0,
                               STAGES.get(kind, -1) + 1,
                               f"{math.log(max(i['cost'], 1e-5)):.4f}",
                               f"{math.log(max(i['nodes'], 1)):.4f}",
                               f"{math.log(max(i['cost'], 1e-5) / max(prev['cost'], 1e-5)):.4f}"
                               if prev else "0",
                               changed, changes, since, f"{i['instability']:.4f}",
                               i["score"], i["score"] - prev["score"] if prev else 0,
                               len(i["pv"]),
                               f"{math.log(max(i['predicted'], 1e-5) / max(i['elapsed'], 1e-5)):.4f}"
                               if i["predicted"] else "0"]
                        print("\t".join(map(str, row)))


# ---- fit ----

def features(r, name):
    """Feature vector of one row for the named set."""
    tr = [float(r["tr"] == x) for x in TRS]
    sc = float(r["score"])
    ds = float(r["dscore"])
    base = tr
    if name == "instability":
        return base + [float(r["inst"])]
    v = base + [float(r["changed"]), float(r["inst"]), min(float(r["changes"]), 6),
                min(float(r["since_change"]), 6)]
    if name == "changes":
        return v
    v += [abs(ds), min(abs(ds), 4), abs(sc), float(abs(sc) <= 2), float(r["pv_len"]) / 10]
    if name == "+score":
        return v
    e = float(r["empties"])
    v += [e / 10, (e / 10) ** 2, float(r["depth"]) / 10, float(r["stage"]),
          float(r["k"]) / 10, float(r["log_cost"]), float(r["log_nodes"]) / 10,
          float(r["log_ratio"]), float(r["warm"]), float(r["log_pred_ratio"])]
    if name == "+search":
        return v
    v += [float(r["mobility"]) / 10, float(r["opp_mobility"]) / 10, float(r["regions"]),
          float(r["odd_regions"]), float(r["threads"] == "4")]
    if name == "+board":
        return v
    # all, with the transition interacting with the main terms
    inter = [t * x for t in tr for x in (float(r["changed"]), float(r["inst"]), abs(ds),
                                          e / 10, float(r["depth"]) / 10)]
    return v + inter


SETS = ("instability", "changes", "+score", "+search", "+board", "all")


def logistic(x, y, l2=1e-3, iters=60):
    """Newton's method on the L2-penalised log-likelihood."""
    n, d = x.shape
    w = np.zeros(d)
    for _ in range(iters):
        p = 1 / (1 + np.exp(-(x @ w)))
        g = x.T @ (p - y) / n + l2 * w
        h = (x * (p * (1 - p))[:, None]).T @ x / n + l2 * np.eye(d)
        step = np.linalg.solve(h, g)
        w -= step
        if np.max(np.abs(step)) < 1e-6:
            break
    return w


def auc(s, y):
    order = np.argsort(s, kind="mergesort")
    ranks = np.empty(len(s))
    ranks[order] = np.arange(1, len(s) + 1)
    # average ranks of ties
    ss = s[order]
    i = 0
    while i < len(ss):
        j = i
        while j + 1 < len(ss) and ss[j + 1] == ss[i]:
            j += 1
        if j > i:
            ranks[order[i:j + 1]] = (i + j + 2) / 2
        i = j + 1
    pos = y == 1
    npos, nneg = pos.sum(), (~pos).sum()
    return (ranks[pos].sum() - npos * (npos + 1) / 2) / (npos * nneg)


def at_fpr(s, y, gain, fpr):
    """TPR, threshold-flagged mean gain, and flagged share at the given FPR."""
    neg = np.sort(s[y == 0])
    thr = neg[int(len(neg) * (1 - fpr))] if len(neg) else 0
    flag = s > thr
    tpr = flag[y == 1].mean()
    return tpr, gain[flag].mean() if flag.any() else float("nan"), flag.mean()


def fit(path):
    rows = []
    head = None
    for line in open(path):
        w = line.rstrip("\n").split("\t")
        if head is None:
            head = w
            continue
        rows.append(dict(zip(head, w)))
    y = np.array([int(r["better"]) for r in rows], dtype=float)
    gain = np.array([float(r["gain"]) for r in rows])
    openings = sorted({r["opening"] for r in rows})
    rng = random.Random(20261124)
    rng.shuffle(openings)
    fold = {o: i % 5 for i, o in enumerate(openings)}
    f = np.array([fold[r["opening"]] for r in rows])
    print(f"# pairs {len(rows)}, better {y.mean():.3f}, openings {len(openings)};"
          " 5-fold CV by opening; TPR at FPR 0.10 / 0.15, mean gain (discs) of the flagged"
          " pairs at FPR 0.15 and of all pairs")
    print(f"# legacy rule for reference: instability >= 0.5 -> TPR "
          f"{np.mean([float(r['inst']) >= 0.5 for r in rows if r['better'] == '1']):.3f}"
          f" at FPR {np.mean([float(r['inst']) >= 0.5 for r in rows if r['better'] == '0']):.3f}")
    held = {}
    for name in SETS:
        x = np.array([features(r, name) for r in rows])
        mu, sd = x.mean(0), x.std(0)
        sd[sd == 0] = 1
        x = np.hstack([np.ones((len(x), 1)), (x - mu) / sd])
        s = np.zeros(len(rows))
        for k in range(5):
            tr = f != k
            w = logistic(x[tr], y[tr])
            s[~tr] = x[~tr] @ w
        held[name] = s
        t10 = at_fpr(s, y, gain, 0.10)
        t15 = at_fpr(s, y, gain, 0.15)
        print(f"{name:12s}\tAUC {auc(s, y):.3f}\tTPR {t10[0]:.3f} / {t15[0]:.3f}"
              f"\tflagged gain {t15[1]:.3f} (all {gain.mean():.3f})")
    s = held["all"]
    inst = np.array([float(r["inst"]) for r in rows])
    print("# by transition: legacy's instability >= 0.5 (TPR / FPR), and the largest set's TPR"
          " at legacy's FPR and at FPR 0.15 (threshold per transition). The pooled rates above"
          " mix transitions whose base rates differ")
    for g in TRS:
        idx = np.array([i for i, r in enumerate(rows) if r["tr"] == g])
        yy = y[idx]
        lt = (inst[idx] >= 0.5)[yy == 1].mean()
        lf = (inst[idx] >= 0.5)[yy == 0].mean()
        m_at = at_fpr(s[idx], yy, gain[idx], lf)[0]
        m15 = at_fpr(s[idx], yy, gain[idx], 0.15)[0]
        print(f"tr {g:6s}\tn {len(idx)}\tbetter {yy.mean():.3f}\tlegacy {lt:.3f} / {lf:.3f}"
              f"\tmodel at legacy's FPR {m_at:.3f}\tmodel at 0.15 {m15:.3f}"
              f"\tAUC {auc(s[idx], yy):.3f}")
    print("# the largest set, by transition and by condition (threshold per group at FPR 0.15)")
    for label, key in (("tr", "tr"), ("cond", "cond")):
        groups = defaultdict(list)
        for i, r in enumerate(rows):
            groups[r[key]].append(i)
        for g in sorted(groups):
            idx = np.array(groups[g])
            if y[idx].sum() < 20 or (1 - y[idx]).sum() < 20:
                continue
            t = at_fpr(s[idx], y[idx], gain[idx], 0.15)
            print(f"{label} {g:10s}\tn {len(idx)}\tbetter {y[idx].mean():.3f}"
                  f"\tAUC {auc(s[idx], y[idx]):.3f}\tTPR@0.15 {t[0]:.3f}")


def gap_rows(gap_dir):
    """{key: [iteration rows]} from the gap probe."""
    out = defaultdict(list)
    for path in sorted(Path(gap_dir).glob("out-*.tsv")):
        for line in open(path):
            w = line.rstrip("\n").split("\t")
            if len(w) < 15:
                continue
            out[w[0]].append({
                "empties": int(w[1]), "k": int(w[2]), "kind": w[3], "best": w[4].lower(),
                "score": int(w[5]), "nodes": int(w[6]), "elapsed": float(w[7]),
                "first_nodes": int(w[8]), "window_nodes": int(w[9]),
                "fail_highs": int(w[10]), "asp": int(w[11]),
                "gap": None if w[12] == "-" else int(w[12]),
                "second": None if w[13] == "-" else w[13].lower()})
    for rows in out.values():
        rows.sort(key=lambda r: r["k"])
    return out


def gap_missing(gap_dir, refer):
    ref = reference(refer)
    pos = {}
    for line in open(Path(gap_dir) / "positions.tsv"):
        w = line.rstrip("\n").split("\t")
        pos[w[0]] = w
    for key, rows in gap_rows(gap_dir).items():
        have = set(ref[key][1]) if key in ref else set()
        want = {r["best"] for r in rows} | {r["second"] for r in rows if r["second"]}
        want = sorted(m for m in want - have if m not in ("-", "pass"))
        if want:
            w = pos[key]
            print(f"{key}\t{w[1]}\t{w[2]}\t{w[3]}\t{w[4]}\t{','.join(want)}\tgap\t0\t0\tnew")


GAP_SETS = ("legacy", "log", "+free", "+gap")


def gap_features(rows, k, name):
    r, n = rows[k], rows[k + 1]
    tr = transition(r["kind"], n["kind"])
    trv = [float(tr == x) for x in TRS]
    inst, changes, since = 0.0, 0, 0
    for j in range(k + 1):
        ch = j > 0 and rows[j - 1]["best"] != rows[j]["best"]
        inst = 1.0 if ch else inst * 0.5
        changes += ch
        since = 0 if ch else since + 1
    if name == "legacy":
        return trv + [inst]
    prev = rows[k - 1] if k > 0 else r
    v = trv + [inst, float(since == 0 and k > 0), min(changes, 6), min(since, 6),
               abs(r["score"] - prev["score"]), abs(r["score"]), float(abs(r["score"]) <= 2),
               r["empties"] / 10, (int(r["kind"][1:]) if r["kind"][0] == "d" else 0) / 10,
               k / 10, math.log(max(r["nodes"], 1)) / 10,
               math.log(max(r["nodes"], 1) / max(prev["nodes"], 1))]
    if name == "log":
        return v
    off = 1 - r["first_nodes"] / r["window_nodes"] if r["window_nodes"] else 0.0
    v += [off, float(off > 0.5), min(r["fail_highs"], 4), float(r["fail_highs"] > 0),
          min(r["asp"], 4), float(r["asp"] > 0)]
    if name == "+free":
        return v
    g = r["gap"]
    return v + [min(g, 10) / 10, float(g == 0), float(g <= 1), float(g <= 2)]


def gap_fit(gap_dir, refer):
    ref = reference(refer)
    data = {"exact": [], "mid": []}
    second_better = defaultdict(lambda: [0, 0])
    missing = 0
    for key, rows in gap_rows(gap_dir).items():
        if key not in ref:
            continue
        best, values = ref[key]
        for k in range(len(rows) - 1):
            r, n = rows[k], rows[k + 1]
            if r["gap"] is None:
                continue
            if r["best"] not in values or n["best"] not in values:
                missing += 1
                continue
            tr = transition(r["kind"], n["kind"])
            if tr == "back":
                continue
            e = r["empties"]
            if e <= 28:
                group = "exact"
            elif n["kind"][0] == "d" and int(n["kind"][1:]) <= 22:
                group = "mid"
            else:
                continue
            lk, ln = best - values[r["best"]], best - values[n["best"]]
            data[group].append((key, rows, k, tr, int(ln < lk), lk - ln))
            if r["second"] in values:
                sb = second_better[(group, tr)]
                sb[0] += values[r["second"]] > values[r["best"]]
                sb[1] += 1
    print(f"# gap probe pairs; skipped for moves without a reference value: {missing}")
    for group, rs in data.items():
        if not rs:
            continue
        y = np.array([x[4] for x in rs], dtype=float)
        gain = np.array([x[5] for x in rs], dtype=float)
        keys = sorted({x[0] for x in rs})
        rng = random.Random(20261124)
        rng.shuffle(keys)
        fold = {kk: i % 5 for i, kk in enumerate(keys)}
        f = np.array([fold[x[0]] for x in rs])
        trs = np.array([x[3] for x in rs])
        print(f"## {group}: pairs {len(rs)}, positions {len(keys)}, better {y.mean():.3f}")
        held = {}
        for name in GAP_SETS:
            x = np.array([gap_features(x_[1], x_[2], name) for x_ in rs])
            mu, sd = x.mean(0), x.std(0)
            sd[sd == 0] = 1
            x = np.hstack([np.ones((len(x), 1)), (x - mu) / sd])
            s_ = np.zeros(len(rs))
            for k in range(5):
                t = f != k
                w = logistic(x[t], y[t])
                s_[~t] = x[~t] @ w
            held[name] = s_
        inst = np.array([gap_features(x_[1], x_[2], "legacy")[-1] for x_ in rs])
        for tr in TRS:
            idx = np.where(trs == tr)[0]
            yy = y[idx]
            if yy.sum() < 10 or (1 - yy).sum() < 10:
                print(f"{group}\t{tr}\tpairs {len(idx)}\tbetter {int(yy.sum())} (too few)")
                continue
            lt = (inst[idx] >= 0.5)[yy == 1].mean()
            lf = (inst[idx] >= 0.5)[yy == 0].mean()
            cells = []
            for name in GAP_SETS:
                tpr = at_fpr(held[name][idx], yy, gain[idx], lf)[0]
                cells.append(f"{name} AUC {auc(held[name][idx], yy):.3f} TPR {tpr:.3f}")
            print(f"{group}\t{tr}\tpairs {len(idx)}\tbetter {yy.mean():.3f}"
                  f"\tlegacy rule {lt:.3f}/{lf:.3f}\t" + "\t".join(cells))
    print("# the all-moves second move better than the iteration's best (reference)")
    for (group, tr), (a, n) in sorted(second_better.items()):
        print(f"{group}\t{tr}\t{a}/{n}\t{a / n:.3f}")


def sample(run, n_exact, n_mid):
    seen = {}
    for cond in NORMAL:
        out = Path(run) / f"{cond}.tsv"
        records = {}
        for line in out.read_text().splitlines()[1:]:
            f = line.split("\t")
            records[(int(f[0]), f[1])] = f[6]
        for line in out.with_suffix(".logs.tsv").read_text().splitlines()[1:]:
            w = line.split("\t")
            opening, nb = int(w[0]), w[1]
            if not 200 <= opening <= 299:
                continue
            pos = positions(records[(opening, nb)])
            for player, (name, a, b) in (("new", w[3:6]), ("old", w[6:9])):
                black = (player == "new") == (nb == "1")
                for _, t in turns(out.parent / cond / name, int(a), int(b)):
                    e = t["empties"]
                    if not t["searched"] or not 20 <= e <= 44:
                        continue
                    bw = pos[(e, black)]
                    key = f"{bw[0]:016x}{bw[1]:016x}{'b' if black else 'w'}"
                    seen.setdefault(key, (bw, black, e))
    rng = random.Random(20261124)
    keys = sorted(seen)
    rng.shuffle(keys)
    exact = [k for k in keys if seen[k][2] <= 28][:n_exact]
    mid = [k for k in keys if seen[k][2] > 28][:n_mid]
    for k in exact + mid:
        bw, black, e = seen[k]
        print(f"{k}\t{bw[0]:016x}\t{bw[1]:016x}\t{'b' if black else 'w'}\t{e}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "gapmissing":
        gap_missing(sys.argv[2], sys.argv[3])
    elif len(sys.argv) > 1 and sys.argv[1] == "gapfit":
        gap_fit(sys.argv[2], sys.argv[3])
    elif len(sys.argv) > 1 and sys.argv[1] == "positions":
        sample(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]))
    elif len(sys.argv) > 1 and sys.argv[1] == "table":
        table(sys.argv[2], sys.argv[3])
    elif len(sys.argv) > 1 and sys.argv[1] == "fit":
        fit(sys.argv[2])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
