#!/usr/bin/env python3
"""Losses of the logged searches against the stage 3-2 reference.

usage: refer_loss.py table RUN_DIR REFER_DIR > iterations.tsv
       refer_loss.py summary iterations.tsv

`table` replays the turns that refer_input.py took (normal games of
openings 200..299, the extended turn of extension games of 300..449) and
writes one row per iteration with the loss of the move it chose: the
reference value of the best move (the unrestricted search) minus the value
of that move. Positions with at most 28 empties are exact; above that the
reference is a depth-26 search at 85% MPC, so a loss can be negative (the
reference preferred its best move by a noisy margin) and midgame
iterations near depth 26 are measured against little more than themselves.

Columns: cond opening new_black player empties k tag next elapsed cost
move loss played_loss stop_reason extended after_decision decision.
A turn whose first iteration ran into hard (no iteration finished; the
search had warm-started from the table at a deep iteration) has one row
with k = -1, tag -, elapsed = the turn's time and the move played.
`next` is the kind of the following iteration of the turn if it finished
(else -), `after_decision` is 1 for iterations that ended after the clock
had decided to stop (extended turns only), `decision` is 1 on the last
iteration the clock had when it decided.

`summary` prints, with 95% intervals from a bootstrap over openings (the
colour-swapped games of one opening nearly repeat each other):
  1. the loss of the played move per turn and per game, by empties band;
     the turns without a finished iteration, their loss and time;
  2. the gain of each finished next iteration, loss(k) - loss(k+1), by
     transition (mid, entry, stage, exact); for extension games only the
     iterations that ended after the clock's decision (those it gave up);
  3. on extended turns, the loss at the clock's decision minus the loss
     at the end of the extension: what stopping where the clock stopped
     cost against searching on.
"""
import json, random, sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from refer_input import positions, turns  # noqa: E402

HEAD = ("cond opening new_black player empties k tag next elapsed cost move loss "
        "played_loss stop_reason extended after_decision decision").split()
NORMAL = ("n-gtp2", "n-fs6", "n-gtp20", "n-gtp60", "n-gtp20t4")
EXTEND = ("x-gtp2", "x-gtp20", "x-gtp60", "x-gtp20t4")


def reference(refer):
    ref = {}
    # The main run, then supplementary runs in subdirectories (moves the
    # main run lacked); a position's best move and value come from the
    # first run that searched it.
    paths = sorted(Path(refer).glob("out-*.tsv")) + sorted(Path(refer).glob("*/out-*.tsv"))
    for path in paths:
        for line in open(path):
            w = line.rstrip("\n").split("\t")
            moves = [m.split(":") for m in w[2].split()]
            values = {m: int(s) for m, s in moves}
            if w[0] in ref:
                ref[w[0]][1].update({m: v for m, v in values.items() if m not in ref[w[0]][1]})
            else:
                ref[w[0]] = (int(moves[0][1]), values)
    return ref


def table(run, refer):
    ref = reference(refer)
    print("\t".join(HEAD))
    missing = defaultdict(int)
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
                for its, turn in turns(out.parent / cond / name, int(a), int(b)):
                    if not turn["searched"] or turn["after_extension"]:
                        continue
                    if cond in EXTEND and not turn["extended"]:
                        continue
                    its = [i for i in its if i["best_move"]]
                    bw = pos[(turn["empties"], black)]
                    key = f"{bw[0]:016x}{bw[1]:016x}{'b' if black else 'w'}"
                    played = turn["played"]
                    if key not in ref:
                        missing["position"] += 1
                        continue
                    best, values = ref[key]
                    pl = best - values[played] if played in values else None
                    if pl is None:
                        missing["played"] += 1
                    if not its:
                        # No iteration finished (the first one ran into
                        # hard): one row for the move played.
                        row = [cond, opening, nb, player, turn["empties"], -1, "-", "-",
                               f"{turn['used']:.5f}", "-", played, "-" if pl is None else pl,
                               "-" if pl is None else pl, turn["stop_reason"],
                               int(turn["extended"]), 0, 0]
                        print("\t".join(map(str, row)))
                        continue
                    dec = None
                    if turn["extended"] and turn["decision_elapsed"] is not None:
                        done = [k for k, i in enumerate(its)
                                if i["elapsed"] <= turn["decision_elapsed"] + 1e-9]
                        dec = done[-1] if done else -1
                    for k, it in enumerate(its):
                        nxt = its[k + 1]["kind"] if k + 1 < len(its) else "-"
                        row = [cond, opening, nb, player, it["empties"], k, it["kind"], nxt,
                               f"{it['elapsed']:.5f}", f"{it['cost']:.5f}", it["best_move"],
                               best - values[it["best_move"]], "-" if pl is None else pl,
                               turn["stop_reason"], int(turn["extended"]),
                               int(it.get("after_decision", False)), int(k == dec)]
                        print("\t".join(map(str, row)))
    for k, v in missing.items():
        print(f"missing {k}: {v}", file=sys.stderr)


