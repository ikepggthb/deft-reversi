#!/usr/bin/env python3
"""CPU utilisation of exact solves: process CPU time vs wall and reported search time.

Model loading and table allocation run on one thread before the search; they are
measured from a 1-thread run of the same binary (wall - search) and subtracted
as single-thread CPU when estimating the search's CPU time.
"""
import argparse, os, re, resource, subprocess, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tools' / 'bench'))
from compare_engines import parse_result

B = Path('/home/azure/dev/bench-engines')


def command(engine, case, threads):
    if engine == 'deft':
        return [str(B / 'bin/deft-3b4e83d'), str(B / 'model/deft-eval-main.dpr2'), str(case), 'exact', str(threads), '256', '--legacy-main-mpc']
    if engine.startswith('deft-'):
        return [str(B / 'bin' / engine), str(B / 'model/deft-eval-main.dpr2'), str(case), 'exact', str(threads), '256', '--legacy-main-mpc']
    if engine == 'edax':
        return ['/home/azure/dev/edax-4.5.5/bin/lEdax-x86-64-v3', '-solve', str(case), '-n', str(threads), '-l', '60', '-h', '23',
                '-eval-file', str(B / 'edax/data/eval.dat'), '-book-usage', 'off']
    return [str(B / 'dl-egaroucid-7.8.1/Egaroucid-console_v7.8.1/bin/Egaroucid_for_Console_clang.out'), '-t', str(threads), '-l', '60',
            '-nobook', '-hash', '23', '-s', str(case)]


def run(engine, case, threads, cpus):
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    start = time.perf_counter()
    res = subprocess.run(['taskset', '-c', cpus, *command(engine, case, threads)], capture_output=True, text=True, check=True)
    wall = time.perf_counter() - start
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu = (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)
    empties = case.read_text().split()[0].count('-')
    name = 'deft' if engine.startswith('deft') else 'edax' if engine == 'edax' else 'egar'
    score, seconds, nodes, _ = parse_result(name, res, empties)
    return wall, cpu, seconds, nodes


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--engines', default='deft,edax,egar')
    p.add_argument('--ffo', default='42,45,46,49')
    p.add_argument('--threads', type=int, default=8)
    p.add_argument('--cpus', default='0-7')
    p.add_argument('--repeat', type=int, default=3)
    a = p.parse_args()
    print('ffo\tengine\tthreads\tsearch_s\twall_s\tcpu_s\tsetup_s\tsearch_cpu_s\tutilisation\tnodes')
    for f in a.ffo.split(','):
        case = B / 'cases' / f'ffo{f}.obf'
        for e in a.engines.split(','):
            w1, c1, s1, _ = run(e, case, 1, '4')
            setup = max(w1 - s1, 0.0)
            for _ in range(a.repeat):
                wall, cpu, seconds, nodes = run(e, case, a.threads, a.cpus)
                search_cpu = cpu - setup
                print(f'{f}\t{e}\t{a.threads}\t{seconds:.4f}\t{wall:.4f}\t{cpu:.4f}\t{setup:.4f}\t{search_cpu:.4f}\t{search_cpu / (seconds * a.threads):.3f}\t{nodes}', flush=True)


if __name__ == '__main__':
    main()
