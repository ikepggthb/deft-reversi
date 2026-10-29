#!/usr/bin/env python3
"""Compare three fresh-process solve adapters with all six execution orders."""
import argparse
import csv
import hashlib
import io
import itertools
import json
import statistics
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('base', 'core', 'legal', 'model', 'positions', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=6,
                        help='multiples of six balance all execution orders')
    parser.add_argument('--cpu', type=int, default=0)
    parser.add_argument('--round-offset', type=int, default=0,
                        help='zero-based first round, for replaying interrupted work')
    parser.add_argument('--legacy-main-mpc', action='store_true')
    args = parser.parse_args()
    if args.rounds < 1 or args.round_offset < 0:
        parser.error('rounds must be positive and round-offset nonnegative')
    names = ('base', 'core', 'legal')
    orders = list(itertools.permutations(names))
    positions = [line for line in args.positions.read_text().splitlines()
                 if line.strip() and not line.startswith('#')]
    if not positions:
        parser.error('empty position set')
    raw = []
    totals = {name: [] for name in names}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open('w') as checkpoint, tempfile.TemporaryDirectory(prefix='deft-leaf-three-') as temporary:
        writer = None
        case = Path(temporary) / 'case.obf'
        for round_index in range(args.rounds):
            actual_round = round_index + args.round_offset
            elapsed = dict.fromkeys(names, 0.0)
            for index, position in enumerate(positions):
                case.write_text(position + '\n')
                expected = None
                # Every position sees every execution order once per six rounds.
                for name in orders[(actual_round + index) % len(orders)]:
                    command = ['taskset', '-c', str(args.cpu),
                               str(getattr(args, name).resolve()),
                               str(args.model.resolve()), str(case), 'exact', '1', '256']
                    if args.legacy_main_mpc:
                        command.append('--legacy-main-mpc')
                    result = subprocess.run(command, text=True, capture_output=True,
                                            check=True, timeout=120)
                    rows = list(csv.DictReader(io.StringIO(result.stdout), delimiter='\t'))
                    if len(rows) != 1 or rows[0]['stop'] != 'Completed':
                        raise RuntimeError('incomplete solve')
                    row = rows[0]
                    if expected is not None and row['score'] != expected:
                        raise RuntimeError(f'score mismatch: case {index + 1}')
                    expected = row['score']
                    elapsed[name] += float(row['seconds'])
                    raw.append({**row, 'round': actual_round + 1,
                                'version': name, 'position': index + 1})
                    if writer is None:
                        writer = csv.DictWriter(checkpoint, fieldnames=list(raw[-1]),
                                                delimiter='\t', lineterminator='\n')
                        writer.writeheader()
                    writer.writerow(raw[-1])
                    checkpoint.flush()
            for name in names:
                totals[name].append(elapsed[name])
            print('round=' + str(actual_round + 1) + ' ' + ' '.join(
                f'{name}={elapsed[name]:.6f}s' for name in names), flush=True)
    ratios = {name: [a / b for a, b in zip(totals[name], totals['base'])]
              for name in names if name != 'base'}
    legal_core = [a / b for a, b in zip(totals['legal'], totals['core'])]
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    report = {
        'rounds': args.rounds, 'round_offset': args.round_offset,
        'all_six_orders_per_position': args.rounds % 6 == 0, 'cpu': args.cpu, 'mode': 'exact', 'threads': 1,
        'tt_mib': 256, 'legacy_main_mpc': args.legacy_main_mpc,
        'execution_orders': orders, 'round_seconds': totals,
        'seconds_median': {name: statistics.median(values) for name, values in totals.items()},
        'paired_time_ratios_vs_base': ratios,
        'paired_time_ratio_median_vs_base': {name: statistics.median(values)
                                            for name, values in ratios.items()},
        'paired_time_ratios_legal_vs_core': legal_core,
        'paired_time_ratio_median_legal_vs_core': statistics.median(legal_core),
        'all_scores_match': True,
        'binary_sha256': {name: sha(getattr(args, name)) for name in names},
        'model_sha256': sha(args.model), 'positions_sha256': sha(args.positions),
    }
    args.out.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
