#!/usr/bin/env python3
"""エンジン本体とは独立した座標ルールで対局を再現し、テスト用の小さな局面集を再生成する。

シード 1〜8 でランダムに対局を進め、12・24・36 手目（中盤）と 48 手目（終盤）の局面を
deft-reversi-engine/tests/fixtures/ へ書き出す。あわせてリポジトリ内の
fforum-40-59.obf から先頭 5 問（FFO 40〜44）を切り出して保存する。
"""
from pathlib import Path
import random

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / 'deft-reversi-engine/tests/fixtures'
DIRECTIONS = [(dx,dy) for dx in (-1,0,1) for dy in (-1,0,1) if dx or dy]

def flips(board, turn, pos):
    """8 方向を走査し、指定位置に着手したときに裏返る相手石の座標一覧を返す。"""
    if board[pos]: return []
    result=[]
    for dx,dy in DIRECTIONS:
        x,y=pos%8+dx,pos//8+dy
        ray=[]
        while 0<=x<8 and 0<=y<8:
            square=y*8+x
            if board[square]==-turn:
                ray.append(square)
            else:
                if board[square]==turn: result.extend(ray)
                break
            x+=dx
            y+=dy
    return result

def main():
    """固定シードの対局から中盤・終盤の局面ファイルと FFO 40〜44 のテストフィクスチャを生成する。"""
    mid=[];end=[]
    for seed in range(1,9):
        rng=random.Random(seed)
        board=[0]*64
        board[28]=board[35]=1;board[27]=board[36]=-1
        turn=1
        for ply in range(61):
            line=''.join('X' if cell==1 else 'O' if cell==-1 else '-' for cell in board)+(' X\n' if turn==1 else ' O\n')
            if ply in (12,24,36): mid.append(line)
            if ply==48: end.append(line)
            legal=[(i,flips(board,turn,i)) for i in range(64)]
            legal=[m for m in legal if m[1]]
            if not legal:
                turn=-turn
                legal=[(i,flips(board,turn,i)) for i in range(64)]
                legal=[m for m in legal if m[1]]
                if not legal: break
            pos,ray=rng.choice(legal)
            board[pos]=turn
            for square in ray: board[square]=turn
            turn=-turn
    DEST.mkdir(parents=True,exist_ok=True)
    (DEST/'midgame.obf').write_text('# Seeded coordinate-reference games: seeds 1..8, placements 12/24/36.\n'+''.join(mid))
    (DEST/'endgame.obf').write_text('# Seeded coordinate-reference games: seeds 1..8, placement 48.\n'+''.join(end))
    ffo=(ROOT/'deft-reversi-cli/problem/fforum-40-59.obf').read_text().splitlines(keepends=True)
    (DEST/'ffo40-44.obf').write_text('# FFO 40..44 from repository problem/fforum-40-59.obf\n'+''.join(ffo[:5]))

if __name__=='__main__': main()
