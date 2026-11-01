#!/bin/sh
# Time-matched self-play: new model (calibrated rules) vs the current model.
set -e
N=/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/e8.dpr2
R=../2026-10-31-mpc-calibration/rules.json
O=/home/azure/dev/bench-engines/model/deft-eval-main.dpr2
S=tools/target/release/selfplay
$S $N@$R $O 21/100 1000 8 4 1 selfplay-100ms.tsv | tee selfplay-100ms.txt
$S $N@$R $O 21/400 500 8 4 1 selfplay-400ms.tsv | tee selfplay-400ms.txt
