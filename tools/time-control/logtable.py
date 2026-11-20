#!/usr/bin/env python3
"""The iterations of DEFT_GTP_LOG files as a features.py table.

usage: logtable.py LOG... > table.tsv

Games are cut where the empties count rises. Board columns, score and move
are not in the logs and are written as 0 / "-" (use cost_fit.py
--no-board). For the last iteration of a move:
  - the search ran into hard (the pending iteration was running): the row
    is censored, with the time it ran as a lower bound;
  - the forecast stopped before the pending iteration: it never started,
    so there is nothing to learn and the row is left out.
Rows from a forecast stop are thus missing by a decision on the features,
not on the outcome, which does not bias a fit conditional on them.
"""
import sys

HEAD = ("game empties side_mob opp_mob odd_regions regions corners_empty disc_diff "
        "tag t cost score move prev_tag prev_cost next_tag next_cost censored").split()


def main():
    print("\t".join(HEAD))
    game = 0
    for f, path in enumerate(sys.argv[1:]):
        prev_e = 99
        for line in open(path):
            w = line.split()
            if "empties" not in w or "steps" not in w:
                continue
            i = w.index("empties")
            e = int(w[i + 1])
            if e > prev_e:
                game += 1
            prev_e = e
            hard, used, done = float(w[i + 3]), float(w[i + 5]), float(w[i + 7])
            pending = w[w.index("next") + 1]
            steps = [(s.split(":")[0], float(s.split(":")[1])) for s in w[w.index("steps") + 1:]]
            hit_hard = used - done > 0.002 and used >= 0.9 * hard
            prev_t = 0.0
            for k, (tag, t) in enumerate(steps):
                cost = t - prev_t
                prev = steps[k - 1] if k else None
                prev_cost = prev[1] - (steps[k - 2][1] if k > 1 else 0.0) if prev else None
                if k + 1 < len(steps):
                    nxt, ncost, cens = steps[k + 1][0], steps[k + 1][1] - t, 0
                elif pending != "-" and hit_hard:
                    nxt, ncost, cens = pending, used - t, 1
                else:
                    prev_t = t
                    continue
                row = [game, e, 0, 0, 0, 0, 0, 0, tag, f"{t:.4f}", f"{cost:.4f}", 0, "-",
                       prev[0] if prev else "-", f"{prev_cost:.4f}" if prev else "",
                       nxt, f"{ncost:.4f}", cens]
                print("\t".join(map(str, row)))
                prev_t = t
        game += 1


if __name__ == "__main__":
    main()
