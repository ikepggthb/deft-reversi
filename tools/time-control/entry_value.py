#!/usr/bin/env python3
"""The value of ENTRY on extended turns (stage 3-1 issue 1, run x-gtp20e).

usage: entry_value.py missing RUN_DIR COND > positions.tsv
       entry_value.py summary RUN_DIR REFER_DIR COND

ENTRY is the iteration that moves from the last midgame depth to the first
endgame stage (e85: to the end of the game at 85% MPC). Stage 3-2 found it
worth 1.0-1.4 discs on the turns where it finished, but those were light:
about 70% of the heavy ones did not finish within the extension. COND is an
extension run (x-gtp20e: 20 s games, 24-32 empties, 30 s extension; the
stage 3-1 runs x-gtp20 etc. work too), RUN_DIR the stage3.sh output and
REFER_DIR the stage 3-2 reference with its supplementary runs.

`missing` lists, in refer_input.py's format, the extended turns' positions
and the moves their iterations chose (each position once), for refer_log.

`summary` takes every extended turn whose clock decided to stop in the
midgame (before ENTRY) and prints, with 95% intervals from a bootstrap
over openings:
  1. how many reached ENTRY within the extension, how many finished it,
     and ENTRY's time against the turn's soft and hard;
  2. the loss of the move at the clock's decision, before ENTRY (after the
     midgame steps the extension added) and after ENTRY, so the value of
     the midgame steps and of ENTRY, by empties and by ENTRY's time over
     hard; turns whose ENTRY did not finish count with the move they had
     (no value), so the means are what a turn could expect;
  3. the endgame stages after ENTRY (e95 .. e100) on the same turns;
  4. the turns whose clock decided in the endgame: the loss at the
     decision minus the loss at the end of the extension, and how often
     the exact solve (e100) finished within it.
Positions with at most 28 empties have exact references; above that the
reference is a depth-26 search at 85% MPC, unless REFER_DIR lists an exact
run first (`refer_log ... 0`, comma-separated: EXACT_DIR,REFER_DIR).
"""
import random, sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from refer_input import positions, turns  # noqa: E402
from refer_loss import reference, openings  # noqa: E402


def extended_turns(run, cond):
    """(opening, nb, key, black, white, side, empties, turn, iterations)."""
    out = Path(run) / f"{cond}.tsv"
    first, last = openings(cond)
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
        name, a, b = w[3:6]
        black = nb == "1"
        for its, t in turns(out.parent / cond / name, int(a), int(b)):
            if not t["extended"] or not t["searched"]:
                continue
            bw = pos[(t["empties"], black)]
            key = f"{bw[0]:016x}{bw[1]:016x}{'b' if black else 'w'}"
            yield opening, nb, key, bw, black, t, its


def missing(run, cond, refer=None):
    have = {}
    if refer:
        have = {k: set(v[1]) for k, v in reference(refer).items()}
    seen = {}
    for opening, nb, key, bw, black, t, its in extended_turns(run, cond):
        moves = {i["best_move"] for i in its if i["best_move"]} | {t["played"]}
        moves = {m for m in moves if m and m != "pass"} - have.get(key, set())
        if not moves:
            continue
        if key in seen:
            seen[key][1] |= moves
            continue
        seen[key] = [(bw, black, t["empties"], opening, nb), moves]
    for key, ((bw, black, e, opening, nb), moves) in seen.items():
        print(f"{key}\t{bw[0]:016x}\t{bw[1]:016x}\t{'b' if black else 'w'}\t{e}"
              f"\t{','.join(sorted(moves))}\t{cond}\t{opening}\t{nb}\tnew")


class Groups:
    def __init__(self):
        self.v = defaultdict(lambda: defaultdict(list))

    def add(self, key, opening, x):
        self.v[key][opening].append(x)

    def line(self, key, rng):
        g = self.v[key]
        ops = sorted(g)

        def mean(sample):
            s = n = 0
            for o in sample:
                s += sum(g[o])
                n += len(g[o])
            return s / n if n else float("nan")
        boots = sorted(mean([rng.choice(ops) for _ in ops]) for _ in range(2000))
        n = sum(len(x) for x in g.values())
        return f"n={n}\t{mean(ops):+.3f} [{boots[50]:+.3f}, {boots[1949]:+.3f}]"


def band(e):
    return "24-26" if e <= 26 else "27-28" if e <= 28 else "29-32" if e <= 32 else f"{e}"


def heavy(cost, t):
    r = cost / t["hard"]
    return "<1 hard" if r < 1 else "1-3 hard" if r < 3 else ">=3 hard"


def references(refer):
    """REFER may list several directories, `A,B`: a position's best move and
    values come from the first that has it (e.g. an exact run, then the
    stage 3-2 reference)."""
    ref = {}
    for d in refer.split(","):
        for k, v in reference(d).items():
            ref.setdefault(k, v)
    return ref


