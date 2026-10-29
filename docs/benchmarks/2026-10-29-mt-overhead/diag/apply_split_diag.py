#!/usr/bin/env python3
"""On top of apply_diag.py: split points by empties and window, and their waste.

`sp[key*6 + k]`, key = empties*2 + (window wider than null): k=0 splits with a
helper, 1 of them cut, 2 helpers given, 3 nodes of siblings still running or
started when this point was cut, 4 nodes of siblings later in the order than
the cutting move that had already finished (this thread's nodes only),
5 nodes of siblings aborted by an ancestor's cut or a stop.
"""
import sys
root = sys.argv[1] + '/deft-reversi-engine/'
def edit(p, old, new, n=1):
    p = root + p
    s = open(p).read()
    assert s.count(old) == n, (p, old[:70], s.count(old))
    open(p, 'w').write(s.replace(old, new))
edit('src/search/api.rs', "    pub diag_helper_nodes: u64,\n", "    pub diag_helper_nodes: u64,\n    pub diag_sp: Vec<u64>,\n")
edit('src/search/api.rs', "        self.diag_helper_nodes += other.diag_helper_nodes;\n",
     "        self.diag_helper_nodes += other.diag_helper_nodes;\n        diag_add(&mut self.diag_sp, &other.diag_sp);\n")
edit('src/search/api.rs', "    pub fn diag_bump(v: &mut Vec<u64>, i: usize) {",
"""    pub fn diag_addn(v: &mut Vec<u64>, i: usize, n: u64) {
        if v.len() <= i {
            v.resize(i + 1, 0);
        }
        v[i] += n;
    }
    pub fn diag_bump(v: &mut Vec<u64>, i: usize) {""")
edit('src/search.rs', 'local={:?} local_cut={:?}",', 'local={:?} local_cut={:?} sp={:?}",')
edit('src/search.rs', 's.diag_local, s.diag_local_cut\n', 's.diag_local, s.diag_local_cut, s.diag_sp\n')
y = 'src/search/ybwc.rs'
edit(y, """                next: 0,
                alpha,
                best,
            }),""", """                next: 0,
                alpha,
                best,
                diag_done: Vec::new(),
                diag_cut_at: usize::MAX,
            }),
            diag_key: position.board.empty_count() as usize * 2 + usize::from(beta - alpha > 1),""")
edit(y, """    next: usize,
    alpha: i32,
    best: Line,
}""", """    next: usize,
    alpha: i32,
    best: Line,
    diag_done: Vec<(usize, u64)>,
    diag_cut_at: usize,
}""")
edit(y, "    active: AtomicUsize,\n}", "    active: AtomicUsize,\n    diag_key: usize,\n}")
edit(y, """            self.stats.splits += 1;
            SearchStats::diag_bump""", """            self.stats.splits += 1;
            SearchStats::diag_addn(&mut self.stats.diag_sp, point.diag_key * 6, 1);
            SearchStats::diag_addn(&mut self.stats.diag_sp, point.diag_key * 6 + 2, spawned);
            SearchStats::diag_bump""")
edit(y, """        if point.cut.load(Ordering::Acquire) {
            self.stats.diag_split_cut += 1;
        }""", """        if point.cut.load(Ordering::Acquire) {
            self.stats.diag_split_cut += 1;
            let state = parallel::lock(&point.state);
            if spawned != 0 {
                SearchStats::diag_addn(&mut self.stats.diag_sp, point.diag_key * 6 + 1, 1);
                let late: u64 = state.diag_done.iter().filter(|d| d.0 > state.diag_cut_at).map(|d| d.1).sum();
                SearchStats::diag_addn(&mut self.stats.diag_sp, point.diag_key * 6 + 4, late);
            }
        }""")
edit(y, """                    let candidate = state.candidates[index];
                    state.next += 1;
                    Some((candidate, state.alpha))""", """                    let candidate = state.candidates[index];
                    state.next += 1;
                    Some((candidate, state.alpha, index))""")
edit(y, "            let Some((candidate, alpha)) = next else {", "            let Some((candidate, alpha, my_index)) = next else {")
edit(y, """            let Ok(Some(line)) = searched else {
                self.stats.diag_wasted += self.stats.nodes - before;
                break;
            };
            if point.cut.load(Ordering::Acquire) {
                self.stats.diag_wasted += self.stats.nodes - before;
            }""", """            let mut late = false;
            let Ok(Some(line)) = searched else {
                let w = self.stats.nodes - before;
                self.stats.diag_wasted += w;
                let own = point.cut.load(Ordering::Acquire);
                SearchStats::diag_addn(&mut self.stats.diag_sp, point.diag_key * 6 + if own { 3 } else { 5 }, w);
                break;
            };
            if point.cut.load(Ordering::Acquire) {
                let w = self.stats.nodes - before;
                self.stats.diag_wasted += w;
                SearchStats::diag_addn(&mut self.stats.diag_sp, point.diag_key * 6 + 3, w);
                late = true;
            }""")
edit(y, """            let mut state = parallel::lock(&point.state);
            if line.score > state.best.score {""", """            let mut state = parallel::lock(&point.state);
            if !late {
                state.diag_done.push((my_index, self.stats.nodes - before));
            }
            if line.score > state.best.score {""")
edit(y, """            if state.alpha >= point.beta {
                point.cut.store(true, Ordering::Release);""", """            if state.alpha >= point.beta {
                if state.diag_cut_at == usize::MAX {
                    state.diag_cut_at = my_index;
                }
                point.cut.store(true, Ordering::Release);""")
