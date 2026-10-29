#!/usr/bin/env python3
"""On top of apply_busy_diag.py and apply_open_diag.py: also sample how many
threads are blocked in a join wait (`JOIN ms avg`)."""
import sys
root = sys.argv[1] + '/deft-reversi-engine/'
def edit(p, old, new, n=1):
    p = root + p
    s = open(p).read()
    assert s.count(old) == n, (p, old[:70], s.count(old))
    open(p, 'w').write(s.replace(old, new))
edit('src/parallel.rs', "pub static DIAG_BUSY:", "pub static DIAG_JOIN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);\npub static DIAG_BUSY:")
edit('src/parallel.rs', """            DIAG_BUSY.fetch_sub(1, Ordering::Relaxed);
            let (mut state, _) = self""", """            DIAG_BUSY.fetch_sub(1, Ordering::Relaxed);
            DIAG_JOIN.fetch_add(1, Ordering::Relaxed);
            let (mut state, _) = self""")
edit('src/parallel.rs', """                .unwrap_or_else(|p| p.into_inner());
            DIAG_BUSY.fetch_add(1, Ordering::Relaxed);""", """                .unwrap_or_else(|p| p.into_inner());
            DIAG_JOIN.fetch_sub(1, Ordering::Relaxed);
            DIAG_BUSY.fetch_add(1, Ordering::Relaxed);""")
q = 'src/search.rs'
edit(q, "                let mut open: Vec<u64> = Vec::new();", "                let mut open: Vec<u64> = Vec::new();\n                let mut join: Vec<u64> = Vec::new();")
edit(q, """                    open[ms] += crate::parallel::DIAG_OPEN.load(Ordering::Relaxed) as u64;""", """                    open[ms] += crate::parallel::DIAG_OPEN.load(Ordering::Relaxed) as u64;
                    if join.len() <= ms {
                        join.resize(ms + 1, 0);
                    }
                    join[ms] += crate::parallel::DIAG_JOIN.load(Ordering::Relaxed) as u64;""")
edit(q, """                        eprintln!("OPEN {} {:.2}", ms, open[ms] as f64 / *n as f64);""", """                        eprintln!("OPEN {} {:.2}", ms, open[ms] as f64 / *n as f64);
                        eprintln!("JOIN {} {:.2}", ms, join[ms] as f64 / *n as f64);""")
