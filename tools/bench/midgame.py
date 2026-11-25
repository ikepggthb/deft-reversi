#!/usr/bin/env python3
"""Deft の複数構成について、中盤探索の速度と着手の質を比較する。

150 局面の中盤テストセット（mid150.obf: 空きマス 32・40・48 を各 50 局面）を
指定した各レベルで探索する。実行は構成・レベルごとに 1 プロセスずつ順番に行い、
構成の実行順序はラウンドごとに反転する。
着手の質は参照探索（既定は positions/mid150-level24.tsv: 小型ニューラルネット併用モデルで
250 局面 MPC を用いたレベル 24 探索結果）と比べた一致手率および
評価値差の絶対値平均で評価する。

各探索結果を OUT/NAME-rR-lL.tsv に書き出し、構成・レベルごとの所要時間中央値、
総ノード数中央値、および着手の質を表示する。

使い方:
  midgame.py [--levels 21,18] [--threads N] [--cpus LIST] [--rounds R] --out DIR CONFIG...
  CONFIG は ffo.py と同じ NAME[:key=value,...] 形式で指定する。
"""
import argparse
import csv
import statistics
from pathlib import Path

from common import POSITIONS, Deft, paths


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--levels", default="21,18")
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--cpus", default="2")
    parser.add_argument("--rounds", type=int, default=1)
    parser.add_argument("--positions", type=Path, default=POSITIONS / "mid150.obf")
    parser.add_argument("--reference", type=Path, default=POSITIONS / "mid150-level24.tsv")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("configs", nargs="+")
    args = parser.parse_args()

    tools = paths()
    configs = [Deft(spec, tools) for spec in args.configs]
    levels = args.levels.split(",")
    reference = list(csv.DictReader(open(args.reference), delimiter="\t")) if args.reference.exists() else None
    args.out.mkdir(parents=True, exist_ok=True)
    results = {}
    for rnd in range(args.rounds):
        order = configs if rnd % 2 == 0 else configs[::-1]
        for c in order:
            for level in levels:
                rows = c.solve_file(args.positions, f"level{level}", args.threads, args.cpus)
                path = args.out / f"{c.name}-r{rnd + 1}-l{level}.tsv"
                with open(path, "w") as f:
                    w = csv.DictWriter(f, fieldnames=rows[0].keys(), delimiter="\t")
                    w.writeheader()
                    w.writerows(rows)
                results.setdefault((c.name, level), []).append(rows)
                print(f"round {rnd + 1} {c.name} level {level}: {sum(float(r['seconds']) for r in rows):.2f}s",
                      flush=True)
    for c in configs:
        for level in levels:
            runs = results[(c.name, level)]
            seconds = statistics.median(sum(float(r["seconds"]) for r in rows) for rows in runs)
            nodes = statistics.median(sum(int(r["nodes"]) for r in rows) for rows in runs)
            line = f"{c.name:16} level {level}: {seconds:8.2f} s {nodes / 1e6:8.0f}M"
            if reference:
                rows = runs[0]
                same = sum(a["action"] == b["action"] for a, b in zip(rows, reference)) / len(rows)
                diff = sum(abs(int(a["score"]) - int(b["score"])) for a, b in zip(rows, reference)) / len(rows)
                line += f"  same move {same:5.1%}  |d| {diff:4.2f}"
            print(line)


if __name__ == "__main__":
    main()
