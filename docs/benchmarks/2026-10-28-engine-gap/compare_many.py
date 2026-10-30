#!/usr/bin/env python3
"""Compare N deft solve binaries: fresh process per position, rotated order, answer check."""
import argparse
import csv
import hashlib
import io
import json
import re
import statistics
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('variants', nargs='+', help='NAME=BINARY')
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--positions', type=Path, required=True)
    parser.add_argument('--answers', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=6)
    parser.add_argument('--cpu', type=int, default=2)
    parser.add_argument('--tt-mib', default='256')
    args = parser.parse_args()
    variants = [tuple(v.split('=', 1)) for v in args.variants]
    names = [n for n, _ in variants]
    k = len(variants)
    if args.rounds % k:
        parser.error('rounds must be a multiple of the variant count')
    positions = [l.strip() for l in args.positions.read_text().splitlines() if l.strip() and not l.startswith('#')]
    answers = [l.strip() for l in args.answers.read_text().splitlines() if l.strip() and not l.startswith('#')]
    expected = [max(map(int, re.findall(r':[ \t]*([+-]\d+)', a))) for a in answers]
    assert len(positions) == len(expected)
    args.out.mkdir(parents=True, exist_ok=True)
    raw = []
    totals = {n: [] for n in names}
    with (args.out / 'results.tsv').open('w') as tsv, tempfile.TemporaryDirectory() as tmp:
        case = Path(tmp) / 'case.obf'
        writer = None
        for rnd in range(args.rounds):
            elapsed = dict.fromkeys(names, 0.0)
            for index, position in enumerate(positions):
                case.write_text(position + '\n')
                empties = position.split()[0].count('-')
                shift = (rnd + index) % k
                order = names[shift:] + names[:shift]
                if (rnd // k) % 2:
                    order = order[::-1]
                for slot, name in enumerate(order):
                    binary = dict(variants)[name]
                    command = ['taskset', '-c', str(args.cpu), binary, str(args.model), str(case),
                               'exact', '1', args.tt_mib, '--legacy-main-mpc']
                    result = subprocess.run(command, capture_output=True, text=True, check=True, timeout=600)
                    rows = list(csv.DictReader(io.StringIO(result.stdout), delimiter='\t'))
                    row = rows[0]
                    if (len(rows) != 1 or row['stop'] != 'Completed' or row['mode'] != 'Exact'
                            or int(row['depth']) != empties or int(row['score']) != expected[index]):
                        raise RuntimeError(f'bad result {name} position {index}: {row}')
                    out = dict(round=rnd + 1, position=index, variant=name, slot=slot,
                               score=row['score'], action=row['action'], nodes=row['nodes'],
                               seconds=row['seconds'])
                    if writer is None:
                        writer = csv.DictWriter(tsv, fieldnames=list(out), delimiter='\t', lineterminator='\n')
                        writer.writeheader()
                    writer.writerow(out)
                    tsv.flush()
                    raw.append(out)
                    elapsed[name] += float(row['seconds'])
            for n in names:
                totals[n].append(elapsed[n])
            print(f'round={rnd + 1} ' + ' '.join(f'{n}={elapsed[n]:.4f}' for n in names), flush=True)
    base = names[0]
    report = {'cpu': args.cpu, 'rounds': args.rounds, 'tt_mib': args.tt_mib, 'variants': dict(variants),
              'round_seconds': totals,
              'median_seconds': {n: statistics.median(v) for n, v in totals.items()},
              'paired_ratio_median': {n: statistics.median(a / b for a, b in zip(totals[n], totals[base]))
                                      for n in names},
              'faster_rounds_than_base': {n: sum(a < b for a, b in zip(totals[n], totals[base])) for n in names},
              'nodes_per_round': {n: sorted({sum(int(r['nodes']) for r in raw if r['variant'] == n and r['round'] == x)
                                             for x in range(1, args.rounds + 1)}) for n in names},
              'per_position_median': {str(i + 40): {n: statistics.median(float(r['seconds']) for r in raw
                                                                       if r['variant'] == n and r['position'] == i)
                                                    for n in names} for i in range(len(positions))},
              'sha256': {n: hashlib.sha256(Path(p).read_bytes()).hexdigest() for n, p in variants},
              'model_sha256': hashlib.sha256(args.model.read_bytes()).hexdigest()}
    (args.out / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    for n in names:
        print(f"{n:>14}: median {report['median_seconds'][n]:.4f}s ratio {report['paired_ratio_median'][n]:.4f} "
              f"faster {report['faster_rounds_than_base'][n]}/{args.rounds} nodes {report['nodes_per_round'][n]}")


if __name__ == '__main__':
    main()
