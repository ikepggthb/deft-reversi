#!/usr/bin/env python3
"""FFO 形式の終盤局面集を完全読みで解き、複数エンジンの速度とノード数を比較する。

Deft の各構成、Edax 4.5.6、Egaroucid 7.8.2 を同じ CPU コア・スレッド数で実行する。
全局面を毎回新しいプロセスで解き、実行順序は局面・ラウンドごとに回転させ、
1 ラウンドおき（奇数ラウンド）は逆順にする。全スコアを正解と照合し、
OUT/results.tsv（1 局面ごとに追記）と OUT/summary.txt（局面ごとの所要時間・ノード数の
中央値、エンジンごとの合計時間中央値、先頭エンジンとの比率）を出力する。

使い方:
  ffo.py [--threads N] [--cpus LIST] [--rounds R] [--positions FILE] [--only A-B] --out DIR ENGINE...

ENGINE の指定:
  edax | egaroucid | NAME[:key=value,...]（Deft 構成。solve=BINARY, model=FILE, tt=MIB,
  または環境変数用の UPPER_CASE=value を指定できる）

実行例:
  ffo.py --threads 1 --cpus 4 --rounds 3 --out t1 deft edax egaroucid
  ffo.py --threads 20 --cpus 0-19 --rounds 5 --out t20 \\
      base:solve=/tmp/cli-base new:solve=/tmp/cli-new
"""
import argparse
import statistics
from pathlib import Path

from common import POSITIONS, empties, engine, paths, read_answers


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--cpus", default="4", help="taskset CPU list (default 4, one P-core of the 265K)")
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--positions", type=Path, default=POSITIONS / "ffo40-59.obf",
                        help="positions with answers (default FFO 40-59)")
    parser.add_argument("--first", type=int, default=40, help="number of the file's first position (default 40)")
    parser.add_argument("--only", help="a range of position numbers, e.g. 40-49 (default all)")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("engines", nargs="+")
    args = parser.parse_args()
    # 対戦相手のエンジンは各バイナリの配置ディレクトリをカレントディレクトリにして起動するため、
    # --out は絶対パスに解決しておく。
    args.out = args.out.resolve()

    tools = paths()
    engines = [engine(spec, tools) for spec in args.engines]
    names = [e.name for e in engines]
    if len(set(names)) != len(names):
        raise SystemExit("engine names must differ")
    positions = list(enumerate(read_answers(args.positions), args.first))
    if args.only:
        low, _, high = args.only.partition("-")
        positions = [p for p in positions if int(low) <= p[0] <= int(high or low)]
    args.out.mkdir(parents=True, exist_ok=True)
    case = args.out / "case.obf"
    rows = []
    with open(args.out / "results.tsv", "w") as f:
        f.write("round\tposition\tengine\tslot\tempties\tscore\tseconds\tnodes\n")
        for rnd in range(args.rounds):
            for i, (number, (pos, expected)) in enumerate(positions):
                case.write_text(pos + "\n")
                k = (rnd + i) % len(engines)
                order = engines[k:] + engines[:k]
                if rnd % 2:
                    order.reverse()
                for slot, e in enumerate(order):
                    score, seconds, nodes = solve(e, case, args.threads, args.cpus, empties(pos))
                    if expected is not None and score != expected:
                        raise SystemExit(f"{e.name} scored {score} on {number}, expected {expected}")
                    rows.append((rnd, number, e.name, seconds, nodes))
                    f.write(f"{rnd + 1}\t{number}\t{e.name}\t{slot + 1}\t{empties(pos)}\t{score}\t{seconds:.6f}\t{nodes}\n")
                    f.flush()
            totals = "  ".join(f"{n} {sum(r[3] for r in rows if r[0] == rnd and r[2] == n):.2f}s" for n in names)
            print(f"round {rnd + 1}: {totals}", flush=True)
    case.unlink()
    summary = summarize(rows, names, args)
    (args.out / "summary.txt").write_text(summary)
    print(summary)


def solve(e, case, threads, cpus, n_empties):
    """指定エンジンで 1 局面を完全読みし、完了状態と深さを検証したうえで `(石差スコア, 秒数, ノード数)` を返す。"""
    if e.kind == "deft":
        row = e.solve_file(case, "exact", threads, cpus)[0]
        if row["stop"] != "Completed" or row["mode"] != "Exact" or int(row["depth"]) != n_empties:
            raise SystemExit(f"{e.name}: incomplete solve {row}")
        return int(row["score"]), float(row["seconds"]), int(row["nodes"])
    depth, score, seconds, nodes = e.exact(case, threads, cpus)
    if depth != n_empties:
        raise SystemExit(f"{e.name}: solved to depth {depth}, not {n_empties}")
    return score, seconds, nodes


def summarize(rows, names, args):
    """全ラウンドの計測結果を集計し、局面別・エンジン別の中央値と先頭エンジン比の文字列を返す。"""
    rounds = args.rounds
    numbers = sorted({r[1] for r in rows})
    med = lambda xs: statistics.median(xs)
    lines = [f"Median seconds per position (nodes), {args.threads} threads, CPUs {args.cpus}, {rounds} rounds", ""]
    lines.append("pos  " + "".join(f"{n:>24}" for n in names))
    for p in numbers:
        cells = []
        for n in names:
            rs = [r for r in rows if r[1] == p and r[2] == n]
            cells.append(f"{med(r[3] for r in rs):9.3f} ({med(r[4] for r in rs) / 1e6:9.1f}M)")
        lines.append(f"{p:3d}  " + "".join(f"{c:>24}" for c in cells))
    total = lambda n, k: sum(r[3] for r in rows if r[0] == k and r[2] == n)
    nodes = {n: sum(med(r[4] for r in rows if r[1] == p and r[2] == n) for p in numbers) for n in names}
    lines.append("")
    for n in names:
        ratios = [total(n, k) / total(names[0], k) for k in range(rounds)]
        lines.append(f"{n:16} total {med(total(n, k) for k in range(rounds)):9.2f} s  nodes {nodes[n] / 1e9:7.2f}G  "
                     f"vs {names[0]} {med(ratios):.3f} (faster in {sum(x < 1 for x in ratios)}/{rounds} rounds)")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
