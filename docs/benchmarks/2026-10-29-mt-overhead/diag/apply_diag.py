#!/usr/bin/env python3
"""Apply the node-count diagnostics by text replacement (works on top of trial patches)."""
import sys
root = sys.argv[1] + '/deft-reversi-engine/'
def edit(p, old, new):
    p = root + p
    s = open(p).read()
    assert s.count(old) == 1, (p, old[:70], s.count(old))
    open(p, 'w').write(s.replace(old, new))
edit('src/search/api.rs', "    pub stability_cuts: u64,\n}\nimpl SearchStats {",
"""    pub stability_cuts: u64,
    pub diag_general: Vec<u64>,
    pub diag_local: Vec<u64>,
    pub diag_local_cut: Vec<u64>,
    pub diag_split_empties: Vec<u64>,
    pub diag_split_cut: u64,
    pub diag_wasted: u64,
    pub diag_helper_nodes: u64,
}
fn diag_add(a: &mut Vec<u64>, b: &[u64]) {
    if a.len() < b.len() {
        a.resize(b.len(), 0);
    }
    for (x, y) in a.iter_mut().zip(b) {
        *x += y;
    }
}
impl SearchStats {
    pub fn diag_bump(v: &mut Vec<u64>, i: usize) {
        if v.len() <= i {
            v.resize(i + 1, 0);
        }
        v[i] += 1;
    }""")
edit('src/search/api.rs', "        self.stability_cuts += other.stability_cuts;\n",
"""        self.stability_cuts += other.stability_cuts;
        diag_add(&mut self.diag_general, &other.diag_general);
        diag_add(&mut self.diag_local, &other.diag_local);
        diag_add(&mut self.diag_local_cut, &other.diag_local_cut);
        diag_add(&mut self.diag_split_empties, &other.diag_split_empties);
        self.diag_split_cut += other.diag_split_cut;
        self.diag_wasted += other.diag_wasted;
        self.diag_helper_nodes += other.diag_helper_nodes;
""")
edit('src/search/tree.rs', "        self.enter()?;\n        if exact && beta - alpha == 1 && board.empty_count() <= super::endgame::LOCAL_END_MAX {",
"        self.enter()?;\n        SearchStats::diag_bump(&mut self.stats.diag_general, board.empty_count() as usize);\n        if exact && beta - alpha == 1 && board.empty_count() <= super::endgame::LOCAL_END_MAX {")
edit('src/search/endgame/mod.rs', "        let probe = known_probe.unwrap_or_else(|| self.end_cache.probe(board, quality));\n",
"        let probe = known_probe.unwrap_or_else(|| self.end_cache.probe(board, quality));\n        crate::SearchStats::diag_bump(&mut self.stats.diag_local, empties as usize);\n        if probe.matched && (i32::from(probe.lower) >= beta || i32::from(probe.upper) <= alpha) {\n            crate::SearchStats::diag_bump(&mut self.stats.diag_local_cut, empties as usize);\n        }\n")
edit('src/search/ybwc.rs', "        if spawned != 0 {\n            self.stats.splits += 1;\n        }",
"        if spawned != 0 {\n            self.stats.splits += 1;\n            SearchStats::diag_bump(&mut self.stats.diag_split_empties, position.board.empty_count() as usize);\n        }")
edit('src/search/ybwc.rs', "        if self.aborted() {\n            return Err(());\n        }\n        let result = parallel::lock(&point.state).best.clone();",
"        if point.cut.load(Ordering::Acquire) {\n            self.stats.diag_split_cut += 1;\n        }\n        if self.aborted() {\n            return Err(());\n        }\n        let result = parallel::lock(&point.state).best.clone();")
edit('src/search/ybwc.rs', "                    context.split_loop(&task_point);\n",
"                    context.split_loop(&task_point);\n                    context.stats.diag_helper_nodes += context.stats.nodes;\n")
edit('src/search/ybwc.rs', "            let Ok(Some(line)) = self.sibling(", "            let before = self.stats.nodes;\n            let searched = self.sibling(")
edit('src/search/ybwc.rs', """            ) else {
                break;
            };
            if let Some(bounds) = &point.root_bounds {""", """            );
            let Ok(Some(line)) = searched else {
                self.stats.diag_wasted += self.stats.nodes - before;
                break;
            };
            if point.cut.load(Ordering::Acquire) {
                self.stats.diag_wasted += self.stats.nodes - before;
            }
            if let Some(bounds) = &point.root_bounds {""")
edit('src/search.rs', "            let Some(line) = completed else { break };",
"""            {
                let mut s = parallel::lock(&runtime.stats).clone();
                s.merge(&context.stats);
                eprintln!(
                    "DIAG depth={} final={} quality={} nodes={} helper_nodes={} wasted={} splits={} split_cut={} split_empties={:?} general={:?} local={:?} local_cut={:?}",
                    depth, final_iteration, quality, s.nodes, s.diag_helper_nodes, s.diag_wasted, s.splits, s.diag_split_cut, s.diag_split_empties, s.diag_general, s.diag_local, s.diag_local_cut
                );
            }
            let Some(line) = completed else { break };""")
if len(sys.argv) > 2 and sys.argv[2] == 'cutindex':
    edit('src/search/api.rs', "    pub diag_wasted: u64,\n", "    pub diag_wasted: u64,\n    pub diag_cut_index: Vec<u64>,\n    pub diag_cut_taken: Vec<u64>,\n")
    edit('src/search/api.rs', "        self.diag_wasted += other.diag_wasted;\n", "        self.diag_wasted += other.diag_wasted;\n        diag_add(&mut self.diag_cut_index, &other.diag_cut_index);\n        diag_add(&mut self.diag_cut_taken, &other.diag_cut_taken);\n")
    edit('src/search/ybwc.rs', """            let Some((candidate, alpha)) = next else {
                break;
            };""", """            let Some((candidate, alpha)) = next else {
                break;
            };
            let my_index = parallel::lock(&point.state).next - 1;""")
    edit('src/search/ybwc.rs', """            if state.alpha >= point.beta {
                point.cut.store(true, Ordering::Release);""", """            if state.alpha >= point.beta {
                if !point.cut.load(Ordering::Relaxed) {
                    SearchStats::diag_bump(&mut self.stats.diag_cut_index, my_index);
                    SearchStats::diag_bump(&mut self.stats.diag_cut_taken, state.next - my_index - 1);
                }
                point.cut.store(true, Ordering::Release);""")
    edit('src/search.rs', 'local={:?} local_cut={:?}",', 'local={:?} local_cut={:?} cut_index={:?} cut_taken={:?}",')
    edit('src/search.rs', 's.diag_local, s.diag_local_cut\n', 's.diag_local, s.diag_local_cut, s.diag_cut_index, s.diag_cut_taken\n')
