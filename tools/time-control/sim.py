#!/usr/bin/env python3
"""Whole-game simulation of the game clock on logged games (stage 4-1).

usage: sim.py check RUN_DIR REFER_DIR
       sim.py run RUN_DIR REFER_DIR [WORLDS] [HEAVY]
       sim.py first RUN_DIR REFER_DIR [WORLDS] [HEAVY]
       sim.py predict RUN_DIR REFER_DIR

Each side of each normal game of openings 200..299 (stage 3-1, `n-*`) is
replayed turn by turn on its own clock: the moves of the game are fixed,
the clock decides at every iteration whether to start the next one, and
the time it uses carries over to the later turns.

The "world" of a turn is the ladder of iterations the search would run up
to the exact solve, each with its time and the loss of its move (from the
stage 3-2 reference, see refer_loss.py). The logged iterations are taken
as they are. A turn whose first iteration (warm-started from the table at
a deep one) ran into hard has no finished iteration; its time beyond hard
is unknown, and the simulation assumes it runs into hard whatever hard is,
playing the logged move. Beyond them (the clock stopped, or the hard limit cut an
iteration) the ladder is completed by drawing:

  - the kind of the next iteration from the plan seen in the logs (it is
    a fixed function of the empties and the kind);
  - its time as the last time times a ratio drawn from the finished
    iterations of the same transition, threads and empties (in fours);
    a cut iteration draws only ratios above its censoring point. HEAVY
    (default 1) multiplies drawn ENTRY ratios, to test how much the
    answer depends on the light ENTRYs the logs over-represent;
  - its loss from the logged pairs (loss before, loss after) of the same
    transition: a move with no loss keeps the after-loss of a drawn pair
    that also had none, a lossy one is scaled by a drawn after/before;
    the exact solve with at most 28 empties has no loss.

`check` replays the logs with the legacy clock and reports how many
turns end where the log ended (they must all, the clock being
deterministic), and the summed loss against the logged one.

`run` simulates the variants below on WORLDS completed worlds (default 4)
and prints per variant the mean summed loss per game and side, the time
per game, time losses, and the difference to `legacy` with a 95% interval
from a bootstrap over openings.

Variants: `legacy` (the clock of game_clock/legacy.rs), and the same with
the predicted time of some transitions replaced by the world's true time
times exp(sigma * z): mid, entry, stage+exact, all; sigma 0 (truth),
0.25, 0.5, 1.0. The noise z is fixed per iteration across variants.
`nowaste`: legacy, but a turn whose first (warm-started) iteration ran
into hard stops at once and plays the table's move -- the move a cut first
iteration returns anyway (search.rs) -- so its time goes to later turns.
`first` runs legacy, nowaste and the policies for the warm-started first
iteration (see play_turn): `firstsoft K` cuts it at K times soft, `skip T`
plays the table's move at once when the previous turn predicted its next
iteration at more than T times this turn's hard. The table's move is the
previous PV's when known, else its loss is drawn as SIM_TABLE says
(complete()).
`predict` reports, for the warm-started turns, how well the previous
turn's prediction of its next iteration (over this turn's hard) separates
the turns whose first iteration runs into hard (AUC, and the share of such
turns by bins), and on the turns where the previous PV shows the table's
move, its loss minus the loss of the first iteration's move.
Each variant is also run at equal time (`=t`): its soft target scaled so
that its mean time per game matches legacy's (a better forecast also
changes how much the stop rule spends, which would confound the loss).
"""
import json, math, os, random, sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from refer_input import positions, turns  # noqa: E402
from refer_loss import reference  # noqa: E402

