#!/usr/bin/env python3
"""Matches between Deft and Edax 4.5.5 or Egaroucid 7.8.1 over GTP.

usage: match.py OPPONENT MODE OPENINGS OUT.tsv [WORKERS]
  OPPONENT  edax | egaroucid | deft (Deft with the model OPP_MODEL)
  MODE      levelN (every engine at level N) | levelD:E (Deft at level D, the
            opponent at level E) | timeS (S seconds per side per game)
            | timeS:D (the opponent S seconds, Deft D seconds, to even the time used)
  OPENINGS  number of random openings; each is played twice with colours swapped

Openings: 8 random plies from the initial position (seed 1, distinct), or
with XOT=FILE, OPENINGS lines drawn from that XOT list (seed 1).
Every engine runs single-threaded; the two engines of a game share one
physical core (they never search at the same time; no pondering). WORKERS
games run at once on CPUs 0, 2, 4, 6 (default 1; time modes only 1,
since games running beside each other change the engines' speed). Hash: Deft 64 MiB, Edax and
Egaroucid 2^21 entries. Writes one line per game (resumes OUT.tsv) and a
summary at the end.
"""
import math, os, random, subprocess, sys, threading, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFT = [str(HERE / "tools/target/release/deft-gtp"),
        os.environ.get("DEFT_MODEL",
                       "/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/mpc-formula/model.dpr2")]
EDAX = "/home/azure/dev/edax-4.5.5/bin/lEdax-x86-64-v3"
EGAR = "/home/azure/dev/bench-engines/dl-egaroucid-7.8.1/Egaroucid-console_v7.8.1/bin/Egaroucid_for_Console_clang.out"
FULL = (1 << 64) - 1
OPP = None
import os
EGAR_LEVEL = int(os.environ.get("EGAR_LEVEL", "5"))
DEBUG = bool(os.environ.get("MATCH_DEBUG"))
DIRS = [(1, 0xfefefefefefefefe), (-1, 0x7f7f7f7f7f7f7f7f), (8, FULL), (-8, FULL),
        (9, 0xfefefefefefefefe), (7, 0x7f7f7f7f7f7f7f7f), (-7, 0xfefefefefefefefe), (-9, 0x7f7f7f7f7f7f7f7f)]


def shift(x, s, mask):
    return ((x << s) & FULL & mask) if s > 0 else ((x >> -s) & mask)


def moves(p, o):
    empty = ~(p | o) & FULL
    m = 0
    for s, mask in DIRS:
        t = shift(p, s, mask) & o
        for _ in range(5):
            t |= shift(t, s, mask) & o
        m |= shift(t, s, mask) & empty
    return m


def flips(p, o, sq):
    bit, f = 1 << sq, 0
    for s, mask in DIRS:
        line, t = 0, shift(bit, s, mask)
        while t & o:
            line |= t
            t = shift(t, s, mask)
        if t & p:
            f |= line
    return f


