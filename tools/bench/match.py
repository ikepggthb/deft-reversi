#!/usr/bin/env python3
"""Deft と Edax / Egaroucid（または別の Deft 構成）を GTP で対戦させる。

使い方:
  match.py OPPONENT MODE OPENINGS OUT.tsv [WORKERS]

引数:
  OPPONENT  edax | egaroucid | deft（OPP_MODEL で指定したモデルを使う Deft）
            | local/bench.toml で足した edax* / egaroucid* のキー（edax-4.7 など、別の版）
  MODE      levelN（両者レベル N）
            | levelD:E（Deft がレベル D、相手がレベル E）
            | timeS（1 局あたり持ち時間 S 秒）
            | timeS:D（相手 S 秒、Deft D 秒。実時間の差を均す用途）
  OPENINGS  対局に使う開始局面の数。各局面を先手・後手を入れ替えて 2 局ずつ対局する。

開始局面:
  既定では初期局面からランダムに 8 手進めた重複のない局面（シード 1）を使う。
  環境変数 XOT=large を指定すると、XOT の開局リスト（large）から OPENINGS 行を抽出する
  （シード 1）。リストは同梱せず、初回に配布元から local/xot/ へ取得する（common.fetch_xot）。
  XOT=FILE で手元の別のリストも指定できる。
  XOT_FIRST=K を併せて指定すると、XOT をシード 20261120 で並べ替えて対称形の重複を除いた一覧の
  K 番から OPENINGS 個を使う（OUT.tsv の opening 列はこの番号）。調整と最終評価で開局を
  分けるための番号で、0〜999 は調整用、1000 以降は評価用（1000〜1299 は時間管理の評価で使用済み）。

集計:
  石差の平均と、開局ペア（同じ開局の先手・後手 2 局の平均）を単位にした 95% t 区間を表示する。

エンジンと並列実行:
  Deft は CLI の `gtp` サブコマンドで起動する（モデルは DEFT_MODEL、既定は common.py の
  リリースモデル）。OPPONENT に deft を指定した場合、相手側は OPP_MODEL を使い、
  OPP_GTP で別の CLI バイナリ（旧ビルドなど）も指定できる。
  各エンジンは 1 スレッドで動作し、1 局の 2 エンジンは同じ物理コアを共有する
  （先読みなしのため同時に探索しない）。WORKERS 局を既定の CPU 0, 2, 4, 6 または
  MATCH_CPUS=0,1,.. 上で並列対局する（既定は 1。時間制限モードでは他対局が速度に
  影響するため 1 のみ許可）。
  DEFT_ENDGAME=STAGES を指定すると、レベル対局の Deft に `--endgame STAGES` を渡し、終盤の段階を
  相手に合わせられる（例：Edax・Egaroucid のレベル 5 は空き 10 以下で完全読みなので
  `83:0,85:0,95:0,98:0,99:0,100:10`）。
  MATCH_THREADS=N を指定すると 1 局ずつ N スレッドで探索する（CPU は全コアまたは
  MATCH_CPUS）。ハッシュサイズは Deft が 64 MiB、Edax と Egaroucid が 2^21 エントリ。
  1 局ごとに OUT.tsv へ追記し（既存ファイルがあれば続きから再開）、終了時に集計を表示する。
"""
import math, os, random, subprocess, sys, threading, time
from pathlib import Path

from common import fetch_xot, paths

TOOLS = paths()
THREADS = os.environ.get("MATCH_THREADS", "1")
# CARGO_TARGET_DIR=target-native でビルドした CLI の GTP インターフェース。
DEFT = [str(TOOLS["gtp"]), "gtp", "--model", os.environ.get("DEFT_MODEL", str(TOOLS["model"]))]
START = (0x0000000810000000, 0x0000001008000000)  # 黒番開始: d5・e4 が黒、d4・e5 が白
FULL = (1 << 64) - 1
OPP = None
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


def xot_path(value):
    """環境変数 XOT の値を開局リストのパスに変換する。large なら配布元のリストを取得して使う。"""
    return fetch_xot(TOOLS["xot"]) if value == "large" else Path(value)


def xot(n, path, seed=1):
    """XOT リスト（1 行 1 棋譜）から重複なしで n 局面を抽出し、手番ごとのマス番号列で返す。"""
    lines = [l.strip() for l in open(path) if l.strip()]
    pick = random.Random(seed).sample(lines, n)
    return [[square(m[k:k + 2]) for k in range(0, len(m), 2)] for m in pick]


