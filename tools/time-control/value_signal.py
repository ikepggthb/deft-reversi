#!/usr/bin/env python3
"""Can the clock tell, from what it sees, whether the next iteration will
play a better move? (stage 4-2, roadmap B1)

usage: value_signal.py table RUN_DIR REFER_DIR > pairs.tsv
       value_signal.py fit pairs.tsv

`table` writes one row per pair of consecutive finished iterations (k,
k+1) of the logged turns that refer_loss.py covers (normal games of
openings 200..299, the extended turns of extension games of 300..449):
features known when iteration k ends, and the label `better` = 1 if the
move of k+1 has less loss than the move of k (stage 3-2 reference), and
`gain` = loss(k) - loss(k+1).

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


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "table":
        table(sys.argv[2], sys.argv[3])
    elif len(sys.argv) > 1 and sys.argv[1] == "fit":
        fit(sys.argv[2])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
