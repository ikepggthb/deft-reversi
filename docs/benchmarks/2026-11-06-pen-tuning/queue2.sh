#!/bin/bash
cd "$(dirname "$0")"
./bench.sh base2
./bench.sh simple4b DEFT_SIMPLE=4
./bench.sh low4b DEFT_LOW_MARGIN=4
./bench.sh low6 DEFT_LOW_MARGIN=6
./bench.sh exact2b DEFT_EXACT_MARGIN=2
./bench.sh combo DEFT_SIMPLE=4 DEFT_LOW_MARGIN=4 DEFT_EXACT_MARGIN=2
./bench.sh combo-s4l4 DEFT_SIMPLE=4 DEFT_LOW_MARGIN=4
./bench.sh base3
echo QUEUE2 DONE