def summary(run, refer, cond):
    ref = references(refer)
    rng = random.Random(20261124)
    count = defaultdict(int)
    mid, ent, stages, changed = Groups(), Groups(), Groups(), Groups()
    endg, exact_done = Groups(), Groups()
    times = defaultdict(list)
    lost = 0
    for opening, nb, key, bw, black, t, its in extended_turns(run, cond):
        its = [i for i in its if i["best_move"]]
        e = t["empties"]
        if key not in ref or not its:
            lost += 1
            continue
        best, values = ref[key]

        def loss(m):
            return best - values[m] if m in values else None
        dec_el = t["decision_elapsed"]
        done = [k for k, i in enumerate(its) if dec_el is not None and i["elapsed"] <= dec_el + 1e-9]
        if not done:
            count["no iteration at the decision"] += 1
            continue
        d = done[-1]
        if its[d]["kind"][0] != "d":
            count[f"{band(e)}: decided in the endgame"] += 1
            l_dec, l_end = loss(its[d]["best_move"]), loss(its[-1]["best_move"])
            if l_dec is not None and l_end is not None:
                endg.add((band(e), f"after deciding at {its[d]['kind']} (to the end)"),
                         opening, l_dec - l_end)
                endg.add((band(e), "endgame decision: value of the extension"), opening,
                         l_dec - l_end)
                exact_done.add((band(e), "endgame decision: exact solve finished"), opening,
                               int(any(i["kind"] == "e100" for i in its)))
            continue
        # ENTRY: the first endgame iteration after the decision.
        k_entry = next((k for k in range(d + 1, len(its)) if its[k]["kind"][0] == "e"), None)
        last_mid = k_entry - 1 if k_entry is not None else len(its) - 1
        reached = k_entry is not None or (its[-1]["next"] or "")[:1] == "e"
        l_dec, l_mid = loss(its[d]["best_move"]), loss(its[last_mid]["best_move"])
        if l_dec is None or l_mid is None:
            lost += 1
            continue
        count[f"{band(e)}: decided in the midgame"] += 1
        if not reached:
            count[f"{band(e)}:   ENTRY not reached within the extension"] += 1
        elif k_entry is None:
            count[f"{band(e)}:   ENTRY started, not finished"] += 1
        else:
            count[f"{band(e)}:   ENTRY finished"] += 1
        mid.add(("all", "mid steps after the decision"), opening, l_dec - l_mid)
        mid.add((band(e), "mid steps after the decision"), opening, l_dec - l_mid)
        if k_entry is None:
            if reached:
                # Counts with no value: the move stays the midgame's.
                ent.add(("all", "ENTRY (unfinished = 0)"), opening, 0)
                ent.add((band(e), "ENTRY (unfinished = 0)"), opening, 0)
            continue
        cost = its[k_entry]["cost"]
        times[band(e)].append((cost, cost / t["soft"], cost / t["hard"]))
        l_ent = loss(its[k_entry]["best_move"])
        if l_ent is None:
            lost += 1
            continue
        for g in ("all", band(e)):
            ent.add((g, "ENTRY (unfinished = 0)"), opening, l_mid - l_ent)
            ent.add((g, "ENTRY finished"), opening, l_mid - l_ent)
            changed.add((g, "ENTRY changed the move"), opening,
                        int(its[k_entry]["best_move"] != its[last_mid]["best_move"]))
        ent.add((heavy(cost, t), "ENTRY finished"), opening, l_mid - l_ent)
        l_end = loss(its[-1]["best_move"])
        if l_end is not None and len(its) > k_entry + 1:
            stages.add((band(e), f"stages after ENTRY (to {its[-1]['kind']})"), opening,
                       l_ent - l_end)
    print(f"# {cond}: extended turns; skipped (no reference or no iteration): {lost}")
    for k in sorted(count):
        print(f"{k}\t{count[k]}")
    print("# ENTRY time (s), and over the turn's soft and hard: median [10%, 90%]")
    for b, v in sorted(times.items()):
        cols = []
        for j, name in enumerate(("seconds", "/soft", "/hard")):
            xs = sorted(x[j] for x in v)
            cols.append(f"{name} {xs[len(xs) // 2]:.2f} [{xs[len(xs) // 10]:.2f}, "
                        f"{xs[len(xs) * 9 // 10]:.2f}]")
        print(f"{b}\tn={len(v)}\t" + "\t".join(cols))
    print("# value (loss before - loss after, discs per turn) and share where ENTRY changed the move")
    for gr in (mid, ent, changed, stages, endg, exact_done):
        for key in sorted(gr.v):
            print(f"{key[0]}\t{key[1]}\t{gr.line(key, rng)}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "missing":
        missing(sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else None)
    elif len(sys.argv) > 1 and sys.argv[1] == "summary":
        summary(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
