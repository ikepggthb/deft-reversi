#!/usr/bin/env python3
"""実験CSVから論文品質の図を生成する(SVG + PDF)。

- フェーズ別MAE曲線: mae_by_phase.{svg,pdf}
- 手番別MAEドットプロット: mae_by_side.{svg,pdf}

配色は Okabe-Ito(カラーブラインド対応)、マーカー・線種併用でモノクロ印刷でも
判別可能。タイトルは論文キャプション前提で省略。
"""

from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

BASE = Path(__file__).parent

# (凡例ラベル, CSV, 色, マーカー, 線種)
SERIES = [
    ("NNUE v2 (100M samples)", "eval-mae_nnue-v2-100m.csv", "#D55E00", "o", "-"),
    ("Pattern, lr=0.15 (100M samples)", "eval-mae_pattern-100m-lr015.csv", "#0072B2", "s", "--"),
    ("Pattern, legacy weights", "eval-mae_old-eval-json.csv", "#555555", "^", ":"),
]

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
    "xtick.major.size": 3,
    "ytick.major.size": 3,
    "svg.fonttype": "path",
}


def load(name: str) -> dict[int, float]:
    result = {}
    for line in (BASE / name).read_text().splitlines():
        parts = line.split(",")
        if parts and parts[0].isdigit():
            result[64 - int(parts[0])] = float(parts[2])  # stones -> mae
    return result


def tidy(ax) -> None:
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)
    ax.grid(axis="y", linestyle=(0, (1, 3)), linewidth=0.5, color="#999999", alpha=0.8)
    ax.set_axisbelow(True)


def save(fig, stem: str) -> None:
    for ext in ("svg", "pdf"):
        fig.savefig(BASE / f"{stem}.{ext}", bbox_inches="tight", pad_inches=0.02)


def phase_figure() -> None:
    fig, ax = plt.subplots(figsize=(5.5, 3.2))
    for label, path, color, marker, ls in SERIES:
        data = load(path)
        stones = sorted(data)
        ax.plot(
            stones,
            [data[s] for s in stones],
            label=label,
            color=color,
            marker=marker,
            markersize=3.2,
            markerfacecolor="white",
            markeredgewidth=0.9,
            linestyle=ls,
            linewidth=1.3,
        )
    ax.set_xlabel("Stones on board")
    ax.set_ylabel("Mean absolute error (discs)")
    ax.set_xlim(0, 64)
    ax.set_ylim(0, 9.8)
    ax.set_xticks(range(0, 61, 10))
    ax.set_yticks(range(0, 10, 2))
    tidy(ax)
    ax.legend(frameon=False, loc="upper right", handlelength=2.6)
    save(fig, "mae_by_phase")
    plt.close(fig)


def side_figure() -> None:
    fig, ax = plt.subplots(figsize=(3.5, 2.6))
    sides = [("Black to move\n(even stones)", 0), ("White to move\n(odd stones)", 1)]
    # (x方向ずらし, ラベルのy方向オフセットpt)
    offsets = ((-0.06, -11.0), (0.0, 5.0), (0.06, -11.0))
    for (label, path, color, marker, _), (off, label_dy) in zip(SERIES, offsets):
        data = load(path)
        values = []
        for _, parity in sides:
            vals = [m for s, m in data.items() if s % 2 == parity]
            values.append(sum(vals) / len(vals))
        x = [i + off for i in range(len(sides))]
        ax.plot(
            x,
            values,
            label=label.replace(" (100M samples)", ""),
            color=color,
            marker=marker,
            markersize=5,
            markerfacecolor="white",
            markeredgewidth=1.1,
            linestyle="-",
            linewidth=0.9,
            alpha=0.9,
        )
        for side_idx, (xi, v) in enumerate(zip(x, values)):
            outward = -10 if side_idx == 0 else 10
            ax.annotate(
                f"{v:.2f}",
                (xi, v),
                textcoords="offset points",
                xytext=(outward, label_dy),
                ha="right" if side_idx == 0 else "left",
                fontsize=7.5,
                color=color,
            )
    ax.set_xticks(range(len(sides)))
    ax.set_xticklabels([s for s, _ in sides])
    ax.set_xlim(-0.4, 1.6)
    ax.set_ylim(0, 6.4)
    ax.set_ylabel("Mean absolute error (discs)")
    tidy(ax)
    ax.legend(frameon=False, loc="lower right", fontsize=7.5)
    save(fig, "mae_by_side")
    plt.close(fig)


def main() -> None:
    plt.rcParams.update(STYLE)
    phase_figure()
    side_figure()
    print("wrote mae_by_phase.{svg,pdf} mae_by_side.{svg,pdf}")


if __name__ == "__main__":
    main()
