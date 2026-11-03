#!/bin/sh
# poor man's profiler: sample the solver's stack N times during its final stage
B=$1; M=$2; P=$3; N=$4; START=$5
taskset -c 2 $B $M $P exact 1 256 > /dev/null &
pid=$!
sleep $START
i=0
while [ $i -lt $N ] && kill -0 $pid 2>/dev/null; do
  gdb -p $pid -batch -ex "bt 12" 2>/dev/null | grep -E "^#" | sed -E 's/^#[0-9]+ +(0x[0-9a-f]+ in )?//; s/ \(.*//' | tr '\n' ';' ; echo
  i=$((i+1)); sleep 0.2
done
wait $pid