def name(sq):
    return "abcdefgh"[sq % 8] + str(sq // 8 + 1)


def square(text):
    return (int(text[1]) - 1) * 8 + "abcdefgh".index(text[0].lower())


def xot(n, path, seed=1):
    """n openings drawn without replacement from an XOT list (one move string per line)."""
    lines = [l.strip() for l in open(path) if l.strip()]
    pick = random.Random(seed).sample(lines, n)
    return [[square(m[k:k + 2]) for k in range(0, len(m), 2)] for m in pick]


def openings(n, plies=8, seed=1):
    rng, seen, out = random.Random(seed), set(), []
    while len(out) < n:
        p, o = 0x0000000810000000, 0x0000001008000000  # black to move: d5 e4 black, d4 e5 white
        line = []
        for _ in range(plies):
            m = moves(p, o)
            sqs = [i for i in range(64) if m >> i & 1]
            sq = rng.choice(sqs)
            f = flips(p, o, sq)
            p, o = o ^ f, p | f | (1 << sq)
            line.append(sq)
        if tuple(line) not in seen:
            seen.add(tuple(line))
            out.append(line)
    return out


class Gtp:
    def __init__(self, cmd, cpu, cwd=None, log=None):
        err = open(log, "a") if log else subprocess.DEVNULL
        self.proc = subprocess.Popen(["taskset", "-c", str(cpu), *cmd], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=err, text=True, cwd=cwd, bufsize=1)

    def send(self, line):
        if DEBUG:
            print(">>", self.proc.args[3].split("/")[-1], line, file=sys.stderr, flush=True)
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()
        reply = []
        while True:
            r = self.proc.stdout.readline()
            if r == "":
                raise RuntimeError(f"engine died on {line!r}")
            r = r.rstrip("\n")
            if r == "" and reply:
                break
            if r:
                reply.append(r)
        # Edax sometimes prints a warning line before the reply.
        head = next((r for r in reply if r[:1] in "=?"), reply[0])
        if not head.startswith("="):
            raise RuntimeError(f"{line!r} -> {reply}")
        return head[1:].strip()

    def close(self):
        try:
            self.send("quit")
        except Exception:
            pass
        self.proc.kill()


def seconds_of(mode, kind):
    if not mode.startswith("time"):
        return 0
    opp, _, deft = mode[4:].partition(":")
    return float(deft or opp) if kind == "deft" else float(opp)


def level_of(mode, kind):
    if not mode.startswith("level"):
        return 60
    deft, _, opp = mode[5:].partition(":")
    return int(deft) if kind == "deft" else int(opp or deft)


def engine(kind, mode, cpu):
    level = level_of(mode, kind)
    seconds = seconds_of(mode, kind)
    if kind == "deft":
        args = ["--level", str(level)] if not seconds else ["--game-ms", str(round(seconds * 1000))]
        # DEFT_GTP_LOG=FILE: Deft logs each move's budget and time there.
        return Gtp([*DEFT, *args, "--tt", "64"], cpu, log=os.environ.get("DEFT_GTP_LOG"))
    if kind == "deft-opp":
        args = ["--level", str(level)] if not seconds else ["--game-ms", str(round(seconds * 1000))]
        return Gtp([DEFT[0], os.environ["OPP_MODEL"], *args, "--tt", "64"], cpu)
    if kind == "edax":
        # EDAX_MOVE_TIME=S: a time per move instead of per game.
        if seconds and os.environ.get("EDAX_MOVE_TIME"):
            limit = ["-move-time", os.environ["EDAX_MOVE_TIME"]]
        else:
            limit = ["-t", f"{seconds:g}"] if seconds else ["-l", str(level)]
        return Gtp([EDAX, "-gtp", "-eval-file", "data/eval.dat", "-book-usage", "off", "-n", "1",
                    "-h", "21", "-ponder", "off", *limit], cpu, cwd=Path(EDAX).parent)
    # Egaroucid's GTP genmove ignores -time: in time modes it plays at
    # EGAR_LEVEL, and Deft's time is set to what Egaroucid used.
    extra = ["-l", str(EGAR_LEVEL if seconds else level)]
    return Gtp([EGAR, "-gtp", "-nobook", "-t", "1", "-hash", "21", *extra], cpu, cwd=Path(EGAR).parent)


def play_game(engines, deft_black, opening, mode):
    """engines: {'deft': Gtp, 'opp': Gtp}. Returns (deft disc diff, deft s, opp s, moves)."""
    seconds = seconds_of(mode, "deft")
    for key, e in engines.items():
        e.send("clear_board")
        # Edax and Egaroucid get their game time on the command line
        # (Edax -t, Egaroucid -time); Edax hangs after GTP time_settings.
        if seconds and key == "deft":
            e.send(f"time_settings {seconds:g} 0 0")
    p, o, black = 0x0000000810000000, 0x0000001008000000, True
    used = {"deft": 0.0, "opp": 0.0}
    record = []

    def apply(sq):
        nonlocal p, o, black
        f = flips(p, o, sq)
        assert f, f"illegal move {name(sq)}"
        p, o, black = o ^ f, p | f | (1 << sq), not black

    for sq in opening:
        for e in engines.values():
            e.send(f"play {'b' if black else 'w'} {name(sq)}")
        record.append(name(sq))
        apply(sq)
    while True:
        m = moves(p, o)
        color = "b" if black else "w"
        if m == 0:
            if moves(o, p) == 0:
                break
            for e in engines.values():
                e.send(f"play {color} pass")
            p, o, black = o, p, not black
            continue
        who = "deft" if (black == deft_black) else "opp"
        t = time.monotonic()
        reply = engines[who].send(f"genmove {color}")
        used[who] += time.monotonic() - t
        sq = square(reply)
        assert m >> sq & 1, f"{who} played illegal {reply}"
        other = "opp" if who == "deft" else "deft"
        engines[other].send(f"play {color} {reply}")
        record.append(name(sq))
        apply(sq)
    # p is the side to move; final score from black's view
    bp, wp = (p, o) if black else (o, p)
    b, w = bin(bp).count("1"), bin(wp).count("1")
    e = 64 - b - w
    if b > w:
        b += e
    elif w > b:
        w += e
    diff = b - w if deft_black else w - b
    return diff, used["deft"], used["opp"], "".join(record)


def main():
    global OPP
    opp, mode, n, out = sys.argv[1], sys.argv[2], int(sys.argv[3]), Path(sys.argv[4])
    OPP = opp
    workers = int(sys.argv[5]) if len(sys.argv) > 5 else 1
    if mode.startswith("time") and workers != 1:
        sys.exit("time modes must run one game at a time: other games change the speed")
    games = [(i, deft_black) for i in range(n) for deft_black in (True, False)]
    # XOT=FILE: openings drawn from an XOT list instead of random plies.
    lines = xot(n, os.environ["XOT"]) if os.environ.get("XOT") else openings(n)
    done = set()
    if out.exists():
        for l in out.read_text().splitlines()[1:]:
            f = l.split("\t")
            done.add((int(f[0]), f[1] == "black"))
    else:
        out.write_text("opening\tdeft_color\tdeft_diff\tdeft_seconds\topp_seconds\tmoves\n")
    todo = [g for g in games if g not in done]
    lock = threading.Lock()

    def worker(cpu):
        engines = {"deft": engine("deft", mode, cpu), "opp": engine("deft-opp" if opp == "deft" else opp, mode, cpu)}
        try:
            while True:
                with lock:
                    if not todo:
                        return
                    i, deft_black = todo.pop(0)
                diff, ds, os_, rec = play_game(engines, deft_black, lines[i], mode)
                with lock:
                    with open(out, "a") as w:
                        w.write(f"{i}\t{'black' if deft_black else 'white'}\t{diff}\t{ds:.3f}\t{os_:.3f}\t{rec}\n")
        finally:
            for e in engines.values():
                e.close()

    threads = [threading.Thread(target=worker, args=(cpu,)) for cpu in (0, 2, 4, 6)[:workers]]
    [t.start() for t in threads]
    [t.join() for t in threads]
    summary(out, opp, mode)


def summary(out, opp, mode):
    rows = [l.split("\t") for l in out.read_text().splitlines()[1:]]
    d = [int(r[2]) for r in rows]
    n = len(d)
    w, dr = sum(x > 0 for x in d), sum(x == 0 for x in d)
    score = (w + dr / 2) / n
    mean = sum(d) / n
    sd = math.sqrt(sum((x - mean) ** 2 for x in d) / (n - 1)) / math.sqrt(n)
    elo = lambda s: -400 * math.log10(1 / s - 1) if 0 < s < 1 else float("inf")
    se = math.sqrt(score * (1 - score) / n)
    ds, os_ = sum(float(r[3]) for r in rows), sum(float(r[4]) for r in rows)
    print(f"deft vs {opp} {mode}: games {n}  wins {w} draws {dr} losses {n - w - dr}  score {score:.1%}  "
          f"elo {elo(score):+.0f} [{elo(max(score - 2 * se, 1e-9)):+.0f}, {elo(min(score + 2 * se, 1 - 1e-9)):+.0f}]  "
          f"mean disc diff {mean:+.2f} ± {sd:.2f}  time deft {ds:.0f}s opp {os_:.0f}s (ratio {ds / max(os_, 1e-9):.2f})")


if __name__ == "__main__":
    main()
