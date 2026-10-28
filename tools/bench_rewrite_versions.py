#!/usr/bin/env python3
"""Compare two solve adapters on one CPU with fresh processes per position."""
import argparse
import csv
import hashlib
import io
import json
import statistics
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--positions', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--mode', default='exact')
    parser.add_argument('--rounds', type=int, default=5)
    parser.add_argument('--cpu', type=int, default=1)
    parser.add_argument('--repeat-in-process', type=int, default=1)
    parser.add_argument('--legacy-main-mpc', action='store_true')
    args = parser.parse_args()
    if args.rounds < 5 or args.repeat_in_process < 1:
        parser.error('at least five rounds and one in-process repetition required')
    positions = [line.strip() for line in args.positions.read_text().splitlines()
                 if line.strip() and not line.startswith('#')]
    if not positions:
        parser.error('empty position set')
    raw = []
    totals = {name: [] for name in ('before', 'after')}
    with tempfile.TemporaryDirectory(prefix='deft-versions-') as temporary:
        case = Path(temporary) / 'case.obf'
        for round_number in range(1, args.rounds + 1):
            elapsed = {name: 0.0 for name in totals}
            for index, position in enumerate(positions, 1):
                case.write_text((position + '\n') * args.repeat_in_process)
                expected = None
                for name in ('before', 'after') if round_number % 2 else ('after', 'before'):
                    binary = getattr(args, name)
                    command = ['taskset', '-c', str(args.cpu), str(binary.resolve()),
                               str(args.model.resolve()), str(case), args.mode, '1', '256']
                    if args.legacy_main_mpc:
                        command.append('--legacy-main-mpc')
                    result = subprocess.run(command, capture_output=True, text=True,
                                            check=True, timeout=120)
                    rows = list(csv.DictReader(io.StringIO(result.stdout), delimiter='\t'))
                    if len(rows) != args.repeat_in_process or any(row['stop'] != 'Completed' for row in rows):
                        raise RuntimeError('incomplete solve')
                    if len({row['score'] for row in rows}) != 1:
                        raise RuntimeError('in-process scores differ')
                    row = rows[-1]
                    if expected is not None and row['score'] != expected:
                        raise RuntimeError(f'score mismatch: case {index}')
                    expected = row['score']
                    elapsed[name] += float(row['seconds'])
                    raw.append({**row, 'round': round_number, 'version': name, 'position': index})
            for name in totals:
                totals[name].append(elapsed[name])
            print(f'round={round_number} before={elapsed["before"]:.6f}s after={elapsed["after"]:.6f}s', flush=True)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open('w') as output:
        fields = list(dict.fromkeys(key for row in raw for key in row))
        writer = csv.DictWriter(output, fieldnames=fields, delimiter='\t', lineterminator='\n')
        writer.writeheader()
        writer.writerows(raw)
    medians = {name: statistics.median(times) for name, times in totals.items()}
    paired_ratios = [after / before for before, after in
                     zip(totals['before'], totals['after'])]
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    report = {'cpu': args.cpu, 'rounds': args.rounds, 'mode': args.mode,
              'repeat_in_process': args.repeat_in_process, 'tt_mib': 256,
              'legacy_main_mpc': args.legacy_main_mpc,
              'seconds_median': medians, 'round_seconds': totals,
              'paired_time_ratios': paired_ratios,
              'paired_time_ratio_median': statistics.median(paired_ratios),
              'speedup': medians['before'] / medians['after'], 'all_scores_match': True,
              'before_sha256': sha(args.before), 'after_sha256': sha(args.after),
              'model_sha256': sha(args.model), 'positions_sha256': sha(args.positions)}
    args.out.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
