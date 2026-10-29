#!/usr/bin/env python3
"""On top of apply_diag.py and apply_split_diag.py: expected node type of split
points (0 PV = wider than a null window, 1 CUT = null-window child of PV or ALL,
2 ALL = null-window child of CUT). `spt[type*6 + k]` has the fields of `sp`.
"""
import sys
root = sys.argv[1] + '/deft-reversi-engine/'
def edit(p, old, new, n=1):
    p = root + p
    s = open(p).read()
    assert s.count(old) == n, (p, old[:70], s.count(old))
    open(p, 'w').write(s.replace(old, new))
edit('src/search/api.rs', "    pub diag_sp: Vec<u64>,\n", "    pub diag_sp: Vec<u64>,\n    pub diag_spt: Vec<u64>,\n")
edit('src/search/api.rs', "        diag_add(&mut self.diag_sp, &other.diag_sp);\n",
     "        diag_add(&mut self.diag_sp, &other.diag_sp);\n        diag_add(&mut self.diag_spt, &other.diag_spt);\n")
edit('src/search.rs', ' sp={:?}",', ' sp={:?} spt={:?}",')
edit('src/search.rs', 's.diag_sp\n', 's.diag_sp, s.diag_spt\n')
s = 'src/search/session.rs'
edit(s, "    pub(super) end_cache: EndgameCache,\n    #[cfg", "    pub(super) end_cache: EndgameCache,\n    pub(super) diag_type: u8,\n    #[cfg")
t = 'src/search/tree.rs'
edit(t, """    pub(super) fn node(
        &mut self,
        position: Position,
        mut alpha: i32,
        mut beta: i32,
        spec: SearchSpec,
    ) -> Result<Line, ()> {
""", """    pub(super) fn node(
        &mut self,
        position: Position,
        alpha: i32,
        beta: i32,
        spec: SearchSpec,
    ) -> Result<Line, ()> {
        let parent = self.diag_type;
        self.diag_type = if beta - alpha > 1 {
            0
        } else if parent == 1 {
            2
        } else {
            1
        };
        let r = self.node_typed(position, alpha, beta, spec);
        self.diag_type = parent;
        r
    }
    fn node_typed(
        &mut self,
        position: Position,
        mut alpha: i32,
        mut beta: i32,
        spec: SearchSpec,
    ) -> Result<Line, ()> {
""")
y = 'src/search/ybwc.rs'
edit(y, "            diag_key: position", "            diag_type: self.diag_type,\n            diag_key: position")
edit(y, "    diag_key: usize,\n}", "    diag_key: usize,\n    diag_type: u8,\n}")
edit(y, "                    context.stats.helper_jobs += 1;\n", "                    context.stats.helper_jobs += 1;\n                    context.diag_type = task_point.diag_type;\n")
# mirror every sp update into spt keyed by type
src = open(root + y).read()
import re
src = re.sub(r"SearchStats::diag_addn\(&mut self\.stats\.diag_sp, point\.diag_key \* 6( \+ [^,]+)?, ([^;]+)\);",
             lambda m: m.group(0) + f"\n            SearchStats::diag_addn(&mut self.stats.diag_spt, point.diag_type as usize * 6{m.group(1) or ''}, {m.group(2)});", src)
open(root + y, 'w').write(src)
edit(t, "            end_cache: EndgameCache::default(),\n            #[cfg", "            end_cache: EndgameCache::default(),\n            diag_type: 0,\n            #[cfg")
