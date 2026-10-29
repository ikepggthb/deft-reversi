#!/usr/bin/env python3
"""Experiment only: run a deft solve, optionally pinning each of its threads to its own CPU.

usage: pin_wrapper.py pin|free BINARY ARGS...  (stdout of the solve is passed through)
The CPUs are the ones the wrapper itself may use (taskset), in order.
"""
import glob, os, subprocess, sys, time
mode, binary, args = sys.argv[1], sys.argv[2], sys.argv[3:]
cpus = sorted(os.sched_getaffinity(0))
p = subprocess.Popen([binary, *args], stdout=subprocess.PIPE, text=True)
if mode == 'pin':
    want = int(args[3])  # exact THREADS TT
    deadline = time.time() + 2
    tids = []
    while time.time() < deadline:
        tids = sorted(int(t.rsplit('/', 1)[1]) for t in glob.glob(f'/proc/{p.pid}/task/*'))
        if len(tids) >= want:
            break
        time.sleep(0.0002)
    for i, tid in enumerate(tids):
        try:
            os.sched_setaffinity(tid, [cpus[i % len(cpus)]])
        except OSError:
            pass
sys.stdout.write(p.communicate()[0])
sys.exit(p.returncode)
