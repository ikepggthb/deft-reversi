#!/usr/bin/env python3
"""サイズ vs 精度のトレードオフ図(論文品質、SVG + PDF)。"""

import csv
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

BASE = Path(__file__).parent

STYLE = {
    "font.family": "serif",
    "font.serif": ["DejaVu Serif"],
    "mathtext.fontset": "dejavuserif",
    "font.size": 9,
    "axes.labelsize": 10,
    "axes.linewidth": 0.7,
    "xtick.labelsize": 9,
    "ytick.labelsize": 9,
    "legend.fontsize": 8.5,
    "xtick.direction": "in",
    "ytick.direction": "in",
    "svg.fonttype": "path",
}

GROUPS = {
    "direct (shared rotations)": ("#0072B2", "o"),
    "direct (v2, non-shared)": ("#D55E00", "D"),
    "residual (+legacy pattern eval)": ("#555555", "^"),
}

LABEL_OFFSETS = {  # 点ラベルの位置調整 (dx, dy) pt
    "v2": (0, 7),
    "S1": (0, 7),
    "S2": (0, -11),
    "S3": (8, 3),
    "S4": (-2, 7),
    "S5": (0, 7),
    "S6": (0, 7),
    "R1": (0, 7),
    "R2": (0, 7),
}


def main() -> None:
    plt.rcParams.update(STYLE)
    rows = list(csv.DictReader(open(BASE / "results.csv")))
    fig, ax = plt.subplots(figsize=(5.5, 3.4))
    for row in rows:
        short = row["label"].split()[0]
        if short.startswith("R"):
            group = "residual (+legacy pattern eval)"
        elif short == "v2":
            group = "direct (v2, non-shared)"
        else:
            group = "direct (shared rotations)"
        color, marker = GROUPS[group]
        x = float(row["ft_raw_mb"])
        y = float(row["valid_mae"])
        ax.scatter(x, y, s=34, color=color, marker=marker,
                   facecolor="white", linewidths=1.2, zorder=3,
                   label=group if group not in ax.get_legend_handles_labels()[1] else None)
        dx, dy = LABEL_OFFSETS.get(short, (0, 7))
        ax.annotate(short, (x, y), textcoords="offset points", xytext=(dx, dy),
                    ha="center", fontsize=8, color=color)
    ax.set_xscale("log")
    ax.set_xlabel("Feature-transformer table size (MB, int16)")
    ax.set_ylabel("Validation MAE (discs)")
    ax.set_ylim(4.3, 5.2)
    ax.set_xticks([10, 20, 50, 100, 200, 500])
    ax.get_xaxis().set_major_formatter(plt.ScalarFormatter())
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)
    ax.grid(axis="y", linestyle=(0, (1, 3)), linewidth=0.5, color="#999999", alpha=0.8)
    ax.set_axisbelow(True)
    ax.legend(frameon=False, loc="upper right")
    for ext in ("svg", "pdf"):
        fig.savefig(BASE / f"size_vs_mae.{ext}", bbox_inches="tight", pad_inches=0.02)
    print("wrote size_vs_mae.{svg,pdf}")


if __name__ == "__main__":
    main()
