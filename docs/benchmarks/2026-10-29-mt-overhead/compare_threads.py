#!/usr/bin/env python3
"""Thread-scaling comparison of deft, Edax and Egaroucid on pinned CPU sets."""
import argparse, csv, json, re, statistics, subprocess, sys, tempfile, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools' / 'bench'))
from compare_engines import parse_result

ENGINES = ('deft', 'edax', 'egar_simd')

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root', type=Path, required=True)
    p.add_argument('--edax-eval', type=Path, required=True)
    p.add_argument('--model', type=Path, required=True)
    p.add_argument('--positions', type=Path, required=True)
    p.add_argument('--answers', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--threads', default='1,2,4')
    p.add_argument('--engines', default=','.join(ENGINES))
    p.add_argument('--rounds', type=int, default=6)
    p.add_argument('--cpusets', required=True, help='THREADS=LIST,... e.g. 1=4;2=4-5')
    a = p.parse_args()
    threads = [int(t) for t in a.threads.split(',')]
    CPUS = {int(k): v for k, v in (x.split('=') for x in a.cpusets.split(';'))}
    engines = a.engines.split(',')
    positions = [l.strip() for l in a.positions.read_text().splitlines() if l.strip()]
    answers = [l.strip() for l in a.answers.read_text().splitlines() if l.strip()]
    expected = [max(map(int, re.findall(r':[ \t]*([+-]\d+)', x))) for x in answers]
    a.out.mkdir(parents=True, exist_ok=True)
    raw = []
    with (a.out / 'results.tsv').open('w') as tsv, (a.out / 'process-output.jsonl').open('w') as logs, \
            tempfile.TemporaryDirectory() as tmp:
        case = Path(tmp) / 'case.obf'
        writer = None
        for rnd in range(a.rounds):
            for t in (threads if rnd % 2 == 0 else threads[::-1]):
                for idx, pos in enumerate(positions):
                    case.write_text(pos + '\n')
                    empties = pos.split()[0].count('-')
                    k = (rnd + idx) % len(engines)
                    order = engines[k:] + engines[:k]
                    for slot, name in enumerate(order):
                        exe = a.root / name / 'engine'
                        if name == 'deft':
                            cmd = [str(exe), str(a.model), str(case), 'exact', str(t), '256', '--legacy-main-mpc']
                        elif name.startswith('edax'):
                            cmd = [str(exe), '-solve', str(case), '-n', str(t), '-l', '60', '-h', '23',
                                   '-eval-file', str(a.edax_eval), '-book-usage', 'off']
                        else:
                            cmd = [str(exe), '-t', str(t), '-l', '60', '-nobook', '-hash', '23', '-s', str(case)]
                        cmd = ['taskset', '-c', CPUS[t], *cmd]
                        start = time.perf_counter()
                        res = subprocess.run(cmd, cwd=exe.parent, capture_output=True, text=True,
                                             errors='replace', check=True, timeout=600)
                        wall = time.perf_counter() - start
                        logs.write(json.dumps({'round': rnd + 1, 'threads': t, 'ffo': idx + 40, 'engine': name,
                                               'command': cmd, 'stdout': res.stdout, 'stderr': res.stderr}) + '\n')
                        logs.flush()
                        score, seconds, nodes, action = parse_result('edax' if name.startswith('edax') else 'egar' if name.startswith('egar') else name, res, empties)
                        if score != expected[idx]:
                            raise RuntimeError(f'wrong score {name} t={t} ffo={idx+40}: {score}')
                        row = dict(round=rnd + 1, threads=t, ffo=idx + 40, engine=name, slot=slot, score=score,
                                   seconds=seconds, nodes=nodes, wall=wall)
                        if writer is None:
                            writer = csv.DictWriter(tsv, fieldnames=list(row), delimiter='\t', lineterminator='\n')
                            writer.writeheader()
                        writer.writerow(row); tsv.flush(); raw.append(row)
                tot = {n: sum(r['seconds'] for r in raw if r['round'] == rnd + 1 and r['threads'] == t and r['engine'] == n) for n in engines}
                print(f'round={rnd+1} threads={t} ' + ' '.join(f'{n}={v:.3f}' for n, v in tot.items()), flush=True)
    summary = {}
    for t in threads:
        for n in engines:
            per_round = [sum(r['seconds'] for r in raw if r['round'] == x and r['threads'] == t and r['engine'] == n)
                         for x in range(1, a.rounds + 1)]
            summary[f'{n}@{t}'] = {'median': statistics.median(per_round), 'rounds': per_round,
                                   'per_position_median': {str(f): statistics.median(r['seconds'] for r in raw if r['threads'] == t and r['engine'] == n and r['ffo'] == f) for f in range(40, 50)},
                                   'nodes_median': statistics.median(sum(r['nodes'] for r in raw if r['round'] == x and r['threads'] == t and r['engine'] == n) for x in range(1, a.rounds + 1))}
    (a.out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    for n in engines:
        base = summary[f'{n}@1']['median']
        print(n, ' '.join(f"t{t}={summary[f'{n}@{t}']['median']:.3f}s(x{base/summary[f'{n}@{t}']['median']:.2f})" for t in threads))

if __name__ == '__main__':
    main()