def symmetries():
    """盤の 8 通りの対称変換を、マス番号の対応表のリストで返す。"""
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


def xot_numbered(path):
    """XOT リストをシード 20261120 で並べ替え、対称形の重複を除いた一覧（番号 = 添字）を返す。"""
    lines = [l.strip() for l in open(path) if l.strip()]
    random.Random(20261120).shuffle(lines)
    maps, seen, out = symmetries(), set(), []
    for line in lines:
        p, o = START
        for k in range(0, len(line), 2):
            sq = square(line[k:k + 2])
            f = flips(p, o, sq)
            p, o = o ^ f, p | f | (1 << sq)
        key = min((sum(1 << m[s] for s in range(64) if p >> s & 1), sum(1 << m[s] for s in range(64) if o >> s & 1))
                  for m in maps)
        if key not in seen:
            seen.add(key)
            out.append([square(line[k:k + 2]) for k in range(0, len(line), 2)])
    return out


def openings(n, plies=8, seed=1):
    """初期局面からランダムに plies 手進めた重複のない開始局面を n 個生成する。"""
    rng, seen, out = random.Random(seed), set(), []
    while len(out) < n:
        p, o = START
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
    """指定した CPU コアに固定して GTP エンジンプロセスを起動し、対話するラッパー。"""

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
        # Edax は応答行の前に警告行を出力することがあるため、先頭の '=' または '?' を探す。
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
    """指定された種別・モード・CPU コアで GTP エンジンを起動し、Gtp インスタンスを返す。"""
    level = level_of(mode, kind)
    seconds = seconds_of(mode, kind)
    if kind == "deft":
        args = ["--level", str(level)] if not seconds else ["--game-ms", str(round(seconds * 1000))]
        if not seconds and os.environ.get("DEFT_ENDGAME"):
            args += ["--endgame", os.environ["DEFT_ENDGAME"]]
        # DEFT_GTP_LOG=FILE を指定すると Deft の stderr をそこへ書き出す。
        # `trace` feature 付きでビルドした CLI は各手の配分時間と消費時間を記録する。
        return Gtp([*DEFT, *args, "--tt-mib", "64", "--threads", THREADS], cpu, log=os.environ.get("DEFT_GTP_LOG"))
    if kind == "deft-opp":
        args = ["--level", str(level)] if not seconds else ["--game-ms", str(round(seconds * 1000))]
        return Gtp([os.environ.get("OPP_GTP", DEFT[0]), "gtp", "--model", os.environ["OPP_MODEL"], *args, "--tt-mib", "64", "--threads", THREADS], cpu)
    if kind.startswith("edax"):
        edax = str(TOOLS[kind])
        # EDAX_MOVE_TIME=S が設定されている場合は 1 局単位ではなく 1 手あたりの秒数を使う。
        if seconds and os.environ.get("EDAX_MOVE_TIME"):
            limit = ["-move-time", os.environ["EDAX_MOVE_TIME"]]
        else:
            limit = ["-t", f"{seconds:g}"] if seconds else ["-l", str(level)]
        return Gtp([edax, "-gtp", "-eval-file", "data/eval.dat", "-book-usage", "off", "-n", THREADS,
                    "-h", "21", "-ponder", "off", *limit], cpu, cwd=Path(edax).parent)
    # Egaroucid の GTP genmove は -time を無視するため、時間制限モードでは
    # EGAR_LEVEL で固定し、Deft 側の持ち時間を Egaroucid の消費時間に合わせる。
    egar = str(TOOLS[kind])
    extra = ["-l", str(EGAR_LEVEL if seconds else level)]
    return Gtp([egar, "-gtp", "-nobook", "-t", THREADS, "-hash", "21", *extra], cpu, cwd=Path(egar).parent)


