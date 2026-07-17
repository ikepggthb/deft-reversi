#!/usr/bin/env python3
import argparse
import csv
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("metrics_csv", type=Path)
    parser.add_argument("--out", type=Path, default=Path("training_metrics.svg"))
    parser.add_argument("--empty-metrics-csv", type=Path, default=None)
    parser.add_argument("--out-by-empties", type=Path, default=None)
    args = parser.parse_args()

    rows = read_rows(args.metrics_csv)
    if not rows:
        raise SystemExit("no metrics rows found")

    train = [row for row in rows if row["kind"] == "train"]
    summary = [row for row in rows if row["kind"] == "summary"]
    svg = render_svg(train, summary)
    args.out.write_text(svg, encoding="utf-8")
    print(f"wrote {args.out}")

    empty_metrics_csv = args.empty_metrics_csv
    if empty_metrics_csv is None:
        empty_metrics_csv = args.metrics_csv.parent / "valid_by_empties.csv"
    if empty_metrics_csv.exists():
        empty_rows = read_rows(empty_metrics_csv)
        if empty_rows:
            out_by_empties = args.out_by_empties or (args.out.parent / "valid_by_empties.svg")
            out_by_empties.write_text(render_empties_svg(empty_rows), encoding="utf-8")
            print(f"wrote {out_by_empties}")


def read_rows(path: Path) -> list[dict[str, float | str]]:
    rows: list[dict[str, float | str]] = []
    with path.open(newline="", encoding="utf-8") as file:
        for row in csv.DictReader(file):
            parsed: dict[str, float | str] = {"kind": row["kind"], "evaluator": row["evaluator"]}
            for key, value in row.items():
                if key in {"kind", "evaluator"}:
                    continue
                parsed[key] = float(value) if value else float("nan")
            rows.append(parsed)
    return rows


def render_svg(train: list[dict[str, float | str]], summary: list[dict[str, float | str]]) -> str:
    width, height = 980, 560
    margin = 64
    plot_w = width - margin * 2
    plot_h = height - margin * 2

    series = []
    if train:
        series.append(("train avg_loss", "#2d6f73", [(float(r["step"]), float(r["avg_loss"])) for r in train]))
        series.append(("train disc_mae", "#8b5a2b", [(float(r["step"]), float(r["disc_mae"])) for r in train]))
    if summary:
        series.append(("valid loss", "#b23a48", [(float(r["step"]), float(r["valid_loss"])) for r in summary]))
        series.append(("valid disc_mae", "#315f9d", [(float(r["step"]), float(r["valid_disc_mae"])) for r in summary]))

    points = [(x, y) for _, _, values in series for x, y in values if y == y]
    if not points:
        raise SystemExit("no plottable numeric values found")
    min_x = min(x for x, _ in points)
    max_x = max(x for x, _ in points)
    min_y = 0.0
    max_y = max(y for _, y in points)
    if max_x <= min_x:
        max_x = min_x + 1.0
    if max_y <= min_y:
        max_y = min_y + 1.0

    def sx(x: float) -> float:
        return margin + (x - min_x) / (max_x - min_x) * plot_w

    def sy(y: float) -> float:
        return height - margin - (y - min_y) / (max_y - min_y) * plot_h

    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" fill="#ffffff"/>',
        f'<line x1="{margin}" y1="{height-margin}" x2="{width-margin}" y2="{height-margin}" stroke="#333"/>',
        f'<line x1="{margin}" y1="{margin}" x2="{margin}" y2="{height-margin}" stroke="#333"/>',
        f'<text x="{width/2}" y="30" text-anchor="middle" font-family="sans-serif" font-size="20">Training metrics</text>',
        f'<text x="{width/2}" y="{height-18}" text-anchor="middle" font-family="sans-serif" font-size="13">optimizer step</text>',
        f'<text x="18" y="{height/2}" transform="rotate(-90 18 {height/2})" text-anchor="middle" font-family="sans-serif" font-size="13">metric value</text>',
    ]

    for i in range(6):
        y_value = min_y + (max_y - min_y) * i / 5
        y = sy(y_value)
        lines.append(f'<line x1="{margin}" y1="{y:.2f}" x2="{width-margin}" y2="{y:.2f}" stroke="#e5e7eb"/>')
        lines.append(f'<text x="{margin-8}" y="{y+4:.2f}" text-anchor="end" font-family="monospace" font-size="11">{y_value:.3g}</text>')

    legend_y = 56
    for idx, (name, color, values) in enumerate(series):
        clean = [(x, y) for x, y in values if y == y]
        if len(clean) == 1:
            x, y = clean[0]
            lines.append(f'<circle cx="{sx(x):.2f}" cy="{sy(y):.2f}" r="3" fill="{color}"/>')
        elif len(clean) > 1:
            d = " ".join(f'{sx(x):.2f},{sy(y):.2f}' for x, y in clean)
            lines.append(f'<polyline points="{d}" fill="none" stroke="{color}" stroke-width="2"/>')
        lx = margin + idx * 190
        lines.append(f'<rect x="{lx}" y="{legend_y}" width="14" height="3" fill="{color}"/>')
        lines.append(f'<text x="{lx+20}" y="{legend_y+6}" font-family="sans-serif" font-size="12">{escape(name)}</text>')

    lines.append("</svg>")
    return "\n".join(lines) + "\n"


