#!/usr/bin/env python3
"""持ち時間制で、FlipSight プロトコルの Deft と Edax / Egaroucid を対局させる。

使い方:
  clock_match.py OPPONENT SECONDS FIRST OPENINGS OUT.tsv [WORKERS]

引数:
  OPPONENT  edax | egaroucid | local/bench.toml で足した edax* / egaroucid* のキー
  SECONDS   1 局あたりの各自の持ち時間（秒、切れ負け）
  FIRST     開局の番号の始め。XOT をシード 20261120 で並べ替えて対称形の重複を除いた一覧
            （match.py の XOT_FIRST と同じ番号）の FIRST 番から OPENINGS 個を使い、
            各開局を先手・後手を入れ替えて 2 局ずつ打つ
  WORKERS   並べて打つ局数（既定 1）。局 i は CPU_LIST の 1 つに固定し、2 エンジンが同じ CPU を
            使う（先読みなしなので同時には探索しない）

エンジン:
  Deft は CLI の `flipsight` サブコマンド。手番ごとに盤面・合法手・残り時間（remaining_ms）と、
  その手番の上限（budget_ms、残り時間と同じ）を JSON Lines で送る。
  Edax は GTP（`-t SECONDS`、genmove の前に `time_left` で残り時間を送る）。
  Egaroucid は GTP では持ち時間を使わないので、コンソールのコマンド（`setboard`・`settime`・`go`）
  で手番ごとに局面と残り時間（秒、切り捨て）を渡す。
  どのエンジンも 1 スレッド、先読みなし、定石なし。ハッシュは Deft 64 MiB、Edax と Egaroucid 2^21。
  1 局ごとにエンジンを起動し直し、起動とモデルの読み込みが終わってから時計を動かす。

時計:
  審判が 1 手ごとに、局面を送ってから着手が返るまでの実時間を測り、手番側の残り時間から引く。
  残り時間を使い切ったら、その側の時間切れ負け（相手から見て +64）。時間切れは表と集計に出す。

環境変数:
  DEFT_MODEL  Deft のモデル（既定は common.py のリリースモデル）
  CPU_LIST    ワーカーの CPU（カンマ区切り、既定 0,1,2,3,4,5,6,7）
  XOT         開局リスト（既定 large。match.py と同じ）

出力:
  OUT.tsv に 1 局ずつ追記し（既存ファイルがあれば続きから再開）、終了時に集計を表示する。
  列は opening, deft_color, deft_diff, deft_seconds, opp_seconds, timeout（deft / opp / 空）, moves。
"""
import json
import math
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

from common import paths
from match import START, flips, moves, name, pair_interval, square, xot_numbered, xot_path

TOOLS = paths()
MODEL = os.environ.get("DEFT_MODEL", str(TOOLS["model"]))


class Process:
    """CPU に固定して起動したエンジンの標準入出力。"""

    def __init__(self, cmd, cpu, cwd=None):
        self.proc = subprocess.Popen(["taskset", "-c", str(cpu), *cmd], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
                                     cwd=cwd, bufsize=1)

    def write(self, line):
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()

    def readline(self):
        line = self.proc.stdout.readline()
        if line == "":
            raise RuntimeError(f"{self.proc.args[3]} exited")
        return line.rstrip("\n")

    def close(self):
        try:
            self.write("quit")
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()


class Deft:
    """FlipSight プロトコルの Deft。"""

    def __init__(self, cpu):
        self.p = Process([str(TOOLS["solve"]), "flipsight", "--model", MODEL, "--threads", "1",
                          "--tt-mib", "64"], cpu)
        # モデルを読み終えるまで待つ（最初の手番の応答は読み込みの完了後に返る）。この手は捨てる。
        p, o = START
        self.move(p, o, True, 100)

    def move(self, p, o, black, remaining_ms, sent=None):
        board = [1 if (p if black else o) >> i & 1 else 2 if (o if black else p) >> i & 1 else 0
                 for i in range(64)]
        m = moves(p, o)
        turn = {"board": board, "color": 1 if black else 2, "legal_moves": [i for i in range(64) if m >> i & 1],
                "game_mode": "std", "board_size": 8, "remaining_ms": remaining_ms, "budget_ms": remaining_ms}
        self.p.write(json.dumps(turn))
        return json.loads(self.p.readline())["move"]

    def opponent_moved(self, black, sq):
        pass

    def close(self):
        self.p.proc.stdin.close()
        self.p.proc.wait(timeout=10)


