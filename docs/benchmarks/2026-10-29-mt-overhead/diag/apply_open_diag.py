#!/usr/bin/env python3
"""On top of apply_busy_diag.py: also sample the number of split points that
still have unpicked moves (`OPEN ms avg`)."""
import sys
root = sys.argv[1] + '/deft-reversi-engine/'
def edit(p, old, new, n=1):
    p = root + p
    s = open(p).read()
    assert s.count(old) == n, (p, old[:70], s.count(old))
    open(p, 'w').write(s.replace(old, new))
edit('src/parallel.rs', "pub static DIAG_BUSY:", "pub static DIAG_OPEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);\npub static DIAG_BUSY:")
y = 'src/search/ybwc.rs'
edit(y, "            active: AtomicUsize::new(0),\n", "            active: AtomicUsize::new(0),\n            diag_open: AtomicBool::new(true),\n")
edit(y, "    active: AtomicUsize,\n", "    active: AtomicUsize,\n    diag_open: AtomicBool,\n")
edit(y, "        let wanted = parallel::lock(&point.state).candidates.len() - 1;\n", "        let wanted = parallel::lock(&point.state).candidates.len() - 1;\n        crate::parallel::DIAG_OPEN.fetch_add(1, Ordering::Relaxed);\n")
edit(y, """                    let candidate = state.candidates[index];
                    state.next += 1;""", """                    let candidate = state.candidates[index];
                    state.next += 1;
                    if state.next == state.candidates.len() && point.diag_open.swap(false, Ordering::Relaxed) {
                        crate::parallel::DIAG_OPEN.fetch_sub(1, Ordering::Relaxed);
                    }""")
edit(y, """            if state.alpha >= point.beta {
                point.cut.store(true, Ordering::Release);""", """            if state.alpha >= point.beta {
                if point.diag_open.swap(false, Ordering::Relaxed) {
                    crate::parallel::DIAG_OPEN.fetch_sub(1, Ordering::Relaxed);
                }
                point.cut.store(true, Ordering::Release);""")
edit(y, """        self.split_loop(&point);
        let join_start""", """        self.split_loop(&point);
        if point.diag_open.swap(false, Ordering::Relaxed) {
            crate::parallel::DIAG_OPEN.fetch_sub(1, Ordering::Relaxed);
        }
        let join_start""")
q = 'src/search.rs'
edit(q, """                let mut sums: Vec<(u64, u64)> = Vec::new();""", """                let mut sums: Vec<(u64, u64)> = Vec::new();
                let mut open: Vec<u64> = Vec::new();""")
edit(q, """                    sums[ms].1 += 1;""", """                    sums[ms].1 += 1;
                    if open.len() <= ms {
                        open.resize(ms + 1, 0);
                    }
                    open[ms] += crate::parallel::DIAG_OPEN.load(Ordering::Relaxed) as u64;""")
edit(q, """                        eprintln!("BUSY {} {:.2}", ms, *sum as f64 / *n as f64);""", """                        eprintln!("BUSY {} {:.2}", ms, *sum as f64 / *n as f64);
                        eprintln!("OPEN {} {:.2}", ms, open[ms] as f64 / *n as f64);""")