def render_empties_svg(rows: list[dict[str, float | str]]) -> str:
    width, height = 980, 560
    margin = 64
    plot_w = width - margin * 2
    plot_h = height - margin * 2
    colors = [
        "#2d6f73",
        "#b23a48",
        "#315f9d",
        "#8b5a2b",
        "#6f4aa1",
        "#4f7f1f",
        "#a34468",
        "#2f6f9f",
    ]

    by_epoch: dict[int, list[tuple[float, float]]] = {}
    for row in rows:
        if row["kind"] != "valid_by_empties":
            continue
        epoch = int(float(row["epoch"]))
        empties = float(row["empties"])
        mae = float(row["valid_disc_mae"])
        if mae == mae:
            by_epoch.setdefault(epoch, []).append((empties, mae))
    if not by_epoch:
        raise SystemExit("no valid_by_empties rows found")

    series = [
        (f"epoch {epoch}", colors[idx % len(colors)], sorted(values))
        for idx, (epoch, values) in enumerate(sorted(by_epoch.items()))
    ]
    points = [(x, y) for _, _, values in series for x, y in values if y == y]
    min_x = min(x for x, _ in points)
    max_x = max(x for x, _ in points)
    min_y = 0.0
    max_y = max(y for _, y in points)
    if max_x <= min_x:
        max_x = min_x + 1.0
    if max_y <= min_y:
        max_y = min_y + 1.0

    def sx(x: float) -> float:
        return margin + (x - min_x) / (max_x - min_x) * plot_w

    def sy(y: float) -> float:
        return height - margin - (y - min_y) / (max_y - min_y) * plot_h

    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" fill="#ffffff"/>',
        f'<line x1="{margin}" y1="{height-margin}" x2="{width-margin}" y2="{height-margin}" stroke="#333"/>',
        f'<line x1="{margin}" y1="{margin}" x2="{margin}" y2="{height-margin}" stroke="#333"/>',
        f'<text x="{width/2}" y="30" text-anchor="middle" font-family="sans-serif" font-size="20">Validation MAE by empties</text>',
        f'<text x="{width/2}" y="{height-18}" text-anchor="middle" font-family="sans-serif" font-size="13">empty squares</text>',
        f'<text x="18" y="{height/2}" transform="rotate(-90 18 {height/2})" text-anchor="middle" font-family="sans-serif" font-size="13">disc MAE</text>',
    ]

    for i in range(6):
        y_value = min_y + (max_y - min_y) * i / 5
        y = sy(y_value)
        lines.append(f'<line x1="{margin}" y1="{y:.2f}" x2="{width-margin}" y2="{y:.2f}" stroke="#e5e7eb"/>')
        lines.append(f'<text x="{margin-8}" y="{y+4:.2f}" text-anchor="end" font-family="monospace" font-size="11">{y_value:.3g}</text>')

    for i in range(6):
        x_value = min_x + (max_x - min_x) * i / 5
        x = sx(x_value)
        lines.append(f'<line x1="{x:.2f}" y1="{margin}" x2="{x:.2f}" y2="{height-margin}" stroke="#f3f4f6"/>')
        lines.append(f'<text x="{x:.2f}" y="{height-margin+18}" text-anchor="middle" font-family="monospace" font-size="11">{x_value:.0f}</text>')

    legend_y = 56
    for idx, (name, color, values) in enumerate(series):
        clean = [(x, y) for x, y in values if y == y]
        if len(clean) == 1:
            x, y = clean[0]
            lines.append(f'<circle cx="{sx(x):.2f}" cy="{sy(y):.2f}" r="3" fill="{color}"/>')
        elif len(clean) > 1:
            d = " ".join(f'{sx(x):.2f},{sy(y):.2f}' for x, y in clean)
            lines.append(f'<polyline points="{d}" fill="none" stroke="{color}" stroke-width="2"/>')
        lx = margin + (idx % 4) * 190
        ly = legend_y + (idx // 4) * 18
        lines.append(f'<rect x="{lx}" y="{ly}" width="14" height="3" fill="{color}"/>')
        lines.append(f'<text x="{lx+20}" y="{ly+6}" font-family="sans-serif" font-size="12">{escape(name)}</text>')

    lines.append("</svg>")
    return "\n".join(lines) + "\n"


def escape(value: str) -> str:
    return value.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


if __name__ == "__main__":
    main()
