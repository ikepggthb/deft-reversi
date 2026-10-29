#!/usr/bin/env python3
"""Summarize compare_threads.py results.tsv, using only complete rounds <= --rounds."""
import argparse, csv, json, statistics
from collections import defaultdict
from pathlib import Path


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('results', type=Path)
    p.add_argument('--rounds', type=int, required=True)
    p.add_argument('--out', type=Path)
    a = p.parse_args()
    rows = [r for r in csv.DictReader(a.results.open(), delimiter='\t') if int(r['round']) <= a.rounds]
    engines = list(dict.fromkeys(r['engine'] for r in rows))
    threads = sorted({int(r['threads']) for r in rows})
    tot = defaultdict(float)
    nodes = defaultdict(int)
    count = defaultdict(int)
    per_pos = defaultdict(list)
    for r in rows:
        k = (int(r['round']), int(r['threads']), r['engine'])
        tot[k] += float(r['seconds'])
        nodes[k] += int(r['nodes'])
        count[k] += 1
        per_pos[(int(r['threads']), r['engine'], int(r['ffo']))].append(float(r['seconds']))
    for k, n in count.items():
        if n != 10:
            raise SystemExit(f'incomplete round {k}: {n} positions')
    rounds = range(1, a.rounds + 1)
    summary = {'rounds': a.rounds, 'runs': len(rows), 'engines': engines, 'threads': threads, 'by_threads': {}}
    for t in threads:
        entry = {}
        for e in engines:
            secs = [tot[(x, t, e)] for x in rounds]
            entry[e] = {'median_seconds': statistics.median(secs), 'min': min(secs), 'max': max(secs),
                        'round_seconds': secs,
                        'nodes_median': statistics.median(nodes[(x, t, e)] for x in rounds),
                        'per_position_median': {f: statistics.median(per_pos[(t, e, f)]) for f in range(40, 50)}}
        for e in engines:
            if e == 'deft':
                continue
            ratios = [tot[(x, t, 'deft')] / tot[(x, t, e)] for x in rounds]
            entry[f'deft/{e}'] = {'median': statistics.median(ratios), 'min': min(ratios), 'max': max(ratios),
                                  'deft_faster_rounds': sum(q < 1 for q in ratios)}
        summary['by_threads'][str(t)] = entry
    for e in engines:
        b = summary['by_threads'][str(threads[0])][e]
        for t in threads:
            s = summary['by_threads'][str(t)][e]
            s['speedup_vs_1'] = b['median_seconds'] / s['median_seconds']
            s['nodes_vs_1'] = s['nodes_median'] / b['nodes_median']
    if a.out:
        a.out.write_text(json.dumps(summary, indent=2) + '\n')
    for t in threads:
        s = summary['by_threads'][str(t)]
        print(f't={t} ' + ' '.join(f"{e}={s[e]['median_seconds']:.3f}s[{s[e]['min']:.3f}-{s[e]['max']:.3f}]"
                                    f" x{s[e]['speedup_vs_1']:.2f} n{s[e]['nodes_vs_1']:.3f}" for e in engines))
        print('    ' + ' '.join(f"{k}={v['median']:.3f}({v['deft_faster_rounds']}/{a.rounds},{v['min']:.3f}-{v['max']:.3f})"
                                for k, v in s.items() if k.startswith('deft/')))
    for t in threads:
        s = summary['by_threads'][str(t)]
        print(f'per-position t={t}: ' + ' '.join(
            f"{f}:" + '/'.join(f"{s[e]['per_position_median'][f]:.3f}" for e in engines) for f in range(40, 50)))


if __name__ == '__main__':
    main()
