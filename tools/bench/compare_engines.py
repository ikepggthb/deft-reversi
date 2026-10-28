#!/usr/bin/env python3
"""Single-core exact endgame comparison; checkpoint every fresh-process solve."""
import argparse
import csv
import hashlib
import io
import json
import re
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


NAMES = ('deft', 'edax', 'egar_simd', 'egar_avx512')
# Williams design: each engine occupies every slot and follows every other
# engine once per four rounds, for each position independently.
ORDERS = ((0, 1, 3, 2), (1, 2, 0, 3), (2, 3, 1, 0), (3, 0, 2, 1))


def parse_result(name, result, empties):
    if name == 'deft':
        rows = list(csv.DictReader(io.StringIO(result.stdout), delimiter='\t'))
        if len(rows) != 1:
            raise RuntimeError('deft did not return exactly one result')
        row = rows[0]
        if row['stop'] != 'Completed' or row['mode'] != 'Exact' or int(row['depth']) != empties:
            raise RuntimeError('deft did not complete exact search')
        return int(row['score']), float(row['seconds']), int(row['nodes']), row['action']
    if name == 'edax':
        matches = re.findall(
            r'^\s*1\|\s*(\d+)\s+([+-]\d+)\s+(\d+):(\d+\.\d+)\s+(\d+)\s+\S+\s+(\w+)',
            result.stdout, re.M)
        if len(matches) != 1 or int(matches[0][0]) != empties:
            raise RuntimeError('Edax did not report full nonselective depth')
        depth, score, minutes, seconds, nodes, action = matches[0]
        return int(score), int(minutes) * 60 + float(seconds), int(nodes), action
    matches = re.findall(
        r'^\|\s*60\|\s*(\d+)@100%\|\s*(\w+)\|\s*([+-]?\d+)\|\s*(\d+):(\d+):(\d+\.\d+)\|\s*(\d+)',
        result.stdout, re.M)
    if len(matches) != 1 or int(matches[0][0]) != empties:
        raise RuntimeError('Egaroucid did not report full depth at 100%')
    depth, action, score, hours, minutes, seconds, nodes = matches[0]
    return int(score), int(hours) * 3600 + int(minutes) * 60 + float(seconds), int(nodes), action


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--edax-eval', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--positions', type=Path, required=True)
    parser.add_argument('--answers', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--cpu', type=int, default=0)
    parser.add_argument('--rounds', type=int, default=8)
    args = parser.parse_args()
    for key in ('root', 'edax_eval', 'model', 'positions', 'answers', 'out'):
        setattr(args, key, getattr(args, key).resolve())
    if args.rounds < 4 or args.rounds % 4:
        parser.error('rounds must be a positive multiple of four')
    positions = [line.strip() for line in args.positions.read_text().splitlines()
                 if line.strip() and not line.startswith('#')]
    answers = [line.strip() for line in args.answers.read_text().splitlines()
               if line.strip() and not line.startswith('#')]
    if not positions or len(positions) != len(answers):
        parser.error('positions/answers mismatch')
    expected = []
    for position, answer in zip(positions, answers):
        if position != answer.split(';')[0].strip():
            parser.error('answer board differs')
        expected.append(max(map(int, re.findall(r':[ \t]*([+-]\d+)', answer))))
    args.out.mkdir(parents=True, exist_ok=True)
    raw = []
    totals = {name: [] for name in NAMES}
    with (args.out / 'results.tsv').open('w') as tsv, \
            (args.out / 'process-output.jsonl').open('w') as logs, \
            tempfile.TemporaryDirectory(prefix='deft-four-bench-') as temporary:
        case = Path(temporary) / 'case.obf'
        writer = None
        for rnd in range(args.rounds):
            elapsed = dict.fromkeys(NAMES, 0.0)
            for index, position in enumerate(positions):
                case.write_text(position + '\n')
                empties = position.split()[0].count('-')
                for slot, which in enumerate(ORDERS[(rnd + index) % 4]):
                    name = NAMES[which]
                    executable = args.root / name / 'engine'
                    if name == 'deft':
                        command = [str(executable), str(args.model), str(case), 'exact', '1', '256',
                                   '--legacy-main-mpc']
                    elif name == 'edax':
                        command = [str(executable), '-solve', str(case), '-n', '1', '-l', '60',
                                   '-h', '23', '-eval-file',
                                   str(args.edax_eval), '-book-usage', 'off']
                    else:
                        command = [str(executable), '-t', '1', '-l', '60', '-nobook',
                                   '-hash', '23', '-s', str(case)]
                    command = ['taskset', '-c', str(args.cpu), *command]
                    started = time.perf_counter()
                    result = subprocess.run(command, cwd=executable.parent, capture_output=True,
                                            text=True, errors='replace', check=True, timeout=180)
                    wall = time.perf_counter() - started
                    logs.write(json.dumps({'round': rnd + 1, 'ffo': index + 40, 'engine': name,
                                           'command': command, 'stdout': result.stdout,
                                           'stderr': result.stderr}) + '\n')
                    logs.flush()
                    score, seconds, nodes, action = parse_result(name, result, empties)
                    if score != expected[index] or seconds <= 0:
                        raise RuntimeError(f'invalid result: {name}, FFO {index + 40}, score {score}')
                    row = dict(round=rnd + 1, ffo=index + 40, engine=name, slot=slot + 1,
                               empties=empties, score=score, action=action, seconds=seconds,
                               nodes=nodes, process_wall_seconds=wall)
                    if writer is None:
                        writer = csv.DictWriter(tsv, fieldnames=list(row), delimiter='\t',
                                                lineterminator='\n')
                        writer.writeheader()
                    writer.writerow(row)
                    tsv.flush()
                    raw.append(row)
                    elapsed[name] += seconds
            for name in NAMES:
                totals[name].append(elapsed[name])
            print(f'round={rnd + 1} ' + ' '.join(f'{n}={elapsed[n]:.6f}s' for n in NAMES), flush=True)
    ratios = {name: [a / b for a, b in zip(totals['deft'], totals[name])]
              for name in NAMES if name != 'deft'}
    per_position = {}
    for index in range(len(positions)):
        per_position[str(index + 40)] = {
            name: {'seconds_median': statistics.median(r['seconds'] for r in raw
                                                       if r['ffo'] == index + 40 and r['engine'] == name),
                   'nodes_values': sorted({r['nodes'] for r in raw
                                           if r['ffo'] == index + 40 and r['engine'] == name})}
            for name in NAMES}
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    report = {'rounds': args.rounds, 'positions': len(positions), 'runs': len(raw),
              'cpu': args.cpu, 'threads': 1, 'main_tt_entries': 1 << 23,
              'orders': [[NAMES[i] for i in order] for order in ORDERS],
              'round_seconds': totals,
              'seconds_median': {n: statistics.median(v) for n, v in totals.items()},
              'paired_deft_time_ratios': ratios,
              'paired_deft_time_ratio_medians': {n: statistics.median(v) for n, v in ratios.items()},
              'deft_faster_rounds': {n: sum(v < 1 for v in values) for n, values in ratios.items()},
              'per_position': per_position, 'all_scores_match_published_answers': True,
              'all_full_depth_and_nonselective': True,
              'all_engine_nodes_stable_across_repetitions': all(len(p[n]['nodes_values']) == 1
                                                              for p in per_position.values() for n in NAMES),
              'binary_sha256': {n: sha(args.root / n / 'engine') for n in NAMES},
              'edax_eval_sha256': sha(args.edax_eval), 'model_sha256': sha(args.model), 'positions_sha256': sha(args.positions),
              'answers_sha256': sha(args.answers)}
    (args.out / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'per_position'}, indent=2))


if __name__ == '__main__':
    main()
