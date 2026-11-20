"""GTP referee: python3 referee.py BIN_A BIN_B CLOCK_S OPENINGS N_OPENINGS PLIES WORKER NWORKERS > out
A is the candidate. Each opening is played with both colors."""
import subprocess, sys, time, os
binA, binB, clock, opfile, nop, plies, worker, nworkers = sys.argv[1:9]
clock=float(clock); nop=int(nop); plies=int(plies); worker=int(worker); nworkers=int(nworkers)
def start(b):
    return subprocess.Popen([b,"gtp","--game-ms",str(int(clock*1000)),"--threads","1","--tt-mib","64"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
def cmd(p, c):
    p.stdin.write(c+"\n"); p.stdin.flush()
    out=[]
    while True:
        l=p.stdout.readline()
        if l=="" : raise RuntimeError("engine died")
        if l.strip()=="" and out: break
        if l.strip(): out.append(l.strip())
    r=" ".join(out)
    if r.startswith("?"): raise RuntimeError(c+" -> "+r)
    return r[1:].strip()
engines={"A":start(binA),"B":start(binB)}
ops=[l.strip() for l in open(opfile)][:nop]
for i,line in enumerate(ops):
    if i % nworkers != worker: continue
    moves=[line[2*k:2*k+2] for k in range(plies)]
    for a_black in (True, False):
        col={"A":"b" if a_black else "w","B":"w" if a_black else "b"}
        for e in engines.values():
            cmd(e,"clear_board"); cmd(e,f"time_settings {clock} 0 0")
        # opening: let GTP apply implicit passes
        side="b"
        for m in moves:
            for e in engines.values(): cmd(e,f"play {side} {m}")
            side="w" if side=="b" else "b"
        rem={"A":clock,"B":clock}; flag=None; passes=0
        while True:
            who="A" if col["A"]==side else "B"; other="B" if who=="A" else "A"
            e=engines[who]
            cmd(e,f"time_left {side} {rem[who]:.3f} 0")
            t=time.monotonic(); mv=cmd(e,f"genmove {side}"); used=time.monotonic()-t
            rem[who]-=used
            if rem[who]<0: flag=who; break
            if mv.lower()=="pass":
                passes+=1
                if passes>=2: break
            else:
                passes=0
                cmd(engines[other],f"play {side} {mv}")
            side="w" if side=="b" else "b"
            try: cmd(engines["A"],"final_score"); break
            except RuntimeError: pass
        if flag: score=-64 if flag=="A" else 64
        else:
            fs=cmd(engines["A"],"final_score")
            v=0 if fs=="0" else int(fs[2:])*(1 if fs[0]=="B" else -1)
            score=v if a_black else -v
        print(f"{i}\t{int(a_black)}\t{score}\t{flag or '-'}\t{rem['A']:.3f}\t{rem['B']:.3f}", flush=True)
for e in engines.values(): cmd(e,"quit")