class Edax:
    """GTP の Edax。genmove の前に time_left で残り時間を送る。"""

    def __init__(self, key, cpu, seconds):
        binary = Path(TOOLS[key])
        self.p = Process([str(binary), "-gtp", "-eval-file", "data/eval.dat", "-book-usage", "off", "-n", "1",
                          "-h", "21", "-ponder", "off", "-t", f"{seconds:g}"], cpu, cwd=binary.parent)
        self.send("clear_board")

    def send(self, line):
        self.p.write(line)
        reply = []
        while True:
            r = self.p.readline()
            if r == "" and reply:
                break
            if r:
                reply.append(r)
        head = next((r for r in reply if r[:1] in "=?"), reply[0])
        if not head.startswith("="):
            raise RuntimeError(f"{line!r} -> {reply}")
        return head[1:].strip()

    def move(self, p, o, black, remaining_ms, sent=None):
        color = "b" if black else "w"
        self.send(f"time_left {color} {remaining_ms / 1000:.3f} 0")
        return square(self.send(f"genmove {color}"))

    def opponent_moved(self, black, sq):
        self.send(f"play {'b' if black else 'w'} {'pass' if sq is None else name(sq)}")

    def close(self):
        self.p.close()


class Egaroucid:
    """コンソールのコマンドの Egaroucid。手番ごとに局面と残り時間を渡して go を送る。"""

    def __init__(self, key, cpu, seconds):
        binary = Path(TOOLS[key])
        self.p = Process([str(binary), "-nobook", "-t", "1", "-hash", "21", "-quiet", "-noboard",
                          "-time", f"{int(seconds)}"], cpu, cwd=binary.parent)
        # version の表示（空行まで）が返れば、評価関数の読み込みが終わっている。
        self.p.write("version")
        while self.p.readline().strip():
            pass

    def move(self, p, o, black, remaining_ms, sent=None):
        b, w = (p, o) if black else (o, p)
        cells = "".join("B" if b >> i & 1 else "W" if w >> i & 1 else "-" for i in range(64))
        color = "B" if black else "W"
        self.p.write(f"setboard {cells} {color}")
        self.p.write(f"settime {color} {remaining_ms // 1000}")
        self.p.write("go")
        return square(self.p.readline().split()[-1])

    def opponent_moved(self, black, sq):
        pass

    def close(self):
        self.p.close()


def opponent(key, cpu, seconds):
    if key.startswith("edax") and key in TOOLS:
        return Edax(key, cpu, seconds)
    if key.startswith("egaroucid") and key in TOOLS:
        return Egaroucid(key, cpu, seconds)
    sys.exit(f"unknown opponent {key!r}")


def play_game(opp_key, cpu, seconds, opening, deft_black):
    """1 局打って `(Deft 視点の石差, Deft の消費秒, 相手の消費秒, 時間切れの側, 棋譜)` を返す。"""
    engines = {"deft": Deft(cpu), "opp": opponent(opp_key, cpu, seconds)}
    try:
        (p, o), black = START, True
        left = {"deft": seconds, "opp": seconds}
        record = []
        for sq in opening:
            engines["opp"].opponent_moved(black, sq)
            f = flips(p, o, sq)
            p, o, black = o ^ f, p | f | (1 << sq), not black
            record.append(name(sq))
        timeout = ""
        while True:
            m = moves(p, o)
            if m == 0:
                if moves(o, p) == 0:
                    break
                engines["opp"].opponent_moved(black, None)
                p, o, black = o, p, not black
                continue
            who = "deft" if black == deft_black else "opp"
            remaining_ms = max(0, math.floor(left[who] * 1000))
            t = time.monotonic()
            sq = engines[who].move(p, o, black, remaining_ms)
            left[who] -= time.monotonic() - t
            if left[who] < 0:
                timeout = who
                break
            if not (0 <= sq < 64 and m >> sq & 1):
                raise RuntimeError(f"{who} played an illegal move {sq} after {''.join(record)}")
            if who == "deft":
                engines["opp"].opponent_moved(black, sq)
            f = flips(p, o, sq)
            p, o, black = o ^ f, p | f | (1 << sq), not black
            record.append(name(sq))
        if timeout:
            diff = -64 if timeout == "deft" else 64
        else:
            bp, wp = (p, o) if black else (o, p)
            b, w = bin(bp).count("1"), bin(wp).count("1")
            e = 64 - b - w
            b, w = (b + e, w) if b > w else (b, w + e) if w > b else (b, w)
            diff = b - w if deft_black else w - b
        return diff, seconds - left["deft"], seconds - left["opp"], timeout, "".join(record)
    finally:
        for e in engines.values():
            e.close()