NORMAL = ("n-gtp2", "n-fs6", "n-gtp20", "n-gtp60", "n-gtp20t4")
# Midgame loss pairs above 28 empties are drawn only from iterations up to
# this depth (environment SIM_MID_POOL_DEPTH, default no limit): the
# reference there is a depth-26 search, so deeper iterations are measured
# against a shallower one.
MID_POOL_DEPTH = int(os.environ.get("SIM_MID_POOL_DEPTH", "99"))
# SIM_DEPTH_BUCKETS=0 pools the midgame loss pairs over all depths (the
# first version; it overstates how often a deep step spoils a good move).
DEPTH_BUCKETS = os.environ.get("SIM_DEPTH_BUCKETS", "1") != "0"
# The loss of the table's move of a warm-started turn, when the previous
# PV does not show it (SIM_TABLE): `known` draws it from the warm turns
# where the PV shows it, as a pair (loss of the first iteration's move, loss
# of the table's move) of the same depth and lateness (first iteration
# finished after soft or not); `back` from the logged steps of the same
# transition read backwards (the loss before a step given the loss after
# it), which on the known turns gives the table's move 0.01-0.04 discs per
# turn more loss than it has (sim.py predict).
TABLE = os.environ.get("SIM_TABLE", "known")


TRANSITIONS = ("mid", "entry", "stage", "exact", "back")


def transition(a, b):
    if b[0] == "d":
        return "mid" if a[0] == "d" else "back"
    if a[0] == "d":
        return "entry"
    return "exact" if b == "e100" else "stage"


class Turn:
    __slots__ = ("empties", "searched", "rem", "cap", "permove", "used", "soft", "hard",
                 "overhead", "tail", "its", "end", "cut_at", "cut_latency", "logged_loss",
                 "first_next", "world", "nomove", "warm", "first_pred", "table_loss",
                 "first_tr", "table_imputed")


def fresh_start(kind):
    """A first iteration that starts the ladder from the bottom (no table
    to warm-start from)."""
    return kind[0] == "d" and int(kind[1:]) <= 6


def load(run, refer):
    """{cond: [(opening, nb, player, threads, [Turn])]}, plan, pools."""
    ref = reference(refer)
    games = defaultdict(list)
    plan = {}
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
                ts = []
                threads = 1
                prev = None  # last finished iteration and reached kind of the previous searched turn
                for its, t in turns(out.parent / cond / name, int(a), int(b)):
                    for i in its:
                        if i["next"] is not None:
                            plan[(i["empties"], i["kind"])] = i["next"]
                    u = Turn()
                    u.empties, u.searched = t["empties"], t["searched"]
                    u.rem, u.cap, u.used = t["remaining"], t["cap"], t["used"]
                    u.soft, u.hard = t["soft"], t["hard"]
                    u.permove = (u.cap if u.cap is not None and u.rem is not None
                                 and u.cap < u.rem - 1e-6 else None)
                    threads = t["threads"]
                    u.its, u.end, u.cut_at, u.cut_latency = [], None, None, None
                    u.logged_loss = None
                    u.nomove = False
                    u.warm, u.first_pred, u.table_loss, u.first_tr = False, None, None, None
                    u.table_imputed = False
                    if u.searched:
                        bw = pos[(u.empties, black)]
                        key = f"{bw[0]:016x}{bw[1]:016x}{'b' if black else 'w'}"
                        best, values = ref[key]
                        fin = [i for i in its if i["best_move"]]
                        # Warm start: the first iteration starts deep, from
                        # the table of the previous turn. If it is cut, the
                        # engine plays the table's move: the third move of
                        # the previous turn's PV when the opponent replied
                        # as it expected (the cut turns that show it played
                        # it in 747 of 750), else unknown (imputed in
                        # complete()). `first_pred` is the previous turn's
                        # prediction of its next iteration (taken from the
                        # log, not from the simulated previous turn).
                        u.warm = not its or not fresh_start(its[0]["kind"])
                        if prev is not None:
                            u.first_pred = prev[0]["predicted"]
                            pv = prev[0]["pv"]
                            if t["reply_matched"] and len(pv) >= 3 and pv[2] in values:
                                u.table_loss = best - values[pv[2]]
                            if its:
                                u.first_tr = transition(prev[1], its[0]["kind"])
                        prev = (fin[-1], t["reached"]) if fin else None
                        if not fin:
                            # The first iteration (warm-started from the
                            # table at a deep one) ran into hard. Its time
                            # is unknown beyond that; the simulation assumes
                            # it runs into any hard, and the move played is
                            # the one logged.
                            u.nomove = True
                            u.logged_loss = best - values[t["played"]]
                            u.table_loss = u.logged_loss
                            u.cut_latency = max(0.0, u.used - u.hard)
                            u.world = []
                            ts.append(u)
                            continue
                        loss = None
                        for i in its:
                            if i["best_move"]:
                                loss = best - values[i["best_move"]]
                            u.its.append([i["kind"], i["cost"], loss, i["best_move"]])
                        u.logged_loss = best - values[t["played"]]
                        last = its[-1]
                        u.tail = max(0.0, u.used - last["elapsed"])
                        if last["next"] is None:
                            u.end = "done"
                        elif last["stop"]:
                            u.end = "stop"
                        else:
                            u.end = "cut"
                            u.cut_at = u.used - last["elapsed"]
                            u.cut_latency = max(0.0, u.used - u.hard)
                            u.tail = 0.0
                        u.first_next = last["next"]
                    ts.append(u)
                # Turns without a clock (FlipSight's forced moves) carry on
                # the clock of the turn before.
                for x, y in zip(ts, ts[1:]):
                    if y.rem is None:
                        y.rem = x.rem - x.used
                    x.overhead = x.rem - y.rem - x.used
                ts[-1].overhead = 0.0
                games[cond].append((opening, nb, player, threads, ts))
    return games, plan


