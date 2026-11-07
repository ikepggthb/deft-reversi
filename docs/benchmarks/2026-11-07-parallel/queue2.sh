#!/bin/bash
# Stage-aware split floor (new2) vs the thread-count floor alone (new) and HEAD (base).
cd "$(dirname "$0")"
B=SOLVE=/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/d1ebd8e7-b011-4e04-8fe5-bd518acb9623/scratchpad/solve-base; N=SOLVE=/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/d1ebd8e7-b011-4e04-8fe5-bd518acb9623/scratchpad/solve-new; N2=SOLVE=/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/d1ebd8e7-b011-4e04-8fe5-bd518acb9623/scratchpad/solve-new2
python3 tune.py 20 0-19 5 runs/final2-t20.tsv new=$N new2=$N2 base=$B
python3 tune.py 12 0-11 3 runs/final2-t12.tsv new=$N new2=$N2 base=$B
python3 tune.py 8 0-7 3 runs/final2-t8.tsv new=$N new2=$N2 base=$B
echo QUEUE-DONE
