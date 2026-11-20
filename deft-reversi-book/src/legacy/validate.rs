//! 旧入力の計算関係を当時の意味で検査する。現行の編集からは呼ばない。
use super::*;
use crate::{
    AnalysisProfile, GenerationRoot, GenerationRun, InterruptedAnalysis, catalog, legal_actions,
};
use std::collections::{BTreeMap, VecDeque};
#[derive(Default, Clone)]
pub(crate) struct ReadCatalog {
    pub profiles: BTreeMap<AnalysisIdentity, AnalysisProfile>,
    pub runs: Vec<GenerationRun>,
    pub derivations: BTreeMap<[u8; 32], Vec<[u8; 32]>>,
}
impl ReadCatalog {
    pub fn into_current(self) -> catalog::Catalog {
        catalog::Catalog {
            profiles: self.profiles,
            runs: self.runs,
        }
    }
    pub fn bytes(&self) -> u64 {
        // The decoder's allowance includes historical input lists until validation finishes.
        self.derivations
            .values()
            .map(|v| 96 + v.capacity() as u64 * 32)
            .sum::<u64>()
            + (self.profiles.len() * 256
                + self.runs.capacity() * std::mem::size_of::<GenerationRun>()) as u64
            + self
                .profiles
                .values()
                .map(|p| {
                    p.engine_version.as_ref().map_or(0, String::capacity)
                        + p.model_label.as_ref().map_or(0, String::capacity)
                        + p.build_label.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>() as u64
            + self
                .runs
                .iter()
                .map(|r| {
                    (r.engine_version.capacity()
                        + r.build_label.as_ref().map_or(0, String::capacity)
                        + r.roots.capacity() * std::mem::size_of::<GenerationRoot>()
                        + r.interrupted.capacity() * std::mem::size_of::<InterruptedAnalysis>()
                        + r.config.analysis.bytes()) as u64
                })
                .sum::<u64>()
    }
}
pub(crate) fn provenance_inputs(c: &ReadCatalog, value: Assessment) -> Result<Vec<[u8; 32]>> {
    Ok(match value.analysis() {
        ValueAnalysis::Derived {
            basis: Derivation::Maximum { inputs, .. },
            ..
        } => c
            .derivations
            .get(&inputs)
            .ok_or_else(|| invalid("missing derivation input list"))?
            .clone(),
        ValueAnalysis::Derived {
            basis: Derivation::Child { input, .. },
            ..
        } => vec![input],
        ValueAnalysis::Derived {
            basis: Derivation::Intersection { inputs, .. },
            ..
        } => inputs.to_vec(),
        _ => Vec::new(),
    })
}

pub(crate) fn legacy_summary(values: impl IntoIterator<Item = Assessment>) -> QualitySummary {
    let mut q = QualitySummary::empty();
    for v in values {
        let b = v.analysis().summary();
        q.families |= b.families;
        q.horizon = q.horizon.min(b.horizon);
        q.mid_depth = q.mid_depth.min(b.mid_depth);
        q.mid_sel = q.mid_sel.min(b.mid_sel);
        q.end_sel = q.end_sel.min(b.end_sel);
        q.legacy_depth = q.legacy_depth.min(b.legacy_depth);
        q.legacy_sel = q.legacy_sel.min(b.legacy_sel);
    }
    q
}
// 旧playの計算元を検査した後、現在の品質集計で説明を作り直す。
// 手動値の表示深さを旧探索の最低深さへ混ぜていた版も受け入れるが、旧検査は緩めない。
// 入力関係を子から処理するので、複数段の計算でも古い集計結果を引き継がない。
pub(crate) fn rebuild_legacy_play_quality(
    c: &ReadCatalog,
    values: &mut [Assessment],
) -> Result<bool> {
    if !values
        .iter()
        .any(|v| matches!(v.analysis(), ValueAnalysis::Derived { .. }))
    {
        return Ok(false);
    }
    let mut indexes = BTreeMap::new();
    for (i, v) in values.iter().enumerate() {
        indexes.insert(v.content_id(), i);
    }
    let mut remaining = vec![0; values.len()];
    let mut parents = vec![Vec::new(); values.len()];
    let mut ready = VecDeque::new();
    for (i, &v) in values.iter().enumerate() {
        let ids = provenance_inputs(c, v)?;
        remaining[i] = ids.len();
        if ids.is_empty() {
            ready.push_back(i);
        }
        for id in ids {
            let input = *indexes
                .get(&id)
                .ok_or_else(|| invalid("missing evidence input"))?;
            parents[input].push(i);
        }
    }
    let mut seen = 0;
    let mut changed = false;
    while let Some(i) = ready.pop_front() {
        seen += 1;
        let value = values[i];
        if let ValueAnalysis::Derived { basis, .. } = value.analysis() {
            let ids = provenance_inputs(c, value)?;
            let mut summary = QualitySummary::empty();
            for id in ids {
                summary.include(values[indexes[&id]].analysis().summary());
            }
            if let Derivation::Child { placement, .. } = basis {
                summary = summary.shifted(placement);
            }
            changed |= summary != value.analysis().summary();
            values[i] = Assessment::new(
                value.claim(),
                ValueAnalysis::Derived { basis, summary },
                value.identity(),
            )?;
        }
        for &parent in &parents[i] {
            remaining[parent] -= 1;
            if remaining[parent] == 0 {
                ready.push_back(parent);
            }
        }
    }
    if seen != values.len() {
        return Err(invalid("cyclic evidence inputs"));
    }
    Ok(changed)
}
// 旧形式の「どの評価から計算したか」という入力関係を、当時の規則で検査する。
// 先にValueStoreへ入れると旧入力IDが消えるため、必ず変換前に呼ぶ。
pub(crate) fn validate_catalog(c: &ReadCatalog, values: &[Assessment]) -> Result<()> {
    let historical = values.iter().any(|v| {
        matches!(
            v.analysis(),
            ValueAnalysis::Derived {
                basis: Derivation::Maximum { .. }
                    | Derivation::Child { .. }
                    | Derivation::Intersection { .. },
                ..
            }
        )
    });
    if !historical {
        if !c.derivations.is_empty() {
            return Err(invalid("unexpected historical derivations"));
        }
        for &v in values {
            v.validate()?;
            if let ValueAnalysis::Search { trace, .. } = v.analysis()
                && let Some(i) = trace.run
            {
                let run = c
                    .runs
                    .get(i as usize)
                    .ok_or_else(|| invalid("search run reference"))?;
                if run.identity != v.identity() || !c.profiles.contains_key(&v.identity()) {
                    return Err(invalid("search profile reference"));
                }
            }
        }
        return Ok(());
    }
    let map: BTreeMap<_, _> = values.iter().map(|&v| (v.content_id(), v)).collect();
    // Each derivation is checked from its immediate immutable inputs. Exact
    // content IDs bind every link; cycles are rejected by a separate walk.
    for &v in values {
        v.validate()?;
        let ids = provenance_inputs(c, v)?;
        let inputs: Vec<_> = ids
            .iter()
            .map(|id| {
                map.get(id)
                    .copied()
                    .ok_or_else(|| invalid("missing evidence input"))
            })
            .collect::<Result<_>>()?;
        match v.analysis() {
            ValueAnalysis::Search { trace, .. } => {
                if let Some(id) = trace.run {
                    let run = c
                        .runs
                        .get(id as usize)
                        .ok_or_else(|| invalid("search run reference"))?;
                    if run.identity != v.identity() || !c.profiles.contains_key(&v.identity()) {
                        return Err(invalid("search profile reference"));
                    }
                }
            }
            ValueAnalysis::Derived {
                basis: Derivation::Maximum { root, policy, .. },
                ..
            } => {
                let key = Key(root[0], root[1]);
                let actions = legal_actions(key.board());
                if inputs.len() != actions.len() || actions.is_empty() {
                    return Err(invalid("maximum coverage"));
                }
                for (&input, action) in inputs.iter().zip(actions) {
                    input.validate_candidate(key, action)?;
                }
                let policy = match policy {
                    0 => PropagationPolicy::Quality,
                    1 => PropagationPolicy::Book,
                    _ => return Err(invalid("derivation policy")),
                };
                let old_summary = legacy_summary(inputs.iter().copied());
                if combine_values(inputs.into_iter().map(Some), v.identity(), key, policy)
                    .0
                    .map(|a| (a.claim(), old_summary))
                    != Some((v.claim(), v.analysis().summary()))
                {
                    return Err(invalid("derived maximum"));
                }
            }
            ValueAnalysis::Derived {
                basis: Derivation::Child { placement, .. },
                ..
            } => {
                if !inputs[0].whole_position() || {
                    let a = inputs[0].child_to_parent(placement);
                    (a.claim(), a.analysis().summary()) != (v.claim(), v.analysis().summary())
                } {
                    return Err(invalid("derived child backup"));
                }
            }
            ValueAnalysis::Derived {
                basis: Derivation::Intersection { root, action, .. },
                summary,
            } => {
                let key = Key(root[0], root[1]);
                let action = action_from(action)?;
                for &input in &inputs {
                    input.validate_candidate(key, action)?;
                }
                let mut expected = inputs[0].analysis().summary();
                expected.include(inputs[1].analysis().summary());
                let a = inputs[0]
                    .claim()
                    .certificate()
                    .ok_or_else(|| invalid("unproven intersection"))?;
                let b = inputs[1]
                    .claim()
                    .certificate()
                    .ok_or_else(|| invalid("unproven intersection"))?;
                if v.claim() != ValueClaim::Certified(a.intersect(b)?) || summary != expected {
                    return Err(invalid("derived intersection"));
                }
            }
            _ => {}
        }
    }
    // Kahn's algorithm bounds both time and stack usage on untrusted files.
    let mut remaining: BTreeMap<_, usize> = BTreeMap::new();
    let mut parents: BTreeMap<_, Vec<[u8; 32]>> = BTreeMap::new();
    let mut ready = VecDeque::new();
    for &v in values {
        let id = v.content_id();
        let inputs = provenance_inputs(c, v)?;
        if inputs.is_empty() {
            ready.push_back(id);
        }
        remaining.insert(id, inputs.len());
        for input in inputs {
            parents.entry(input).or_default().push(id);
        }
    }
    let mut seen = 0;
    while let Some(id) = ready.pop_front() {
        seen += 1;
        if let Some(ps) = parents.get(&id) {
            for p in ps {
                let n = remaining.get_mut(p).unwrap();
                *n -= 1;
                if *n == 0 {
                    ready.push_back(*p);
                }
            }
        }
    }
    if seen != values.len() {
        return Err(invalid("cyclic evidence"));
    }
    Ok(())
}
// 一時的なAssessment一覧を現行の共有一覧へ変換する。values.rsが探索記録を分離して書く。
