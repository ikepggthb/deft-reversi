cd ~/dev/data/pen-exp
B=~/dev/deft-reversi-rewrite/target-native/release/deft-reversi-train
date
$B mpc -m full-lin.dpr2 --data ../egaroucid-rd --out mpc-lin --positions 250 -t 4 > mpc-lin.log 2>&1 &
$B mpc -m full-pen.dpr2 --data ../egaroucid-rd --out mpc-pen --positions 250 -t 4 > mpc-pen.log 2>&1 &
wait
date