def main():
    if len(sys.argv) not in (6, 7):
        sys.exit(__doc__)
    opp, seconds, first, n, out = sys.argv[1], float(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), Path(sys.argv[5])
    workers = int(sys.argv[6]) if len(sys.argv) > 6 else 1
    opponent_ok = opp.startswith(("edax", "egaroucid")) and opp in TOOLS
    if not opponent_ok:
        sys.exit(f"unknown opponent {opp!r}")
    numbered = xot_numbered(xot_path(os.environ.get("XOT", "large")))
    games = [(i, deft_black) for i in range(first, first + n) for deft_black in (True, False)]
    done = set()
    if out.exists():
        for l in out.read_text().splitlines()[1:]:
            f = l.split("\t")
            done.add((int(f[0]), f[1] == "black"))
    else:
        out.write_text("opening\tdeft_color\tdeft_diff\tdeft_seconds\topp_seconds\ttimeout\tmoves\n")
    todo = [g for g in games if g not in done]
    cpus = os.environ.get("CPU_LIST", "0,1,2,3,4,5,6,7").split(",")
    if workers > len(cpus):
        sys.exit(f"{workers} workers need as many CPUs; set CPU_LIST")
    lock = threading.Lock()
    errors = []

    def worker(cpu):
        while True:
            with lock:
                if not todo or errors:
                    return
                i, deft_black = todo.pop(0)
            try:
                diff, ds, os_, timeout, rec = play_game(opp, cpu, seconds, numbered[i], deft_black)
            except Exception as e:  # 1 局でも異常があれば止める。
                with lock:
                    errors.append(f"opening {i} ({'black' if deft_black else 'white'}): {e}")
                return
            with lock:
                with open(out, "a") as w:
                    w.write(f"{i}\t{'black' if deft_black else 'white'}\t{diff}\t{ds:.3f}\t{os_:.3f}\t{timeout}\t{rec}\n")

    threads = [threading.Thread(target=worker, args=(cpu,)) for cpu in cpus[:workers]]
    [t.start() for t in threads]
    [t.join() for t in threads]
    if errors:
        sys.exit("\n".join(errors))
    summary(out, opp, seconds)


def summary(out, opp, seconds):
    rows = [l.split("\t") for l in out.read_text().splitlines()[1:]]
    d = [int(r[2]) for r in rows]
    n = len(d)
    w, dr = sum(x > 0 for x in d), sum(x == 0 for x in d)
    pairs, mean, half = pair_interval(rows)
    ds = [float(r[3]) for r in rows]
    os_ = [float(r[4]) for r in rows]
    timeouts = {side: sum(r[5] == side for r in rows) for side in ("deft", "opp")}
    print(f"deft vs {opp} {seconds:g}s: games {n}  wins {w} draws {dr} losses {n - w - dr}  "
          f"score {(w + dr / 2) / n:.1%}  mean disc diff {mean:+.2f} [{mean - half:+.2f}, {mean + half:+.2f}] "
          f"(95% t, {pairs} opening pairs)  time per game deft {sum(ds) / n:.1f}s (max {max(ds):.1f}) "
          f"opp {sum(os_) / n:.1f}s (max {max(os_):.1f})  timeouts deft {timeouts['deft']} opp {timeouts['opp']}")


if __name__ == "__main__":
    main()
