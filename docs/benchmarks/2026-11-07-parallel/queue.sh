#!/bin/bash
# Final check: HEAD (0f11ef5) vs thread-dependent split floor, one run at a time.
cd "$(dirname "$0")"
B=SOLVE=/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/d1ebd8e7-b011-4e04-8fe5-bd518acb9623/scratchpad/solve-base; N=SOLVE=/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/d1ebd8e7-b011-4e04-8fe5-bd518acb9623/scratchpad/solve-new
python3 tune.py 20 0-19 5 runs/final-t20.tsv base=$B new=$N
python3 tune.py 12 0-11 3 runs/final-t12.tsv base=$B new=$N
python3 tune.py 8 0-7 3 runs/final-t8.tsv base=$B new=$N
python3 tune.py 4 4-7 3 runs/final-t4.tsv base=$B new=$N
echo QUEUE-DONE
