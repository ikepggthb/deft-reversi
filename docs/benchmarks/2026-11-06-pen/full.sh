cd ~/dev/data/pen-exp
B=~/dev/deft-reversi-rewrite/target-native/release/deft-reversi-train
$B train --data ../egaroucid-rd --work full-pen --out full-pen.dpr2 --epochs 8 --lr 0.3 --pen --pen-lr 0.03 --pen-dense-lr 0.0003 --threads 4 > full-pen.log 2>&1 &
$B train --data ../egaroucid-rd --work full-lin --out full-lin.dpr2 --epochs 8 --lr 0.3 --threads 2 > full-lin.log 2>&1 &
wait
