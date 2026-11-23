#!/usr/bin/env python3
"""The iterations of referee clock logs (CLOCK_LOG_DIR) as a cost table.

usage: journal_table.py OUT.tsv... > table.tsv

Reads each OUT.logs.tsv next to the given OUT.tsv and the byte ranges it
names. The first columns are those of features.py / logtable.py, so
cost_fit.py reads the table (use --no-board: corners_empty and disc_diff
are not logged and are 0). Extra columns follow:

  cond        the OUT.tsv name without .tsv
  side        new | old
  protocol, threads, remaining, soft, hard   from the turn record
  nodes, next_nodes
  reply_matched   1 | 0 | - (unknown)
  extended    1 on the extended turn of an extension game
  after_decision  1 if the next iteration ended after the clock had
              decided to stop (only extension turns have such rows)
  prev_turn_used  seconds the side's previous turn took
  opening     the referee's opening id (colour-swapped games of one opening
              nearly repeat each other in self-play: resample by opening)

The next iteration of the last one of a turn:
  - not planned (next is null): no row;
  - the policy stopped the search (stop true, not extended): it never ran
    and the row is left out, as logtable.py does;
  - otherwise the search ran into its limit (hard, or the extension's end)
    while it ran: the row is censored at used - elapsed.
Turns after an extension (after_extension) are skipped.
"""
import json, sys
from pathlib import Path

HEAD = ("game empties side_mob opp_mob odd_regions regions corners_empty disc_diff "
        "tag t cost score move prev_tag prev_cost next_tag next_cost censored "
        "cond side protocol threads remaining soft hard nodes next_nodes "
        "reply_matched extended after_decision prev_turn_used opening").split()


def turns(path, start, end):
    """(iterations, turn record) of each turn in the byte range."""
    with open(path, "rb") as f:
        f.seek(start)
        data = f.read(end - start)
    its = []
    for line in data.splitlines():
        r = json.loads(line)
        if r["record"] == "iteration":
            its.append(r)
        else:
            yield its, r
            its = []


def fmt(x, spec=".4f"):
    if x is None:
        return "-"
    if isinstance(x, bool):
        return str(int(x))
    return format(x, spec) if isinstance(x, float) else str(x)


def main():
    print("\t".join(HEAD))
    game = 0
    for out in sys.argv[1:]:
        out = Path(out)
        cond = out.name.removesuffix(".tsv")
        spans = out.with_suffix(".logs.tsv")
        logdir = out.parent / cond
        for line in spans.read_text().splitlines()[1:]:
            w = line.split("\t")
            for side, (name, a, b) in (("new", w[3:6]), ("old", w[6:9])):
                game += 1
                for its, turn in turns(logdir / name, int(a), int(b)):
                    if turn["after_extension"] or not turn["searched"]:
                        continue
                    ext = turn["extended"]
                    for k, it in enumerate(its):
                        prev = its[k - 1] if k else None
                        if k + 1 < len(its):
                            nxt = its[k + 1]
                            ntag, ncost, cens = nxt["kind"], nxt["cost"], 0
                            nnodes, after = nxt["nodes"], nxt.get("after_decision", False)
                        elif it["next"] is None or (it["stop"] and not ext):
                            continue
                        else:
                            ntag, ncost, cens = it["next"], turn["used"] - it["elapsed"], 1
                            nnodes, after = None, ext
                            if ncost <= 0:
                                continue
                        row = [game, it["empties"], turn["mobility"], turn["opponent_mobility"],
                               turn["odd_regions"], turn["regions"], 0, 0,
                               it["kind"], f"{it['elapsed']:.5f}", f"{it['cost']:.5f}",
                               it["score"], it["best_move"] or "-",
                               prev["kind"] if prev else "-",
                               f"{prev['cost']:.5f}" if prev else "",
                               ntag, f"{ncost:.5f}", cens,
                               cond, side, turn["protocol"], turn["threads"],
                               fmt(turn["remaining"]), fmt(turn["soft"]), fmt(turn["hard"]),
                               it["nodes"], fmt(nnodes), fmt(turn["reply_matched"]),
                               int(ext), int(after), fmt(turn["previous_used"]), w[0]]
                        print("\t".join(map(str, row)))


if __name__ == "__main__":
    main()
