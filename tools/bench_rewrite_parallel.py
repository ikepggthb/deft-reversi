#!/usr/bin/env python3
"""Round-robin native thread scaling, with score equality and raw results."""
import argparse
import csv
import hashlib
import io
import json
import os
import statistics
import subprocess
from pathlib import Path


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--model',type=Path,required=True)
    parser.add_argument('--positions',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--threads',default='1,2,4,8,20')
    parser.add_argument('--cpus',help='CPU IDs in priority order; defaults to max-frequency order')
    parser.add_argument('--rounds',type=int,default=5)
    parser.add_argument('--timeout',type=int,default=120)
    args=parser.parse_args()
    counts=[int(n) for n in args.threads.split(',')]
    if not counts or counts[0]!=1 or len(set(counts))!=len(counts) or any(n<1 for n in counts):parser.error('threads must start with 1 and be distinct positive integers')
    if args.rounds<5:parser.error('use at least 5 rounds')
    allowed=os.sched_getaffinity(0)
    def max_frequency(cpu):
        path=Path(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq')
        return int(path.read_text()) if path.exists() else 0
    cpus=[int(cpu) for cpu in args.cpus.split(',')] if args.cpus else sorted(allowed,key=lambda cpu:(-max_frequency(cpu),cpu))
    if not set(cpus)<=allowed or len(set(cpus))!=len(cpus) or max(counts)>len(cpus):parser.error('CPU affinity does not cover requested thread counts')
    raw=[];totals={n:[] for n in counts};expected=None
    for round_number in range(1,args.rounds+1):
        for threads in counts if round_number%2 else list(reversed(counts)):
            result=subprocess.run([str(args.binary.resolve()),str(args.model.resolve()),str(args.positions.resolve()),'exact',str(threads),'256'],capture_output=True,text=True,timeout=args.timeout,check=True,preexec_fn=lambda:os.sched_setaffinity(0,set(cpus[:threads])))
            rows=list(csv.DictReader(io.StringIO(result.stdout),delimiter='\t'))
            if not rows or any(row['stop']!='Completed' or row['mode']!='Exact' for row in rows):raise RuntimeError('incomplete solve')
            scores=[(row['position'],row['score']) for row in rows]
            if expected is None:expected=scores
            if scores!=expected:raise RuntimeError(f'scores differ: round={round_number} threads={threads}')
            seconds=sum(float(row['seconds']) for row in rows);totals[threads].append(seconds)
            for row in rows:raw.append({'round':round_number,'threads':threads,**row})
            print(f'round={round_number} threads={threads} seconds={seconds:.6f}',flush=True)
    args.out.parent.mkdir(parents=True,exist_ok=True)
    with args.out.open('w') as output:
        writer=csv.DictWriter(output,fieldnames=list(raw[0]),delimiter='\t',lineterminator='\n');writer.writeheader();writer.writerows(raw)
    medians={n:statistics.median(v) for n,v in totals.items()}
    sha=lambda path:hashlib.sha256(path.read_bytes()).hexdigest()
    report={'rounds':args.rounds,'tt_mib':256,'mode':'exact','cold_process_each_run':True,
        'binary_sha256':sha(args.binary),'model_sha256':sha(args.model),'positions_sha256':sha(args.positions),
        'seconds_median':medians,'speedup_vs_one_thread':{n:medians[1]/time for n,time in medians.items()},
        'all_scores_match':True,'cpu_affinity':{n:cpus[:n] for n in counts},'cpu_max_khz':{cpu:max_frequency(cpu) for cpu in cpus}}
    args.out.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))

if __name__=='__main__':main()