def band(e):
    return "<=20" if e <= 20 else "21-28" if e <= 28 else "29-40" if e <= 40 else ">40"


def transition(a, b):
    if b[0] == "d":
        return "mid"
    if a[0] == "d":
        return "entry"
    return "exact" if b == "e100" else "stage"


class Groups:
    """Values grouped by opening, for a bootstrap over openings."""

    def __init__(self):
        self.v = defaultdict(lambda: defaultdict(list))

    def add(self, key, opening, x):
        self.v[key][opening].append(x)

    def line(self, key, rng, scale=1.0):
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
        return (f"n={n}\t{mean(ops) * scale:.3f} "
                f"[{boots[50] * scale:.3f}, {boots[1949] * scale:.3f}]")


def summary(path):
    rows = defaultdict(list)
    head = None
    for line in open(path):
        w = line.rstrip("\n").split("\t")
        if head is None:
            head = w
            continue
        r = dict(zip(head, w))
        rows[(r["cond"], r["opening"], r["new_black"], r["player"], r["empties"])].append(r)
    rng = random.Random(20261124)
    played, game = Groups(), Groups()
    gain, changed = Groups(), Groups()
    ext, ext_changed, ext_after = Groups(), Groups(), Groups()
    neg = defaultdict(int)
    per_game = defaultdict(float)
    for (cond, opening, nb, player, e), its in rows.items():
        e = int(e)
        last = its[-1]
        if cond.startswith("n-") and last["played_loss"] != "-":
            pl = int(last["played_loss"])
            played.add((cond, band(e)), opening, pl)
            played.add((cond, "all"), opening, pl)
            per_game[(cond, opening, nb, player)] += pl
            if pl < 0:
                neg[(cond, band(e))] += 1
                neg[(cond, "all")] += 1
        for a, b in zip(its, its[1:]):
            if cond.startswith("x-") and b["after_decision"] != "1":
                continue
            t = transition(a["tag"], b["tag"])
            gain.add((cond, t), opening, int(a["loss"]) - int(b["loss"]))
            changed.add((cond, t), opening, int(a["move"] != b["move"]))
        if cond.startswith("x-"):
            dec = [r for r in its if r["decision"] == "1"]
            if dec:
                d = int(dec[0]["loss"])
                f = int(last["loss"])
                ext.add((cond, band(e)), opening, d - f)
                ext.add((cond, "all"), opening, d - f)
                ext_changed.add((cond, "all"), opening, int(dec[0]["move"] != last["move"]))
                ext_after.add((cond, "all"), opening,
                              sum(r["after_decision"] == "1" for r in its))
    for (cond, opening, nb, player), s in per_game.items():
        game.add(cond, opening, s)
    none_loss, none_time, all_time = Groups(), Groups(), defaultdict(float)
    for (cond, opening, nb, player, e), its in rows.items():
        if not cond.startswith("n-"):
            continue
        if its[0]["k"] == "-1":
            if its[0]["played_loss"] != "-":
                none_loss.add(cond, opening, int(its[0]["played_loss"]))
            none_time.add(cond, opening, float(its[0]["elapsed"]))


    print("# 1. loss of the played move (normal games; discs per turn)")
    for key in sorted(played.v):
        print(f"{key[0]}\t{key[1]}\t{played.line(key, rng)}\tnegative={neg.get(key, 0)}")
    print("# 1b. sum of the played losses per game and player")
    for key in sorted(game.v):
        print(f"{key}\t{game.line(key, rng)}")
    print("# 1c. turns without a finished iteration (first iteration cut at hard):"
          " loss of the move played, seconds per turn")
    for key in sorted(none_loss.v):
        print(f"{key}\tloss {none_loss.line(key, rng)}\ttime {none_time.line(key, rng)}")
    print("# 2. gain of the next finished iteration, loss(k) - loss(k+1) (discs)"
          " and share where the move changed; x-*: iterations after the decision")
    for key in sorted(gain.v):
        print(f"{key[0]}\t{key[1]}\t{gain.line(key, rng)}\tchanged {changed.line(key, rng)}")
    print("# 3. extended turns: loss at the decision - loss at the end (discs),"
          " share where the move changed, iterations after the decision")
    for key in sorted(ext.v):
        print(f"{key[0]}\t{key[1]}\t{ext.line(key, rng)}")
    for key in sorted(ext_changed.v):
        print(f"{key[0]}\tchanged\t{ext_changed.line(key, rng)}"
              f"\tafter\t{ext_after.line(key, rng)}")


def main():
    if sys.argv[1] == "table":
        table(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "summary":
        summary(sys.argv[2])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