def play_game(engines, deft_black, opening, mode):
    """1 局対局して `(Deft 視点の石差, Deft の消費秒数, 相手の消費秒数, 棋譜文字列)` を返す。"""
    seconds = seconds_of(mode, "deft")
    for key, e in engines.items():
        e.send("clear_board")
        # Edax と Egaroucid にはコマンドライン引数（Edax -t, Egaroucid -time）で持ち時間を渡す。
        # Edax は GTP の time_settings を送ると応答しなくなるため Deft にのみ送る。
        if seconds and key == "deft":
            e.send(f"time_settings {seconds:g} 0 0")
    (p, o), black = START, True
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
    # p は手番側の石配置。終局スコアを黒視点にそろえてから Deft 視点の石差を計算する。
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
    if opp != "deft" and not (opp.startswith(("edax", "egaroucid")) and opp in TOOLS):
        sys.exit(f"unknown opponent {opp!r}")
    # XOT が指定されている場合は、ランダム手順の代わりに XOT リストから開始局面を抽出する。
    # XOT_FIRST があれば、番号付きの一覧の XOT_FIRST 番から n 個を使い、番号を opening 列に書く。
    if os.environ.get("XOT") and os.environ.get("XOT_FIRST"):
        first = int(os.environ["XOT_FIRST"])
        numbered = xot_numbered(xot_path(os.environ["XOT"]))
        if first + n > len(numbered):
            sys.exit(f"the numbered XOT list has only {len(numbered)} openings")
        lines = {first + i: numbered[first + i] for i in range(n)}
    else:
        lines = dict(enumerate(xot(n, xot_path(os.environ["XOT"])) if os.environ.get("XOT") else openings(n)))
    games = [(i, deft_black) for i in lines for deft_black in (True, False)]
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

    if THREADS != "1":
        if workers != 1:
            sys.exit("MATCH_THREADS plays one game at a time")
        cpus = [os.environ.get("MATCH_CPUS", f"0-{os.cpu_count() - 1}")]
    else:
        cpus = [int(c) for c in os.environ.get("MATCH_CPUS", "0,2,4,6").split(",")]
    if workers > len(cpus):
        sys.exit(f"{workers} workers need as many CPUs; set MATCH_CPUS")
    threads = [threading.Thread(target=worker, args=(cpu,)) for cpu in cpus[:workers]]
    [t.start() for t in threads]
    [t.join() for t in threads]
    summary(out, opp, mode)


def t975(df):
    """自由度 df の t 分布の 97.5% 点（Cornish-Fisher 展開の近似。df >= 10 で誤差 0.003 未満）。"""
    z = 1.959964
    return z + (z ** 3 + z) / (4 * df) + (5 * z ** 5 + 16 * z ** 3 + 3 * z) / (96 * df ** 2)


def pair_interval(rows):
    """先手・後手がそろった開局ごとに 2 局の石差の平均をとり、`(ペア数, 平均, 95% t 区間の半幅)` を返す。"""
    games = {}
    for r in rows:
        games.setdefault(r[0], []).append(int(r[2]))
    pairs = [sum(d) / 2 for d in games.values() if len(d) == 2]
    n = len(pairs)
    if n < 2:
        return n, float("nan"), float("nan")
    mean = sum(pairs) / n
    sd = math.sqrt(sum((x - mean) ** 2 for x in pairs) / (n - 1))
    return n, mean, t975(n - 1) * sd / math.sqrt(n)


def summary(out, opp, mode):
    rows = [l.split("\t") for l in out.read_text().splitlines()[1:]]
    d = [int(r[2]) for r in rows]
    n = len(d)
    w, dr = sum(x > 0 for x in d), sum(x == 0 for x in d)
    score = (w + dr / 2) / n
    pairs, mean, half = pair_interval(rows)
    elo = lambda s: -400 * math.log10(1 / s - 1) if 0 < s < 1 else float("inf")
    se = math.sqrt(score * (1 - score) / n)
    ds, os_ = sum(float(r[3]) for r in rows), sum(float(r[4]) for r in rows)
    print(f"deft vs {opp} {mode}: games {n}  wins {w} draws {dr} losses {n - w - dr}  score {score:.1%}  "
          f"elo {elo(score):+.0f} [{elo(max(score - 2 * se, 1e-9)):+.0f}, {elo(min(score + 2 * se, 1 - 1e-9)):+.0f}]  "
          f"mean disc diff {mean:+.2f} [{mean - half:+.2f}, {mean + half:+.2f}] (95% t, {pairs} opening pairs)  "
          f"time deft {ds:.0f}s opp {os_:.0f}s (ratio {ds / max(os_, 1e-9):.2f})")


if __name__ == "__main__":
    main()