def pools(games, table=None):
    """Ratio and loss pools from the logged iterations; the pool for the
    table's move is built as `table` (default SIM_TABLE) says."""
    table = table or TABLE
    ratio = defaultdict(list)
    loss = defaultdict(list)
    for cond, gs in games.items():
        for _, _, _, threads, ts in gs:
            for u in ts:
                for (ka, ca, la, _), (kb, cb, lb, _) in zip(u.its, u.its[1:]):
                    tr = transition(ka, kb)
                    if ca > 0.002:
                        ratio[(threads, tr, u.empties // 4)].append(cb / ca)
                    if la is not None and lb is not None and not (
                            tr == "mid" and u.empties > 28 and int(kb[1:]) > MID_POOL_DEPTH):
                        loss[(tr, u.empties <= 28, depth_bucket(kb))].append((la, lb))
    split = {k: ([b for a, b in v if a == 0] or [0], [(a, b) for a, b in v if a > 0] or [(1, 1)])
             for k, v in loss.items()}
    # The same pairs read backwards, for the loss before a step given the
    # loss after it (the table's move of a warm start whose first iteration
    # finished): (befores of no-loss afters, lossy pairs as (after, before)).
    back = {k: ([a for a, b in v if b == 0] or [0], [(b, a) for a, b in v if b > 0] or [(1, 1)])
            for k, v in loss.items()}
    if table == "known":
        known = defaultdict(list)
        for cond, gs in games.items():
            for _, _, _, _, ts in gs:
                for u in ts:
                    if (u.searched and u.warm and not u.nomove and not u.table_imputed
                            and u.table_loss is not None and u.its[0][2] is not None):
                        known[table_key(u)].append((u.its[0][2], u.table_loss))
        back = {k: ([t for f, t in v if f == 0] or [0], [(f, t) for f, t in v if f > 0] or [(1, 1)])
                for k, v in known.items()}
    return ratio, (split, back)


def next_kind(plan, empties, kind):
    if (empties, kind) in plan:
        return plan[(empties, kind)]
    if kind == "e100":
        return None
    if kind[0] == "e":
        order = ["e85", "e95", "e98", "e99", "e100"]
        return order[order.index(kind) + 1]
    # A midgame depth the logs never continued: two plies deeper, or the
    # first endgame stage where the logs show the last depth.
    d = int(kind[1:])
    lasts = [int(k[1:]) for (e, k), n in plan.items() if e == empties and k[0] == "d" and n[0] == "e"]
    if lasts and d >= max(lasts):
        return "e85"
    return f"d{d + 2}" if d + 2 < empties else "e85"


def draw_ratio(rng, ratio, threads, tr, empties, above=0.0, heavy=1.0):
    for eb in (empties // 4, empties // 4 - 1, empties // 4 + 1):
        p = ratio.get((threads, tr, eb))
        if p:
            break
    else:
        p = [3.0]
    r = rng.choice(p) * (heavy if tr == "entry" else 1.0)
    if r > above:
        return r
    q = [x for x in p if x * (heavy if tr == "entry" else 1.0) > above]
    if q:
        return rng.choice(q) * (heavy if tr == "entry" else 1.0)
    return above * math.exp(rng.expovariate(2.0))


def table_key(u):
    return (u.its[0][1] > u.soft, u.empties <= 28, depth_bucket(u.its[0][0]))


def draw_table(rng, pool, u):
    late, e, b = table_key(u)
    for bb in range(b, -2, -1) if b >= 0 else (-1,):
        p = pool.get((late, e, bb))
        if p:
            break
    zero, lossy = p or ([0], [(1, 1)])
    cur = u.its[0][2]
    if cur is None or cur == 0:
        return rng.choice(zero)
    f, t = rng.choice(lossy)
    return round(cur * t / f)


def depth_bucket(kind):
    """Midgame steps change their move less often the deeper they are, so
    their loss pairs are kept by the depth reached (in fours); endgame
    stages in one bucket."""
    if kind[0] != "d" or not DEPTH_BUCKETS:
        return -1
    return min(int(kind[1:]) // 4, 6)


def draw_loss(rng, loss, tr, empties, kind, cur, exact_free=True):
    if exact_free and kind == "e100" and empties <= 28:
        return 0
    b = depth_bucket(kind)
    # The deepest bucket with data at or below this depth.
    for bb in range(b, -2, -1) if b >= 0 else (-1,):
        pool = loss.get((tr, empties <= 28, bb))
        if pool:
            break
    zero, lossy = pool or ([0], [(1, 1)])
    if cur is None or cur == 0:
        return rng.choice(zero)
    a, b = rng.choice(lossy)
    return round(cur * b / a)


def complete(games, plan, ratio, losses, seed, heavy=1.0):
    """Give each searched turn a world: the logged ladder carried on to the
    exact solve."""
    loss, back = losses
    for cond, gs in games.items():
        for gi, (opening, nb, player, threads, ts) in enumerate(gs):
            for ti, u in enumerate(ts):
                if not u.searched or u.nomove:
                    continue
                if u.warm and (u.table_loss is None or u.table_imputed):
                    # The table's move unknown: the loss before the first
                    # iteration's step, drawn backwards from its loss
                    # (own generator, so the worlds stay as before).
                    trng = random.Random(f"{seed}/{cond}/{opening}/{nb}/{player}/{ti}/table")
                    u.table_imputed = True
                    if TABLE == "known":
                        u.table_loss = draw_table(trng, back, u)
                    else:
                        u.table_loss = draw_loss(trng, back, u.first_tr or "mid", u.empties,
                                                 u.its[0][0], u.its[0][2], exact_free=False)
                rng = random.Random(f"{seed}/{cond}/{opening}/{nb}/{player}/{ti}")
                w = [list(i) for i in u.its]
                if u.end != "done":
                    kind = u.first_next
                    above = u.cut_at / w[-1][1] if u.end == "cut" and w[-1][1] > 0 else 0.0
                    fresh = 0
                    while kind is not None:
                        k0, c0, l0, m0 = w[-1]
                        tr = transition(k0, kind)
                        r = draw_ratio(rng, ratio, threads, tr, u.empties, above, heavy)
                        above = 0.0
                        l1 = draw_loss(rng, loss, tr, u.empties, kind, l0)
                        if l1 != l0:
                            fresh += 1
                            m0 = f"?{fresh}"
                        w.append([kind, max(c0, 1e-4) * r, l1, m0])
                        kind = next_kind(plan, u.empties, kind)
                u.world = w


# ---- the legacy clock (game_clock/legacy.rs) ----

def budget(empties, remaining, cap):
    turns = max(1, (empties + 1) // 2)
    pool, left = remaining, turns
    for distance in range(turns - 1, 0, -1):
        future = empties - 2 * distance
        solve = 0.002 + 3.0 * 0.006 * 2.0 ** (future - 17)
        if solve >= pool / left:
            break
        pool -= solve
        left -= 1
    share = pool / left
    hard = min(share + min(share, (pool - share) / 2), cap)
    return min(share, hard), hard


def ratio_at(base, pivot, per, floor, e):
    return max(base * per ** (e - pivot), floor)


def growth(empties, kind, nxt, cost, last_cost, last_depth):
    if kind[0] == "d" and nxt[0] == "d":
        d, deeper = int(kind[1:]), int(nxt[1:])
        if last_depth is not None and last_cost > 0 and d > last_depth:
            per2 = min(max((cost / last_cost) ** (2.0 / (d - last_depth)), 1.5), 6.0)
        else:
            per2 = 3.0
        return per2 ** (max(deeper - d, 0) / 2.0)
    entry = ratio_at(5.0, 24.0, 1.06, 2.0, empties)
    exact = ratio_at(3.0, 22.0, 1.15, 1.0, empties)
    if kind[0] == "d":
        return entry * exact if nxt == "e100" else entry
    if nxt == "e100":
        return exact
    if nxt[0] == "e":
        return 1.25
    return entry


def policy(truth, name):
    for t in truth:
        if t.startswith(name + ":"):
            return float(t.split(":")[1])
    return None


def play_turn(u, soft, hard, truth, sigma, noise_key):
    """(used, loss, cut) of one turn under the legacy stop rule; `truth` is
    the set of transitions whose prediction is the world's time times
    exp(sigma * z)."""
    if u.warm:
        # Policies for the warm-started first iteration (stage 4-1, A):
        # skip:T plays the table's move at once when the previous turn's
        # prediction of its next iteration exceeds T times hard;
        # firstsoft:K cuts the first iteration at K times soft (at most
        # hard) and plays the table's move.
        th = policy(truth, "skip")
        if th is not None and u.first_pred is not None and u.first_pred > th * hard:
            return 0.002, u.table_loss, False
        k = policy(truth, "firstsoft")
        if k is not None:
            limit = min(hard, k * soft)
            if u.nomove or u.world[0][1] > limit:
                return limit + 0.002, u.table_loss, True
    if u.nomove:
        if "nowaste" in truth:
            # Stop at once and play the table's move, which a cut first
            # iteration plays anyway.
            return 0.002, u.logged_loss, False
        return hard + u.cut_latency, u.logged_loss, True
    elapsed = 0.0
    last_cost, last_depth = 0.0, None
    best_loss, inst, best = None, 0.0, None
    w = u.world
    rng = random.Random(noise_key) if any(t in TRANSITIONS for t in truth) else None
    for k, (kind, cost, loss, move) in enumerate(w):
        if elapsed + cost > hard:
            return hard + (u.cut_latency or 0.002), (best_loss if best_loss is not None else loss), True
        elapsed += cost
        changed = best is not None and best != move
        inst = 1.0 if changed else inst * 0.5
        best = move
        if loss is not None:
            best_loss = loss
        if k + 1 >= len(w):
            break
        nxt, ncost = w[k + 1][0], w[k + 1][1]
        tr = transition(kind, nxt)
        z = rng.gauss(0, 1) if rng else 0.0
        if tr in truth:
            pred = ncost * math.exp(sigma * z)
        else:
            pred = cost * growth(u.empties, kind, nxt, cost, last_cost, last_depth)
        last_cost = cost
        last_depth = int(kind[1:]) if kind[0] == "d" else None
        if elapsed + pred > min(soft * (1 + inst), hard):
            return elapsed + u.tail, best_loss, False
    return elapsed + u.tail, best_loss, False


BANDS = ("<=20", "21-28", "29-40", ">40")


def band(e):
    return 0 if e <= 20 else 1 if e <= 28 else 2 if e <= 40 else 3


def play_game(ts, truth=frozenset(), sigma=0.0, key="", check=False, margin=0.010, scale=1.0):
    """(loss, time, time losses, mismatches, cuts, loss by band) of one side.
    A time loss is a turn that takes more than its cap plus the protocol's
    reply margin (the clock's `remaining` is net of the margin)."""
    rem = ts[0].rem
    total_loss = 0
    used_total = 0.0
    flags = mism = cuts = 0
    bands = [0, 0, 0, 0]
    for ti, u in enumerate(ts):
        cap = rem
        if not u.searched:
            used = u.used
        else:
            cap = min(u.permove, rem) if u.permove is not None else rem
            soft, hard = budget(u.empties, rem, cap)
            soft = min(soft * scale, hard)
            used, loss, cut = play_turn(u, soft, hard, truth, sigma, f"{key}/{ti}")
            total_loss += loss
            bands[band(u.empties)] += loss
            cuts += cut
            if check:
                mism += int(abs(used - u.used) > 1e-6 or loss != u.logged_loss)
        if used > cap + margin:
            flags += 1
        rem -= used + u.overhead
        used_total += used
    return total_loss, used_total, flags, mism, cuts, bands


def boot(diffs, rng, n=2000):
    """Mean and 95% interval of {opening: [values]} over openings."""
    ops = sorted(diffs)

    def mean(sample):
        t = c = 0
        for o in sample:
            t += sum(diffs[o])
            c += len(diffs[o])
        return t / c
    b = sorted(mean([rng.choice(ops) for _ in ops]) for _ in range(n))
    return mean(ops), b[n // 40], b[n - 1 - n // 40]


def predict(games):
    import bisect
    bins = (-9, -2, -1.5, -1, -0.5, 0, 0.5, 9)
    print("# warm-started turns: x = ln(previous turn's predicted next iteration / hard);"
          " AUC of x for the first iteration running into hard; share of such turns by x")
    for cond, gs in games.items():
        xs = [(math.log(u.first_pred / u.hard), u.nomove) for *_, ts in gs for u in ts
              if u.searched and u.warm and u.first_pred and u.hard]
        neg = sorted(x for x, c in xs if not c)
        pos = [x for x, c in xs if c]
        auc = sum(bisect.bisect_left(neg, x) + 0.5 * (bisect.bisect_right(neg, x)
                  - bisect.bisect_left(neg, x)) for x in pos) / len(pos) / len(neg)
        cells = []
        for lo, hi in zip(bins, bins[1:]):
            b = [c for x, c in xs if lo <= x < hi]
            cells.append(f"[{lo:g},{hi:g}) {sum(b)}/{len(b)}")
        print(f"{cond}\tturns {len(xs)}\tcut {len(pos)}\tAUC {auc:.3f}\t" + "\t".join(cells))
    print("# warm turns whose table's move the previous PV shows: loss of the table's move"
          " minus loss of the first iteration's move (discs per turn), share where they differ;"
          " late = the first iteration finished after soft")
    for cond, gs in games.items():
        for late in (False, True):
            d = [u.table_loss - (u.its[0][2] or 0) for *_, ts in gs for u in ts
                 if u.searched and u.warm and not u.nomove and u.table_loss is not None
                 and not u.table_imputed and (u.its[0][1] > u.soft) == late]
            per_side = len(d) / len(gs)
            print(f"{cond}\t{'late' if late else 'early'}\tn {len(d)} ({per_side:.2f} per side)"
                  f"\tmean {sum(d) / len(d):+.3f}\tdiffer {sum(x != 0 for x in d) / len(d):.3f}")
        late_all = sum(1 for *_, ts in gs for u in ts if u.searched and u.warm and not u.nomove
                       and u.its[0][1] > u.soft) / len(gs)
        unknown = sum(1 for *_, ts in gs for u in ts if u.searched and u.warm and not u.nomove
                      and u.table_loss is None) / len(gs)
        print(f"{cond}\tall late turns per side {late_all:.2f}\twarm turns with the table's move"
              f" unknown per side {unknown:.2f}")
    print("# warm turns whose first iteration finished: ln(its time / the previous turn's"
          " prediction), quartiles")
    for cond, gs in games.items():
        r = sorted(math.log(u.its[0][1] / u.first_pred) for *_, ts in gs for u in ts
                   if u.searched and u.warm and not u.nomove and u.first_pred and u.its[0][1] > 0)
        print(f"{cond}\tn {len(r)}\t" + " ".join(f"{r[int(q * (len(r) - 1))]:+.2f}"
                                               for q in (0.25, 0.5, 0.75)))
    print("# the two imputations of the table's move, tried on the turns where it is known:"
          " mean (table's move - first iteration's move) loss, known and drawn")
    _, (_, known) = pools(games, "known")
    _, (_, back) = pools(games, "back")
    for cond, gs in games.items():
        k, a, b = [], [], []
        for o, nb, pl, _, ts in gs:
            for ti, u in enumerate(ts):
                if (u.searched and u.warm and not u.nomove and u.table_loss is not None
                        and not u.table_imputed):
                    f = u.its[0][2] or 0
                    rng = random.Random(f"predict/{cond}/{o}/{nb}/{pl}/{ti}")
                    k.append(u.table_loss - f)
                    a.append(draw_table(rng, known, u) - f)
                    b.append(draw_loss(rng, back, u.first_tr or "mid", u.empties, u.its[0][0],
                                       u.its[0][2], exact_free=False) - f)
        print(f"{cond}\tknown {sum(k) / len(k):+.3f}\tdrawn known {sum(a) / len(a):+.3f}"
              f"\tdrawn back {sum(b) / len(b):+.3f}")


def main():
    mode, run, refer = sys.argv[1:4]
    games, plan = load(run, refer)
    ratio, loss = pools(games)
    if mode == "predict":
        predict(games)
        return
    if mode == "check":
        complete(games, plan, ratio, loss, 0)
        for cond, gs in games.items():
            turns_n = mism = 0
            sim = logged = 0
            for _, _, _, _, ts in gs:
                l, _, _, m, _, _ = play_game(ts, check=True)
                sim += l
                logged += sum(u.logged_loss for u in ts if u.searched)
                mism += m
                turns_n += sum(u.searched for u in ts)
            print(f"{cond}\tturns {turns_n}\tdiffer {mism}\tloss sim {sim} logged {logged}")
        return
    worlds = int(sys.argv[4]) if len(sys.argv) > 4 else 4
    heavy = float(sys.argv[5]) if len(sys.argv) > 5 else 1.0
    variants = [("legacy", frozenset(), 0.0)]
    if mode == "run":
        for name, tr in (("mid", {"mid"}), ("entry", {"entry"}), ("stage+exact", {"stage", "exact"}),
                         ("all", {"mid", "entry", "stage", "exact", "back"})):
            for sigma in (0.0, 0.25, 0.5, 1.0):
                variants.append((f"{name} s={sigma}", frozenset(tr), sigma))
    variants.append(("nowaste", frozenset({"nowaste"}), 0.0))
    if mode == "first":
        for k in (1.0, 1.5):
            variants.append((f"firstsoft {k}", frozenset({f"firstsoft:{k}"}), 0.0))
        for th in (2.0, 3.0, 5.0, 8.0):
            variants.append((f"skip {th}", frozenset({f"skip:{th}"}), 0.0))
        variants.append(("skip 3+firstsoft 1", frozenset({"skip:3.0", "firstsoft:1.0"}), 0.0))
    res = defaultdict(lambda: defaultdict(lambda: defaultdict(list)))  # cond, variant, opening
    # A variant that predicts better also changes how much time the stop
    # rule spends. To compare at equal time, each variant's soft target is
    # scaled (hard unchanged) so that its mean time per game in world 0
    # matches legacy's within 0.3%. The scale is set from time alone, not
    # from the losses.
    complete(games, plan, ratio, loss, 0, heavy)
    scales = {}
    for cond, gs in games.items():
        margin = 0.050 if "fs" in cond else 0.010

        def mean_time(tr, sigma, scale):
            return sum(play_game(ts, tr, sigma, f"0/{cond}/{o}/{nb}/{pl}", margin=margin,
                                 scale=scale)[1] for o, nb, pl, _, ts in gs) / len(gs)
        target = mean_time(frozenset(), 0.0, 1.0)
        for name, tr, sigma in variants:
            lo, hi = 0.3, 3.0
            scale = 1.0
            for _ in range(20):
                t = mean_time(tr, sigma, scale)
                if abs(t / target - 1) < 0.003:
                    break
                if t < target:
                    lo = scale
                else:
                    hi = scale
                scale = (lo + hi) / 2
            scales[(cond, name)] = scale
    for wi in range(worlds):
        complete(games, plan, ratio, loss, wi, heavy)
        for cond, gs in games.items():
            for opening, nb, player, _, ts in gs:
                key = f"{wi}/{cond}/{opening}/{nb}/{player}"
                for name, tr, sigma in variants:
                    for eq in (False, True):
                        res[cond][name + (" =t" if eq else "")][opening].append(
                            play_game(ts, tr, sigma, key, margin=0.050 if "fs" in cond else 0.010,
                                      scale=scales[(cond, name)] if eq else 1.0))
    rng = random.Random(20261124)
    print(f"# worlds {worlds}, heavy {heavy}; per game and side: loss (discs), time (s), "
          "time losses; difference to legacy [95%, bootstrap over openings]")
    for cond in NORMAL:
        base = res[cond]["legacy"]
        for name in [n + e for n, _, _ in variants for e in ("", " =t")]:
            if name == "legacy =t":
                continue
            r = res[cond][name]
            ls = {o: [x[0] for x in v] for o, v in r.items()}
            ts_ = {o: [x[1] for x in v] for o, v in r.items()}
            flags = sum(x[2] for v in r.values() for x in v)
            d = {o: [a[0] - b[0] for a, b in zip(r[o], base[o])] for o in r}
            m, lo, hi = boot(d, rng)
            ml = sum(sum(v) for v in ls.values()) / sum(len(v) for v in ls.values())
            mt = sum(sum(v) for v in ts_.values()) / sum(len(v) for v in ts_.values())
            cuts = sum(x[4] for v in r.values() for x in v) / sum(len(v) for v in r.values())
            nb = sum(len(v) for v in r.values())
            bl = " ".join(f"{sum(x[5][i] for v in r.values() for x in v) / nb:.2f}"
                          for i in range(4))
            sc = scales.get((cond, name.removesuffix(" =t")), 1.0) if name.endswith("=t") else 1.0
            print(f"{cond}\t{name}\tscale {sc:.3f}\tloss {ml:.3f}\ttime {mt:.2f}\tflags {flags}\tcuts {cuts:.2f}"
                  f"\tbands {bl}\tdiff {m:+.3f} [{lo:+.3f}, {hi:+.3f}]")


if __name__ == "__main__":
    main()
