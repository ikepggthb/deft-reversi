#!/usr/bin/env python3
"""On top of apply_diag.py and apply_order_diag.py: null-window exact general
nodes (14+ empties) and local child probes.

`gen[e*10 + k]`: k=0 nodes that reached move ordering, 1 with a usable TT move,
2 fail-highs, 3 fail-highs on the first move, 4 this thread's subtree nodes,
5 first-move fail-highs with a TT move, 6 all entries, 7 fail-highs with a TT move,
8 fail-highs by a child's main-TT bound (ETC) before any search.
`ordc[e*4 + k]` (local parent with e empties): k=0 child probes, 1 matched,
2 child lower >= beta (parent cut), 3 child upper <= alpha (move skipped).
"""
import sys
root = sys.argv[1] + '/deft-reversi-engine/'
def edit(p, old, new):
    p = root + p
    s = open(p).read()
    assert s.count(old) == 1, (p, old[:70], s.count(old))
    open(p, 'w').write(s.replace(old, new))
edit('src/search/api.rs', "    pub diag_ord: Vec<u64>,\n", "    pub diag_ord: Vec<u64>,\n    pub diag_gen: Vec<u64>,\n    pub diag_ordc: Vec<u64>,\n")
edit('src/search/api.rs', "        diag_add(&mut self.diag_ord, &other.diag_ord);\n",
     "        diag_add(&mut self.diag_ord, &other.diag_ord);\n        diag_add(&mut self.diag_gen, &other.diag_gen);\n        diag_add(&mut self.diag_ordc, &other.diag_ordc);\n")
edit('src/search.rs', ' ord={:?}",', ' ord={:?} gen={:?} ordc={:?}",')
edit('src/search.rs', 's.diag_ord\n', 's.diag_ord, s.diag_gen, s.diag_ordc\n')
o = 'src/search/endgame/ordered.rs'
edit(o, """            let probe = self.end_cache.probe(child, quality);
""", """            let probe = self.end_cache.probe(child, quality);
            {
                let e = board.empty_count() as usize * 4;
                let v = &mut self.stats.diag_ordc;
                crate::SearchStats::diag_addn(v, e, 1);
                crate::SearchStats::diag_addn(v, e + 1, u64::from(probe.matched));
                crate::SearchStats::diag_addn(v, e + 2, u64::from(probe.matched && -i32::from(probe.upper) >= beta));
                crate::SearchStats::diag_addn(v, e + 3, u64::from(probe.matched && -i32::from(probe.upper) < beta && -i32::from(probe.lower) <= alpha));
            }
""")
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
        let before = self.stats.nodes;
        let mut diag = (0u8, 0u8, 0u8);
        let r = self.node_diag(position, alpha, beta, spec, &mut diag);
        let e = position.board.empty_count() as usize;
        if spec.exact && beta - alpha == 1 && e > 13 && let Ok(line) = &r {
            let v = &mut self.stats.diag_gen;
            let b = e * 10;
            let cut = line.score >= beta;
            crate::SearchStats::diag_addn(v, b + 6, 1);
            if diag.0 == 1 {
                let tt = diag.1 == 1;
                let first = diag.2 == 1;
                crate::SearchStats::diag_addn(v, b, 1);
                crate::SearchStats::diag_addn(v, b + 1, u64::from(tt));
                crate::SearchStats::diag_addn(v, b + 2, u64::from(cut));
                crate::SearchStats::diag_addn(v, b + 3, u64::from(cut && diag.2 == 1));
                crate::SearchStats::diag_addn(v, b + 4, self.stats.nodes - before);
                crate::SearchStats::diag_addn(v, b + 5, u64::from(tt && cut && first));
                crate::SearchStats::diag_addn(v, b + 7, u64::from(tt && cut));
                crate::SearchStats::diag_addn(v, b + 8, u64::from(diag.2 == 2));
            }
        }
        r
    }
    fn node_diag(
        &mut self,
        position: Position,
        mut alpha: i32,
        mut beta: i32,
        spec: SearchSpec,
        diag: &mut (u8, u8, u8),
    ) -> Result<Line, ()> {
""")
edit(t, """        let tt_move = cached.map(|e| e.best).unwrap_or(64);
""", """        let tt_move = cached.map(|e| e.best).unwrap_or(64);
        diag.0 = 1;
""")
edit(t, """            let candidate = candidates[index];
            candidates.copy_within(index + 1..count, index);
            count -= 1;
""", """            let candidate = candidates[index];
            diag.1 = 1;
            candidates.copy_within(index + 1..count, index);
            count -= 1;
""")
edit(t, """            alpha = alpha.max(line.score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                if self.aborted() {""", """            alpha = alpha.max(line.score);
            if alpha >= beta {
                diag.2 = 1;
                self.stats.beta_cuts += 1;
                if self.aborted() {""")
edit(t, """                if lower >= beta {
                    self.stats.tt_cuts += 1;
                    return Ok(Line::leaf(lower));
                }
                if upper <= original_alpha {""", """                if lower >= beta {
                    diag.2 = 2;
                    self.stats.tt_cuts += 1;
                    return Ok(Line::leaf(lower));
                }
                if upper <= original_alpha {""")
edit(t, """        alpha = alpha.max(best.score);
        if alpha < beta && count > start {""", """        alpha = alpha.max(best.score);
        if alpha >= beta {
            diag.2 = 1;
        }
        if alpha < beta && count > start {""")
