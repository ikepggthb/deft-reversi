//! Book グラフ全体を走査して統計情報を集計する診断用モジュール。
//!
//! 異なる手順から同じ局面へ合流する場合、局面数（`positions`）は重複を除いて 1 回だけ数え、
//! 進行ルート数（`root_lines`）は合流前の経路ごとに分けて数える。
//! グラフ全体を走査するためコストが大きく、対局中の 1 手ごとの参照からは呼ばず、明示的な診断用途で使う。
use super::*;
use std::collections::BTreeMap;

/// 深さ・選択率・完全読み別で集計した候補手の品質バケット。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityBucket {
    pub depth: u8,
    pub selectivity: Selectivity,
    pub solved: bool,
    pub candidates: u64,
    pub unregistered_candidates: u64,
}
/// 評価値の由来カテゴリ別の点評価・上下限件数バケット。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueKindBucket {
    pub kind: AnalysisCategory,
    pub points: u64,
    pub bounds: u64,
}
/// ワークスペース全体の診断統計情報。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceStats {
    pub positions: u64,
    /// 点評価および上限カバレッジによって局面値が確定している局面数。
    pub valued_positions: u64,
    pub complete_positions: u64,
    pub solved_positions: u64,
    /// 登録済みの出次数リンクを持たない局面数（終局局面を含む）。
    pub leaf_positions: u64,
    pub registered_moves: u64,
    /// 未登録の合法候補手数（終局局面は 0）。
    pub unregistered_candidates: u64,
    /// 点評価を持たない候補手数（上下限のみ既知の候補を含む）。
    pub unknown_candidates: u64,
    pub bounded_candidates: u64,
    pub unknown_unregistered_candidates: u64,
    pub direct_values_used: u64,
    pub child_values_used: u64,
    pub manual_values_used: u64,
    pub move_search_values_used: u64,
    pub inactive_manual_records: u64,
    /// 設定されたルート群（生成未設定時はグラフのルート群）からの登録済み進行経路数の合計。
    /// 葉局面は 1 ラインと数え、共有された子経路は流入パスごとに 1 回ずつ数える。`u64::MAX` で飽和する。
    pub root_lines: u64,
    pub lines_saturated: bool,
    /// 親到達深さの互換性ヒストグラム（探索モード自体は識別しない）。
    pub qualities: Vec<QualityBucket>,
    pub value_kinds: Vec<ValueKindBucket>,
    pub mixed_derivations: u64,
    pub final_score_values_used: u64,
}
impl BookEditor {
    /// 探索を行わずにグラフ全体を走査して統計を集計する。
    /// `O(positions)` の一時領域を確保するため、明示的な診断用途向け。
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
                    .opt(node.final_score)
                    .is_some_and(|v| v.is_solved()),
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
                    result.unregistered_candidates += 1;
                }
                let evidence = self.move_values_at(key, e);
                result.inactive_manual_records += u64::from(
                    self.values
                        .opt(e.manual())
                        .is_some_and(|v| v.identity() != self.identity),
                );
                match evidence.selected_from {
                    Some(SelectedFrom::Direct) => result.direct_values_used += 1,
                    Some(SelectedFrom::Child) => result.child_values_used += 1,
                    Some(SelectedFrom::Manual) => result.manual_values_used += 1,
                    Some(SelectedFrom::FinalScore) => result.final_score_values_used += 1,
                    Some(SelectedFrom::Search) => result.move_search_values_used += 1,
                    None => {}
                }
                if let Some(v) = evidence.selected {
                    let count = kinds.entry(v.source().category()).or_default();
                    if v.bound() == Bound::Point {
                        count.0 += 1;
                    } else {
                        count.1 += 1;
                    }
                    let q = v.conditions();
                    result.mixed_derivations +=
                        u64::from(v.is_calculated() && q.has_midgame() && q.has_endgame());
                }
                result.bounded_candidates +=
                    u64::from(evidence.selected.is_some_and(|v| v.bound() != Bound::Point));
                if let Some(v) = evidence.selected.filter(|v| v.bound() == Bound::Point) {
                    let entry = qualities
                        .entry((v.depth(), v.selectivity() as u8, v.is_solved()))
                        .or_default();
                    entry.0 += 1;
                    entry.1 += u64::from(!e.registered);
                } else {
                    result.unknown_candidates += 1;
                    result.unknown_unregistered_candidates += u64::from(!e.registered);
                }
            }
            if !linked {
                count = 1;
                result.leaf_positions += 1;
            }
            lines.insert(key, count);
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
                |((depth, sel, solved), (candidates, unregistered_candidates))| QualityBucket {
                    depth,
                    selectivity: Selectivity::ALL[sel as usize],
                    solved,
                    candidates,
                    unregistered_candidates,
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
