#!/bin/bash
# 1 thread on one P-core (CPU 4), then 20 threads on every core; one solve at a time.
set -e
cd "$(dirname "$0")"
A=../2026-11-01-ffo-engines/ffo40-59-answers.obf
python3 run.py 1 4 3 $A t1 2>&1 | tee t1.log
python3 run.py 20 0-19 5 $A t20 2>&1 | tee t20.log
echo QUEUE-DONE
