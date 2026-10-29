#!/usr/bin/env python3
"""On top of apply_diag.py: ordering quality of the local endgame (7-13 empties).

Per empties e, `ord[e*8 + k]`: k=0 ordered nodes, 1 with a usable TT move,
2 beta cuts, 3 cuts on the first move, 4/5 subtree nodes with/without a TT
move, 6 first-move cuts with a TT move, 7 cuts with a TT move.
"""
import sys
root = sys.argv[1] + '/deft-reversi-engine/'
def edit(p, old, new):
    p = root + p
    s = open(p).read()
    assert s.count(old) == 1, (p, old[:70], s.count(old))
    open(p, 'w').write(s.replace(old, new))
edit('src/search/api.rs', "    pub diag_helper_nodes: u64,\n}", "    pub diag_helper_nodes: u64,\n    pub diag_ord: Vec<u64>,\n}")
edit('src/search/api.rs', "        self.diag_helper_nodes += other.diag_helper_nodes;\n",
     "        self.diag_helper_nodes += other.diag_helper_nodes;\n        diag_add(&mut self.diag_ord, &other.diag_ord);\n")
edit('src/search/api.rs', "    pub fn diag_bump(v: &mut Vec<u64>, i: usize) {",
"""    pub fn diag_addn(v: &mut Vec<u64>, i: usize, n: u64) {
        if v.len() <= i {
            v.resize(i + 1, 0);
        }
        v[i] += n;
    }
    pub fn diag_bump(v: &mut Vec<u64>, i: usize) {""")
edit('src/search.rs', 'local={:?} local_cut={:?}",', 'local={:?} local_cut={:?} ord={:?}",')
edit('src/search.rs', 's.diag_local, s.diag_local_cut\n', 's.diag_local, s.diag_local_cut, s.diag_ord\n')
o = 'src/search/endgame/ordered.rs'
edit(o, """    pub(super) fn ordered_endgame(
        &mut self,
        board: Board,
        mut alpha: i32,
        beta: i32,
        known_moves: Option<u64>,
        best_move: &mut u8,
    ) -> Result<i32, ()> {
""", """    pub(super) fn ordered_endgame(
        &mut self,
        board: Board,
        alpha: i32,
        beta: i32,
        known_moves: Option<u64>,
        best_move: &mut u8,
    ) -> Result<i32, ()> {
        let before = self.stats.nodes;
        let mut diag = (2u8, -1i32);
        let r = self.ordered_endgame_diag(board, alpha, beta, known_moves, best_move, &mut diag);
        if diag.0 != 2 && r.is_ok() {
            let e = board.empty_count() as usize * 8;
            let v = &mut self.stats.diag_ord;
            let tt = diag.0 == 1;
            crate::SearchStats::diag_addn(v, e, 1);
            crate::SearchStats::diag_addn(v, e + 1, u64::from(tt));
            crate::SearchStats::diag_addn(v, e + 2, u64::from(diag.1 >= 0));
            crate::SearchStats::diag_addn(v, e + 3, u64::from(diag.1 == 0));
            crate::SearchStats::diag_addn(v, if tt { e + 4 } else { e + 5 }, self.stats.nodes - before);
            crate::SearchStats::diag_addn(v, e + 6, u64::from(tt && diag.1 == 0));
            crate::SearchStats::diag_addn(v, e + 7, u64::from(tt && diag.1 >= 0));
        }
        r
    }
    fn ordered_endgame_diag(
        &mut self,
        board: Board,
        mut alpha: i32,
        beta: i32,
        known_moves: Option<u64>,
        best_move: &mut u8,
        diag: &mut (u8, i32),
    ) -> Result<i32, ()> {
""")
edit(o, """        let tt_move = *best_move;
        let mut best = -65;
""", """        let tt_move = *best_move;
        diag.0 = u8::from(tt_move < 64 && moves & (1u64 << tt_move) != 0);
        let mut searched = 0i32;
        let mut best = -65;
""")
edit(o, """            best = score;
            if score >= beta {
                self.stats.beta_cuts += 1;
                return Ok(score);
            }
        }""", """            best = score;
            if score >= beta {
                diag.1 = 0;
                self.stats.beta_cuts += 1;
                return Ok(score);
            }
            searched = 1;
        }""")
edit(o, """                *best_move = bit.trailing_zeros() as u8;
                self.stats.leaves += 1;
                return Ok(64);""", """                *best_move = bit.trailing_zeros() as u8;
                diag.1 = searched;
                self.stats.leaves += 1;
                return Ok(64);""")
edit(o, """                if lower >= beta {
                    *best_move = bit.trailing_zeros() as u8;
                    self.stats.tt_cuts += 1;""", """                if lower >= beta {
                    *best_move = bit.trailing_zeros() as u8;
                    diag.1 = searched;
                    self.stats.tt_cuts += 1;""")
edit(o, """                if score >= beta {
                    return Ok(score);
                }
                continue;""", """                if score >= beta {
                    diag.1 = searched;
                    return Ok(score);
                }
                searched += 1;
                continue;""")
edit(o, """            alpha = alpha.max(score);
            if alpha >= beta {
                self.stats.beta_cuts += 1;
                break;""", """            alpha = alpha.max(score);
            if alpha >= beta {
                diag.1 = searched + i as i32;
                self.stats.beta_cuts += 1;
                break;""")
