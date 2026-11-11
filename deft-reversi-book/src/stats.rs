//! Cold graph diagnostics. Counts paths with multiplicity across transpositions.
//!
//! 局面数は共有局面を一度、進行数root_linesは合流前の経路ごとに数える。
//! 全体を走査する診断処理であり、対局の一手ごとの参照からは呼ばない。
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityBucket {
    pub quality: AnalysisQuality,
    pub candidates: u64,
    pub frontier_candidates: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueKindBucket {
    pub kind: AnalysisCategory,
    pub points: u64,
    pub bounds: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceStats {
    pub positions: u64,
    /// A position value justified by points plus upper coverage.
    pub valued_positions: u64,
    pub complete_positions: u64,
    pub solved_positions: u64,
    /// Positions with no registered outgoing link, including terminal positions.
    pub leaf_positions: u64,
    pub registered_moves: u64,
    /// Unregistered legal candidates; a terminal position contributes zero.
    pub frontier_candidates: u64,
    /// Candidates without a point value, including known upper/lower bounds.
    pub unknown_candidates: u64,
    pub bounded_candidates: u64,
    pub unknown_frontier_candidates: u64,
    pub direct_values_used: u64,
    pub propagated_values_used: u64,
    pub manual_values_used: u64,
    pub frontier_values_used: u64,
    pub inactive_manual_records: u64,
    pub pending_candidates: u64,
    pub pending_positions: u64,
    /// Sum of registered path counts at configured roots, or graph roots when
    /// generation is unconfigured. A leaf contributes one line; shared child
    /// paths are counted once per incoming path. Saturates at u64::MAX.
    pub root_lines: u64,
    pub lines_saturated: bool,
    /// Compatibility histogram of parent reach; does not identify search mode.
    pub qualities: Vec<QualityBucket>,
    pub value_kinds: Vec<ValueKindBucket>,
    pub mixed_derivations: u64,
    pub certificate_values_used: u64,
}
impl BookEditor {
    /// Traverses the graph without searching. Allocates O(positions) temporary
    /// line-count storage, so it is intended for explicit diagnostics.
    pub fn stats(&self) -> WorkspaceStats {
        let mut result = WorkspaceStats {
            positions: self.nodes.len() as u64,
            ..Default::default()
        };
        let mut kinds: BTreeMap<AnalysisCategory, (u64, u64)> = BTreeMap::new();
        let mut qualities: BTreeMap<(u8, u8, bool), (u64, u64)> = BTreeMap::new();
        let mut keys: Vec<_> = self.nodes.keys().copied().map(editor::order).collect();
        keys.sort_unstable();
        let mut lines: BTreeMap<Key, u64> = BTreeMap::new();
        for (_, _, key) in keys {
            let node = &self.nodes[&key];
            result.valued_positions += u64::from(node.value.is_some());
            result.complete_positions += u64::from(node.complete);
            result.solved_positions += u64::from(
                self.values
                    .opt(node.proof)
                    .is_some_and(|v| v.quality().solved),
            );
            let mut count = 0u64;
            let mut linked = false;
            for e in &node.edges {
                if e.registered {
                    linked = true;
                    result.registered_moves += 1;
                    let child = e.child;
                    count = add_lines(count, lines[&child], &mut result.lines_saturated);
                } else {
                    result.frontier_candidates += 1;
                }
                let evidence = self.evidence(key, e);
                result.inactive_manual_records += u64::from(
                    self.values
                        .opt(e.manual())
                        .is_some_and(|v| v.identity() != self.identity),
                );
                match evidence.source {
                    Some(ValueSource::Direct) => result.direct_values_used += 1,
                    Some(ValueSource::Propagated) => result.propagated_values_used += 1,
                    Some(ValueSource::Manual) => result.manual_values_used += 1,
                    Some(ValueSource::Certificate) => result.certificate_values_used += 1,
                    Some(ValueSource::Frontier) => result.frontier_values_used += 1,
                    None => {}
                }
                if let Some(v) = evidence.effective {
                    let count = kinds.entry(v.analysis().category()).or_default();
                    if v.bound() == Bound::Exact {
                        count.0 += 1;
                    } else {
                        count.1 += 1;
                    }
                    let q = v.analysis().summary();
                    result.mixed_derivations += u64::from(
                        matches!(v.analysis(), ValueAnalysis::Derived { .. })
                            && q.has_midgame()
                            && q.has_endgame(),
                    );
                }
                result.bounded_candidates += u64::from(
                    evidence
                        .effective
                        .is_some_and(|v| v.bound() != Bound::Exact),
                );
                if let Some(v) = evidence.effective.filter(|v| v.bound() == Bound::Exact) {
                    let entry = qualities
                        .entry((
                            v.quality().depth,
                            v.quality().selectivity as u8,
                            v.quality().solved,
                        ))
                        .or_default();
                    entry.0 += 1;
                    entry.1 += u64::from(!e.registered);
                } else {
                    result.unknown_candidates += 1;
                    result.unknown_frontier_candidates += u64::from(!e.registered);
                }
            }
            if !linked {
                count = 1;
                result.leaf_positions += 1;
            }
            lines.insert(key, count);
            if let Some(p) = &node.pending {
                result.pending_positions += 1;
                result.pending_candidates += p.values.iter().filter(|v| v.is_some()).count() as u64;
            }
        }
        if let Some(generation) = &self.generation {
            for root in &generation.roots {
                result.root_lines = add_lines(
                    result.root_lines,
                    lines[&root.key],
                    &mut result.lines_saturated,
                );
            }
        } else {
            for (&key, &count) in &lines {
                if !self.parents.contains_key(&key) {
                    result.root_lines =
                        add_lines(result.root_lines, count, &mut result.lines_saturated);
                }
            }
        }
        result.value_kinds = kinds
            .into_iter()
            .map(|(kind, (points, bounds))| ValueKindBucket {
                kind,
                points,
                bounds,
            })
            .collect();
        result.qualities = qualities
            .into_iter()
            .map(
                |((depth, sel, solved), (candidates, frontier_candidates))| QualityBucket {
                    quality: AnalysisQuality {
                        depth,
                        selectivity: selectivity(sel).unwrap(),
                        solved,
                    },
                    candidates,
                    frontier_candidates,
                },
            )
            .collect();
        result
    }
}
fn add_lines(a: u64, b: u64, saturated: &mut bool) -> u64 {
    a.checked_add(b).unwrap_or_else(|| {
        *saturated = true;
        u64::MAX
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn line_count_overflow_is_reported() {
        let mut saturated = false;
        assert_eq!(add_lines(u64::MAX - 1, 1, &mut saturated), u64::MAX);
        assert!(!saturated);
        assert_eq!(add_lines(u64::MAX, 1, &mut saturated), u64::MAX);
        assert!(saturated);
    }
}
