#!/usr/bin/env python3
"""Clock matches between two Deft CLI builds, over GTP or the FlipSight protocol.

usage:
  referee.py play PROTO NEW_BIN OLD_BIN SECONDS FIRST:LAST OUT.tsv
  referee.py summary OUT.tsv [...]

  PROTO      gtp | flipsight
  SECONDS    game time per side
  FIRST:LAST opening ids (inclusive) in the shuffled, symmetry-deduplicated
             XOT list (tools/bench/positions/xot-openingslarge.txt, seed
             20261120). Ids below 1000 are for tuning, 1000 and above for
             the final evaluation, so the two sets never share a position.

Environment:
  DEFT_MODEL   model (default: the model of tools/bench/common.py)
  THREADS      search threads per engine (default 1)
  CPUS         worker CPU sets separated by ';', as for taskset -c
               (default "0;1;2;3;4;5;6;7"). Both engines of a game share
               their worker's CPUs and never search at the same time.
  LOG_DIR      GTP only: each engine's DEFT_GTP_LOG lines go to
               LOG_DIR/<new|old>-<worker>.log, prefixed with the opening id.
  CLOCK_LOG_DIR  each engine writes its clock log (DEFT_CLOCK_LOG) to
               CLOCK_LOG_DIR/<new|old>-<worker>.jsonl; OUT.logs.tsv gives
               each finished game's byte range in both files.
  EXTEND       LO-HI:SECONDS, extension mode for data (needs CLOCK_LOG_DIR):
               every game starts fresh engines, and `new` runs with
               DEFT_CLOCK_EXTEND=E:SECONDS, E drawn from LO..HI by opening
               and colour. The game ends right after new's extended turn
               (flag x, not scored), as the extra time and table entries
               would change the rest of it.
  STOP_FILE    when this file exists, workers take no new games and the
               run ends after the games in progress (default OUT.tsv.stop).
               Rows are written only for finished games, so a killed run
               loses only its games in progress; rerunning resumes.

Each opening is played with both colours. The referee keeps the clocks:
GTP gets `time_left` before every `genmove`; FlipSight gets remaining_ms and
budget_ms = min(1000, remaining_ms), and each game starts fresh processes
(model loading counts against the first move). A move after the clock (or,
in FlipSight, after budget_ms) loses the game by time: 64 discs.

OUT.tsv: opening, new_black, diff (new's discs minus old's), flag (new | old
| x | -), new_used, old_used, moves. Existing rows are kept and skipped.
A time loss by either side (flag new or old) is a failure to report even
in data runs.
"""
import json, math, os, random, statistics, subprocess, sys, threading, time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "bench"))
from common import paths  # noqa: E402
from match import flips, moves, name, square  # noqa: E402

XOT = Path(__file__).resolve().parents[1] / "bench/positions/xot-openingslarge.txt"
START = (0x0000000810000000, 0x0000001008000000)  # black, white
FIRST_EVAL = 1000


def symmetries():
    maps = []
    for t in range(8):
        m = []
        for sq in range(64):
            r, c = divmod(sq, 8)
            if t & 1:
                c = 7 - c
            if t & 2:
                r = 7 - r
            if t & 4:
                r, c = c, r
            m.append(r * 8 + c)
        maps.append(m)
    return maps


def transform(bits, m):
    return sum(1 << m[sq] for sq in range(64) if bits >> sq & 1)


def openings():
    """The XOT openings in a fixed order, one per position up to symmetry."""
    lines = [l.strip() for l in open(XOT) if l.strip()]
    random.Random(20261120).shuffle(lines)
    maps, seen, out = symmetries(), set(), []
    for line in lines:
        p, o = START
        for k in range(0, len(line), 2):
            sq = square(line[k:k + 2])
            f = flips(p, o, sq)
            p, o = o ^ f, p | f | (1 << sq)
        key = min((transform(p, m), transform(o, m)) for m in maps)
        if key not in seen:
            seen.add(key)
            out.append(line)
    return out


def engine_env(clock_log, extend=None):
    env = dict(os.environ)
    env.pop("DEFT_CLOCK_EXTEND", None)
    if clock_log:
        env["DEFT_CLOCK_LOG"] = clock_log
    if extend:
        env["DEFT_CLOCK_EXTEND"] = extend
    return env


