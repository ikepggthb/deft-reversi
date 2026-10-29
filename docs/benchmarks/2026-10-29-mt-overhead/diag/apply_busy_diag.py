#!/usr/bin/env python3
"""Busy-thread timeline: counts threads doing search work (not blocked in a
mailbox, a join wait or the drain), sampled every 50 us by a side thread when
DEFT_DIAG_BUSY is set; prints `BUSY ms avg` per millisecond and `ITER` spans."""
import sys
root = sys.argv[1] + '/deft-reversi-engine/'
def edit(p, old, new, n=1):
    p = root + p
    s = open(p).read()
    assert s.count(old) == n, (p, old[:70], s.count(old))
    open(p, 'w').write(s.replace(old, new))
p = 'src/parallel.rs'
edit(p, "pub(crate) type Job = Box<dyn FnOnce() + Send + 'static>;", "pub(crate) type Job = Box<dyn FnOnce() + Send + 'static>;\npub static DIAG_BUSY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);")
edit(p, """                        // Search jobs catch failures and signal the owning session.
                        job();""", """                        // Search jobs catch failures and signal the owning session.
                        DIAG_BUSY.fetch_add(1, Ordering::Relaxed);
                        job();
                        DIAG_BUSY.fetch_sub(1, Ordering::Relaxed);""")
edit(p, """            state.waiting = true;
            self.available.store(true, Ordering::Release);
            let (mut state, _) = self
                .ready
                .wait_timeout(state, Duration::from_millis(5))
                .unwrap_or_else(|p| p.into_inner());""", """            state.waiting = true;
            self.available.store(true, Ordering::Release);
            DIAG_BUSY.fetch_sub(1, Ordering::Relaxed);
            let (mut state, _) = self
                .ready
                .wait_timeout(state, Duration::from_millis(5))
                .unwrap_or_else(|p| p.into_inner());
            DIAG_BUSY.fetch_add(1, Ordering::Relaxed);""")
s = 'src/search/session.rs'
edit(s, """        while self.pending.load(Ordering::Acquire) != 0 {
            let (next, _) = self""", """        while self.pending.load(Ordering::Acquire) != 0 {
            crate::parallel::DIAG_BUSY.fetch_sub(1, Ordering::Relaxed);
            let r = self""")
edit(s, """                .unwrap_or_else(|p| p.into_inner());
            guard = next;""", """                .unwrap_or_else(|p| p.into_inner());
            crate::parallel::DIAG_BUSY.fetch_add(1, Ordering::Relaxed);
            let (next, _) = r;
            guard = next;""")
q = 'src/search.rs'
edit(q, "        for (depth, final_iteration, sigma, quality) in iterations {\n",
"""        let diag_start = Instant::now();
        let diag_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        crate::parallel::DIAG_BUSY.fetch_add(1, Ordering::Relaxed);
        let diag_sampler = std::env::var_os("DEFT_DIAG_BUSY").map(|_| {
            let stop = diag_stop.clone();
            std::thread::spawn(move || {
                let mut sums: Vec<(u64, u64)> = Vec::new();
                while !stop.load(Ordering::Relaxed) {
                    let ms = diag_start.elapsed().as_micros() as usize / 1000;
                    if sums.len() <= ms {
                        sums.resize(ms + 1, (0, 0));
                    }
                    sums[ms].0 += crate::parallel::DIAG_BUSY.load(Ordering::Relaxed) as u64;
                    sums[ms].1 += 1;
                    std::thread::sleep(Duration::from_micros(50));
                }
                for (ms, (sum, n)) in sums.iter().enumerate() {
                    if *n != 0 {
                        eprintln!("BUSY {} {:.2}", ms, *sum as f64 / *n as f64);
                    }
                }
            })
        });
        for (depth, final_iteration, sigma, quality) in iterations {
            let diag_iter = diag_start.elapsed().as_secs_f64() * 1e3;
""")
edit(q, "            let Some(line) = completed else { break };",
"""            eprintln!("ITER depth={} final={} quality={} start={:.3} end={:.3}", depth, final_iteration, quality, diag_iter, diag_start.elapsed().as_secs_f64() * 1e3);
            let Some(line) = completed else { break };""")
edit(q, """        }
        runtime.drain();
        // Full TT cutoffs return scores""", """        }
        diag_stop.store(true, Ordering::Relaxed);
        if let Some(sampler) = diag_sampler {
            let _ = sampler.join();
        }
        runtime.drain();
        // Full TT cutoffs return scores""")
