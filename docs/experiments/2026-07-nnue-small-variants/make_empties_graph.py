#!/usr/bin/env python3
"""空きマス数別 valid MAE 曲線(代表5構成、論文品質、SVG + PDF)。"""

import collections
import csv
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

BASE = Path(__file__).parent
REPO = BASE.parent.parent.parent

# (ラベル, run dir, 色, マーカー, 線種)
SERIES = [
    ("S2 shared full acc128, 65 MB", "runs/small-S2-full-acc128", "#D55E00", "o", "-"),
    ("S4 no-corner2x5 acc128, 33 MB", "runs/small-S4-no2x5-acc128", "#0072B2", "s", "--"),
    ("S6 v1 acc64, 9 MB", "runs/small-S6-v1-acc64", "#009E73", "v", "-."),
    ("v2 non-shared, 499 MB", "runs/nnue-v2-100m", "#555555", "^", ":"),
]

# パターン評価(別実験の eval-mae 計測。評価局面は近いが厳密には別サンプル)
PATTERN_SERIES = [
    ("Pattern eval, retrained lr=0.15",
     "docs/experiments/2026-07-nnue-v2-vs-pattern/eval-mae_pattern-100m-lr015.csv",
     "#CC79A7", "D", (0, (3, 1, 1, 1))),
    ("Pattern eval, legacy weights",
     "docs/experiments/2026-07-nnue-v2-vs-pattern/eval-mae_old-eval-json.csv",
     "#E69F00", "P", (0, (5, 2))),
]


def load_eval_mae_csv(path: Path) -> dict[int, float]:
    result = {}
    for line in path.read_text().splitlines():
        parts = line.split(",")
        if parts and parts[0].isdigit():
            result[int(parts[0])] = float(parts[2])
    return result

STYLE = {
    "font.family": "serif",
    "font.serif": ["DejaVu Serif"],
    "mathtext.fontset": "dejavuserif",
    "font.size": 9,
    "axes.labelsize": 10,
    "axes.linewidth": 0.7,
    "xtick.labelsize": 9,
    "ytick.labelsize": 9,
    "legend.fontsize": 8,
    "xtick.direction": "in",
    "ytick.direction": "in",
    "svg.fonttype": "path",
}


def load_last_epoch(run_dir: Path) -> dict[int, float]:
    by = collections.defaultdict(dict)
    for r in csv.DictReader(open(run_dir / "logs/valid_by_empties.csv")):
        by[int(r["epoch"])][int(r["empties"])] = float(r["valid_disc_mae"])
    return by[max(by)]


def main() -> None:
    plt.rcParams.update(STYLE)
    fig, ax = plt.subplots(figsize=(5.5, 3.4))
    for label, run, color, marker, ls in SERIES:
        data = load_last_epoch(REPO / run)
        empties = sorted(data)
        ax.plot(
            empties,
            [data[e] for e in empties],
            label=label,
            color=color,
            marker=marker,
            markersize=3.0,
            markerfacecolor="white",
            markeredgewidth=0.9,
            linestyle=ls,
            linewidth=1.2,
        )
    for label, rel_path, color, marker, ls in PATTERN_SERIES:
        data = load_eval_mae_csv(REPO / rel_path)
        empties = sorted(data)
        ax.plot(
            empties,
            [data[e] for e in empties],
            label=label,
            color=color,
            marker=marker,
            markersize=3.0,
            markerfacecolor="white",
            markeredgewidth=0.9,
            linestyle=ls,
            linewidth=1.2,
        )
    ax.set_xlabel("Empty squares")
    ax.set_ylabel("Validation MAE (discs)")
    ax.set_xlim(0, 60)
    ax.set_ylim(0, 9.8)
    ax.set_xticks(range(0, 61, 10))
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)
    ax.grid(axis="y", linestyle=(0, (1, 3)), linewidth=0.5, color="#999999", alpha=0.8)
    ax.set_axisbelow(True)
    ax.legend(frameon=False, loc="lower right", handlelength=2.8)
    for ext in ("svg", "pdf"):
        fig.savefig(BASE / f"mae_by_empties.{ext}", bbox_inches="tight", pad_inches=0.02)
    print("wrote mae_by_empties.{svg,pdf}")


if __name__ == "__main__":
    main()