def size(path):
    return os.path.getsize(path) if path and os.path.exists(path) else 0


def extended(path, start):
    """Whether the last turn record written after `start` ran extended."""
    if not path or not os.path.exists(path):
        return False
    with open(path, "rb") as f:
        f.seek(start)
        last = None
        for line in f:
            if line.startswith(b'{"record":"turn"'):
                last = line
    return bool(last) and json.loads(last).get("extended", False)


def extension_empties(opening, new_black):
    lo, hi = EXTEND[0]
    return random.Random(f"{opening}-{int(new_black)}").randint(lo, hi)


class Gtp:
    def __init__(self, binary, seconds, cpus, log, clock_log=None, extend=None):
        self.log, self.clock_log = log, clock_log
        env = engine_env(clock_log, extend)
        if log:
            env["DEFT_GTP_LOG"] = "1"
        self.proc = subprocess.Popen(
            ["taskset", "-c", cpus, binary, "gtp", "--model", MODEL, "--game-ms",
             str(round(seconds * 1000)), "--threads", THREADS, "--tt-mib", "64"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
            stderr=subprocess.PIPE if log else subprocess.DEVNULL, env=env)
        self.tag = "-"
        if log:
            threading.Thread(target=self.copy_log, daemon=True).start()

    def copy_log(self):
        with open(self.log, "a") as out:
            for line in self.proc.stderr:
                if line.startswith("empties"):
                    out.write(f"{self.tag} {line}")
                    out.flush()

    def send(self, line):
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()
        reply = []
        while True:
            r = self.proc.stdout.readline()
            if r == "":
                raise RuntimeError(f"engine died on {line!r}")
            r = r.strip()
            if r == "" and reply:
                break
            if r:
                reply.append(r)
        if not reply[0].startswith("="):
            raise RuntimeError(f"{line!r} -> {reply}")
        return " ".join(reply)[1:].strip()

    def close(self):
        try:
            self.send("quit")
        except Exception:
            pass
        self.proc.kill()


class Flipsight:
    def __init__(self, binary, cpus, clock_log=None, extend=None):
        self.clock_log = clock_log
        self.proc = subprocess.Popen(
            ["taskset", "-c", cpus, binary, "flipsight", "--model", MODEL,
             "--threads", THREADS, "--tt-mib", "64"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, bufsize=1, env=engine_env(clock_log, extend))

    def ask(self, turn):
        self.proc.stdin.write(json.dumps(turn) + "\n")
        self.proc.stdin.flush()
        r = self.proc.stdout.readline()
        if r == "":
            raise RuntimeError("engine died")
        return json.loads(r)["move"]

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=10)


def play(proto, bins, seconds, line, new_black, cpus, logs, opening, clock_logs, extend):
    """One game. Returns (diff for new, flag, new used s, old used s, record,
    {side: (clock log, start, end)})."""
    colour = {"new": "b" if new_black else "w", "old": "w" if new_black else "b"}
    fresh = proto == "flipsight" or extend is not None
    ext = {"new": extend, "old": None}
    if proto == "gtp":
        if fresh:
            eng = {k: Gtp(bins[k], seconds, cpus, logs and logs[k], clock_logs[k], ext[k])
                   for k in ("new", "old")}
        else:
            eng = {k: GTP_ENGINES[cpus][k] for k in ("new", "old")}
        for k, e in eng.items():
            e.tag = str(opening)
            e.send("clear_board")
            e.send(f"time_settings {seconds:g} 0 0")
    else:
        eng = {k: Flipsight(bins[k], cpus, clock_logs[k], ext[k]) for k in ("new", "old")}
    start = {k: size(clock_logs[k]) for k in eng}
    left = {"new": seconds, "old": seconds}
    p, o, black = START[0], START[1], True
    record, flag = [], None

    def advance(sq):
        nonlocal p, o, black
        if sq is None:
            p, o = o, p
        else:
            f = flips(p, o, sq)
            assert f, f"illegal move {name(sq)}"
            p, o = o ^ f, p | f | (1 << sq)
        black = not black

    for k in range(0, len(line), 2):
        sq = square(line[k:k + 2])
        if proto == "gtp":
            for e in eng.values():
                e.send(f"play {'b' if black else 'w'} {name(sq)}")
        record.append(name(sq))
        advance(sq)
    try:
        while True:
            m = moves(p, o)
            side = "b" if black else "w"
            if m == 0:
                if moves(o, p) == 0:
                    break
                if proto == "gtp":
                    for e in eng.values():
                        e.send(f"play {side} pass")
                advance(None)
                continue
            who = "new" if colour["new"] == side else "old"
            if proto == "gtp":
                eng[who].send(f"time_left {side} {max(left[who], 0):.3f} 0")
                t = time.monotonic()
                reply = eng[who].send(f"genmove {side}")
                used = time.monotonic() - t
                sq = square(reply)
                late = used > left[who]
            else:
                bp, wp = (p, o) if black else (o, p)
                board = [1 if bp >> i & 1 else 2 if wp >> i & 1 else 0 for i in range(64)]
                remaining = max(0, math.floor(left[who] * 1000))
                budget = min(1000, remaining)
                turn = {"board": board, "color": 1 if black else 2,
                        "legal_moves": [i for i in range(64) if m >> i & 1],
                        "remaining_ms": remaining, "budget_ms": budget}
                t = time.monotonic()
                sq = eng[who].ask(turn)
                used = time.monotonic() - t
                late = used * 1000 > budget
            left[who] -= used
            if extend and who == "new" and extended(clock_logs["new"], start["new"]):
                flag = "x"
                break
            if late:
                flag = who
                break
            assert m >> sq & 1, f"{who} played illegal {name(sq)}"
            if proto == "gtp":
                other = "old" if who == "new" else "new"
                eng[other].send(f"play {side} {name(sq)}")
            record.append(name(sq))
            advance(sq)
    finally:
        if fresh:
            for e in eng.values():
                e.close()
    spans = {k: (clock_logs[k], start[k], size(clock_logs[k])) for k in eng}
    if flag == "x":
        diff = 0
    elif flag:
        diff = -64 if flag == "new" else 64
    else:
        bp, wp = (p, o) if black else (o, p)
        b, w = bin(bp).count("1"), bin(wp).count("1")
        diff = (b - w) if new_black else (w - b)
    return (diff, flag or "-", seconds - left["new"], seconds - left["old"], "".join(record),
            spans)


GTP_ENGINES = {}


def run(proto, new_bin, old_bin, seconds, ids, out):
    lines = openings()
    first, last = (int(x) for x in ids.split(":"))
    games = [(i, nb) for i in range(first, last + 1) for nb in (True, False)]
    done = set()
    if out.exists():
        for l in out.read_text().splitlines()[1:]:
            f = l.split("\t")
            done.add((int(f[0]), f[1] == "1"))
    else:
        out.write_text("opening\tnew_black\tdiff\tflag\tnew_used\told_used\tmoves\n")
    todo = [g for g in games if g not in done]
    lock = threading.Lock()
    bins = {"new": new_bin, "old": old_bin}
    log_dir = os.environ.get("LOG_DIR")
    clock_dir = os.environ.get("CLOCK_LOG_DIR")
    if EXTEND and not clock_dir:
        sys.exit("EXTEND needs CLOCK_LOG_DIR")
    if clock_dir:
        Path(clock_dir).mkdir(parents=True, exist_ok=True)
    spans_out = out.with_suffix(".logs.tsv")
    if clock_dir and not spans_out.exists():
        spans_out.write_text("opening\tnew_black\textend_empties\tnew_log\tnew_start\tnew_end"
                             "\told_log\told_start\told_end\n")
    stop = Path(os.environ.get("STOP_FILE", str(out) + ".stop"))

    def worker(index, cpus):
        logs = None
        if log_dir:
            Path(log_dir).mkdir(parents=True, exist_ok=True)
            logs = {k: f"{log_dir}/{k}-{index}.log" for k in ("new", "old")}
        clock_logs = {k: clock_dir and f"{clock_dir}/{k}-{index}.jsonl" for k in ("new", "old")}
        if proto == "gtp" and not EXTEND:
            GTP_ENGINES[cpus] = {
                k: Gtp(bins[k], seconds, cpus, logs and logs[k], clock_logs[k])
                for k in ("new", "old")}
        try:
            while True:
                with lock:
                    if not todo or stop.exists():
                        return
                    i, nb = todo.pop(0)
                e = extension_empties(i, nb) if EXTEND else None
                extend = f"{e}:{EXTEND[1]:g}" if EXTEND else None
                diff, flag, nu, ou, rec, spans = play(
                    proto, bins, seconds, lines[i], nb, cpus, logs, i, clock_logs, extend)
                with lock:
                    with open(out, "a") as w:
                        w.write(f"{i}\t{int(nb)}\t{diff}\t{flag}\t{nu:.3f}\t{ou:.3f}\t{rec}\n")
                    if clock_dir:
                        with open(spans_out, "a") as w:
                            w.write(f"{i}\t{int(nb)}\t{e if e is not None else '-'}\t"
                                    + "\t".join(f"{os.path.basename(spans[k][0])}\t{spans[k][1]}\t{spans[k][2]}"
                                                 for k in ("new", "old")) + "\n")
                    if flag in ("new", "old"):
                        print(f"TIME LOSS: opening {i} new_black {int(nb)} flag {flag}",
                              file=sys.stderr, flush=True)
        finally:
            for e in GTP_ENGINES.get(cpus, {}).values():
                e.close()

    cpus = os.environ.get("CPUS", "0;1;2;3;4;5;6;7").split(";")
    threads = [threading.Thread(target=worker, args=(n, c)) for n, c in enumerate(cpus)]
    [t.start() for t in threads]
    [t.join() for t in threads]


def t_quantile(df):
    """Two-sided 95% Student t quantile (Cornish-Fisher series; df >= 5)."""
    z = 1.959963984540054
    return (z + (z**3 + z) / (4 * df) + (5 * z**5 + 16 * z**3 + 3 * z) / (96 * df**2)
            + (3 * z**7 + 19 * z**5 + 17 * z**3 - 15 * z) / (384 * df**3))


def summary(path):
    rows = [l.split("\t") for l in Path(path).read_text().splitlines()[1:]]
    aborted = sum(r[3] == "x" for r in rows)
    rows = [r for r in rows if r[3] != "x"]
    if not rows:
        print(f"{path}\tgames 0\taborted (extension) {aborted}")
        return
    pairs = {}
    for r in rows:
        pairs.setdefault(int(r[0]), []).append(int(r[2]))
    means = [sum(v) / 2 for v in pairs.values() if len(v) == 2]
    n = len(means)
    mean = statistics.fmean(means)
    half = t_quantile(n - 1) * statistics.stdev(means) / math.sqrt(n) if n > 1 else float("inf")
    d = [int(r[2]) for r in rows]
    w, dr = sum(x > 0 for x in d), sum(x == 0 for x in d)
    flags = {k: sum(r[3] == k for r in rows) for k in ("new", "old")}
    used = lambda c: statistics.fmean(float(r[c]) for r in rows)
    print(f"{path}\tgames {len(rows)}\tpairs {n}\tdiff {mean:+.2f} [{mean - half:+.2f}, {mean + half:+.2f}]"
          f"\tW/D/L {w}/{dr}/{len(rows) - w - dr}\tflags new {flags['new']} old {flags['old']}"
          f"\tused new {used(4):.3f} old {used(5):.3f}"
          + (f"\taborted (extension) {aborted}" if aborted else ""))


if __name__ == "__main__":
    MODEL = os.environ.get("DEFT_MODEL", str(paths()["model"]))
    THREADS = os.environ.get("THREADS", "1")
    EXTEND = None
    if os.environ.get("EXTEND"):
        span, secs = os.environ["EXTEND"].split(":")
        lo, hi = (int(x) for x in span.split("-"))
        EXTEND = ((lo, hi), float(secs))
    if sys.argv[1] == "summary":
        for p in sys.argv[2:]:
            summary(p)
    elif sys.argv[1] == "play":
        proto, new_bin, old_bin, seconds, ids, out = sys.argv[2:8]
        run(proto, new_bin, old_bin, float(seconds), ids, Path(out))
    else:
        sys.exit(__doc__)
