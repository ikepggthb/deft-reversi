#!/bin/bash
cd "$(dirname "$0")"
until grep -q "^base " tune.log; do sleep 20; done
./bench.sh mid4 DEFT_MID_MARGIN=4
./bench.sh mid6 DEFT_MID_MARGIN=6
./bench.sh mid12 DEFT_MID_MARGIN=12
./bench.sh exact2 DEFT_EXACT_MARGIN=2
./bench.sh exact6 DEFT_EXACT_MARGIN=6
./bench.sh low1 DEFT_LOW_MARGIN=1
./bench.sh low4 DEFT_LOW_MARGIN=4
./bench.sh simple4 DEFT_SIMPLE=4
./bench.sh simple6 DEFT_SIMPLE=6
./bench.sh ordered2 DEFT_ORDERED=2
./bench.sh ordered4 DEFT_ORDERED=4
./bench.sh endord18 DEFT_END_ORDER=18
./bench.sh endord22 DEFT_END_ORDER=22
echo QUEUE1 DONE
