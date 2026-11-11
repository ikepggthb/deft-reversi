//! Shared, current records. Graph slots contain four-byte numbers, never copies
//! of search traces. The store owns the records; numbers do not escape its owner.
//!
//! 参照順はValueId→StoredValue→StoredAnalysis→CompletedSearch。
//! モデル等の識別値も別の一覧を参照する。Assessmentは読取・検査用に展開する型で、
//! 全候補がその構造体を一つずつ保持するわけではない。
use super::*;
use std::{collections::BTreeMap, num::NonZeroU32};

// 内部のValueIdは1始まりで、Option<ValueId>も4バイトに収まる。
// 対局用配列・ファイルの値参照は0始まり。変換はget/atとsnapshot/formatで行う。
pub(super) type ValueId = NonZeroU32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StoredValue {
    claim: ValueClaim,
    analysis: u32,
    profile: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum StoredAnalysis {
    Search(u32, SearchProjection),
    Derived([u64; 2], u8, QualitySummary),
    Rules([u64; 2]),
    Manual(u8, Selectivity),
    Unknown(u8, Selectivity, ValueOrigin),
}

#[derive(Clone, Debug, Default)]
pub(super) struct ValueStore {
    values: Vec<StoredValue>,
    analyses: Vec<StoredAnalysis>,
    searches: Vec<CompletedSearch>,
    profiles: Vec<AnalysisIdentity>,
    // 同じ内容を再登録した時に既存番号を探す補助索引。キーの複製分もbytesへ含む。
    // Bookの構築後は更新しないためread_onlyで全索引を解放する。
    value_index: BTreeMap<StoredValue, ValueId>,
    analysis_index: BTreeMap<StoredAnalysis, u32>,
    search_index: BTreeMap<CompletedSearch, u32>,
    profile_index: BTreeMap<AnalysisIdentity, u32>,
}

/// A borrowed value for normal play. Score/quality checks do not copy the full
/// search trace. Call analysis() or to_owned() when detailed records are needed.
#[derive(Clone, Copy)]
pub struct EvaluationRef<'a> {
    store: &'a ValueStore,
    index: u32,
}
impl std::fmt::Debug for EvaluationRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvaluationRef")
            .field("claim", &self.claim())
            .field("analysis", &self.analysis())
            .finish()
    }
}
impl EvaluationRef<'_> {
    #[inline]
    pub fn claim(self) -> ValueClaim {
        self.store.values[self.index as usize].claim
    }
    #[inline]
    /// 点評価なら点数、区間なら上限（なければ下限）。点の確定はclaim/boundも確認する。
    pub fn score(self) -> i16 {
        match self.claim() {
            ValueClaim::Estimate { score, .. } => score,
            ValueClaim::Certified(v) => v.upper().or(v.lower()).unwrap(),
        }
    }
    #[inline]
    pub fn bound(self) -> Bound {
        match self.claim() {
            ValueClaim::Estimate { bound, .. } => match bound {
                EstimateBound::Point => Bound::Exact,
                EstimateBound::Lower => Bound::Lower,
                EstimateBound::Upper => Bound::Upper,
            },
            ValueClaim::Certified(v) => {
                if v.exact().is_some() {
                    Bound::Exact
                } else if v.upper().is_some() {
                    Bound::ProvenUpper
                } else {
                    Bound::ProvenLower
                }
            }
        }
    }
    #[inline]
    pub fn identity(self) -> AnalysisIdentity {
        self.store.profiles[self.store.values[self.index as usize].profile as usize]
    }
    pub fn analysis(self) -> ValueAnalysis {
        self.store
            .analysis(self.store.values[self.index as usize].analysis)
    }
    pub fn summary(self) -> QualitySummary {
        match self.store.analyses[self.store.values[self.index as usize].analysis as usize] {
            StoredAnalysis::Search(n, _) => QualitySummary::search(self.store.searches[n as usize]),
            StoredAnalysis::Derived(_, _, q) => q,
            StoredAnalysis::Rules(root) => QualitySummary {
                families: QualitySummary::RULES,
                horizon: (64 - (root[0] | root[1]).count_ones()) as u8,
                ..QualitySummary::empty()
            },
            StoredAnalysis::Manual(depth, sel) => QualitySummary {
                families: QualitySummary::MANUAL,
                horizon: depth,
                legacy_depth: depth,
                legacy_sel: sel as u8,
                ..QualitySummary::empty()
            },
            StoredAnalysis::Unknown(depth, sel, _) => QualitySummary::legacy(depth, sel),
        }
    }
    pub fn quality(self) -> AnalysisQuality {
        let q = self.summary();
        AnalysisQuality {
            depth: q.horizon,
            selectivity: if self.claim().certificate().is_some() {
                Selectivity::Full
            } else {
                q.minimum_selectivity()
            },
            solved: self
                .claim()
                .certificate()
                .is_some_and(|v| v.exact().is_some()),
        }
    }
    pub fn is_derived(self) -> bool {
        matches!(
            self.store.analyses[self.store.values[self.index as usize].analysis as usize],
            StoredAnalysis::Derived(..)
        )
    }
    pub fn to_owned(self) -> Assessment {
        self.store.at(self.index as usize)
    }
}
impl PartialEq for EvaluationRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        (*self).to_owned() == (*other).to_owned()
    }
}
impl Eq for EvaluationRef<'_> {}
impl ValueStore {
    // 検査済みのAssessmentを内容ごとに共有する。検索記録と「最大値／最良手／他手の上限」
    // は別の段階で共有するため、同じ探索の三つの使い方でCompletedSearchを複製しない。
    pub fn intern(&mut self, value: Assessment) -> ValueId {
        let analysis = match value.analysis() {
            ValueAnalysis::Search { trace, projection } => {
                let n = *self.search_index.entry(trace).or_insert_with(|| {
                    let n = self.searches.len() as u32;
                    self.searches.push(trace);
                    n
                });
                StoredAnalysis::Search(n, projection)
            }
            ValueAnalysis::Derived { basis, summary } => {
                let (root, kind) = match basis {
                    Derivation::Maximum { root, .. } => (root, 0),
                    Derivation::Child {
                        child, placement, ..
                    } => (child, 1 + u8::from(placement)),
                    Derivation::Intersection { root, action, .. } => (root, 3 + action),
                    Derivation::Current { root, subject } => (root, subject.code()),
                };
                StoredAnalysis::Derived(root, kind, summary)
            }
            ValueAnalysis::Rules { root } => StoredAnalysis::Rules(root),
            ValueAnalysis::Manual { depth, selectivity } => {
                StoredAnalysis::Manual(depth, selectivity)
            }
            ValueAnalysis::LegacyUnknown {
                depth,
                selectivity,
                recorded_origin,
            } => StoredAnalysis::Unknown(depth, selectivity, recorded_origin),
        };
        let analysis = *self.analysis_index.entry(analysis).or_insert_with(|| {
            let n = self.analyses.len() as u32;
            self.analyses.push(analysis);
            n
        });
        let profile = *self
            .profile_index
            .entry(value.identity())
            .or_insert_with(|| {
                let n = self.profiles.len() as u32;
                self.profiles.push(value.identity());
                n
            });
        let value = StoredValue {
            claim: value.claim(),
            analysis,
            profile,
        };
        *self.value_index.entry(value).or_insert_with(|| {
            let n =
                ValueId::new(u32::try_from(self.values.len() + 1).expect("bounded value store"))
                    .unwrap();
            self.values.push(value);
            n
        })
    }
    #[inline]
    pub fn get(&self, id: ValueId) -> Assessment {
        let v = self.values[id.get() as usize - 1];
        Assessment::stored(
            v.claim,
            self.analysis(v.analysis),
            self.profiles[v.profile as usize],
        )
    }
    fn analysis(&self, index: u32) -> ValueAnalysis {
        match self.analyses[index as usize] {
            StoredAnalysis::Search(n, projection) => ValueAnalysis::Search {
                trace: self.searches[n as usize],
                projection,
            },
            StoredAnalysis::Derived(root, kind, summary) => ValueAnalysis::Derived {
                basis: Derivation::Current {
                    root,
                    subject: CalculationSubject::from_code(kind).expect("validated subject"),
                },
                summary,
            },
            StoredAnalysis::Rules(root) => ValueAnalysis::Rules { root },
            StoredAnalysis::Manual(depth, selectivity) => {
                ValueAnalysis::Manual { depth, selectivity }
            }
            StoredAnalysis::Unknown(depth, selectivity, recorded_origin) => {
                ValueAnalysis::LegacyUnknown {
                    depth,
                    selectivity,
                    recorded_origin,
                }
            }
        }
    }
    #[inline]
    pub fn borrowed(&self, index: u32) -> EvaluationRef<'_> {
        EvaluationRef { store: self, index }
    }
    pub fn at(&self, index: usize) -> Assessment {
        self.get(ValueId::new(index as u32 + 1).unwrap())
    }
    pub fn iter(&self) -> impl Iterator<Item = Assessment> + '_ {
        (0..self.values.len()).map(|i| self.at(i))
    }
    pub fn opt(&self, id: Option<ValueId>) -> Option<Assessment> {
        id.map(|id| self.get(id))
    }
    pub fn put(&mut self, v: Option<Assessment>) -> Option<ValueId> {
        v.map(|v| self.intern(v))
    }
    pub fn bytes(&self) -> u64 {
        (self.values.capacity() * std::mem::size_of::<StoredValue>()
            + self.analyses.capacity() * std::mem::size_of::<StoredAnalysis>()
            + self.searches.capacity() * std::mem::size_of::<CompletedSearch>()
            + self.profiles.capacity() * 32
            + self.value_index.len() * (std::mem::size_of::<StoredValue>() + 32)
            + self.analysis_index.len() * (std::mem::size_of::<StoredAnalysis>() + 32)
            + self.search_index.len() * (std::mem::size_of::<CompletedSearch>() + 32)
            + self.profile_index.len() * 64) as u64
    }
    // 以後internしない所有者（Book）だけが呼ぶ。更新用の索引を作り直す処理ではない。
    pub fn read_only(&mut self) {
        self.value_index.clear();
        self.analysis_index.clear();
        self.search_index.clear();
        self.profile_index.clear();
        self.values.shrink_to_fit();
        self.analyses.shrink_to_fit();
        self.searches.shrink_to_fit();
        self.profiles.shrink_to_fit();
    }
    pub fn from_values(values: impl IntoIterator<Item = Assessment>) -> (Self, Vec<ValueId>) {
        let mut s = Self::default();
        let ids = values.into_iter().map(|v| s.intern(v)).collect();
        (s, ids)
    }
}

// The file tables mirror ownership: profiles and completed searches are written
// once, followed by small analysis descriptors and values referring to them.
impl ValueStore {
    pub fn write(&self, w: &mut dyn std::io::Write) -> Result<()> {
        use value_codec::*;
        let u32w = |w: &mut dyn std::io::Write, n: usize| -> Result<()> {
            w.write_all(
                &u32::try_from(n)
                    .map_err(|_| BookError::Limit("record table"))?
                    .to_le_bytes(),
            )?;
            Ok(())
        };
        u32w(w, self.profiles.len())?;
        for p in &self.profiles {
            w.write_all(p)?;
        }
        u32w(w, self.searches.len())?;
        for t in &self.searches {
            requestedw(w, t.requested)?;
            kindw(w, t.planned)?;
            kindw(w, t.completed)?;
            rootw(w, t.root)?;
            n64(w, t.moves)?;
            byte(w, t.best)?;
            scorew(w, t.result_score)?;
            w.write_all(&t.run.unwrap_or(u32::MAX).to_le_bytes())?;
            n64(w, t.nodes)?;
            n64(w, t.elapsed_micros)?;
        }
        u32w(w, self.analyses.len())?;
        for &a in &self.analyses {
            match a {
                StoredAnalysis::Search(n, p) => {
                    byte(w, 0)?;
                    w.write_all(&n.to_le_bytes())?;
                    byte(
                        w,
                        match p {
                            SearchProjection::Maximum => 0,
                            SearchProjection::BestMove => 1,
                            SearchProjection::OtherMoveUpper => 2,
                        },
                    )?;
                }
                StoredAnalysis::Derived(root, kind, q) => {
                    byte(w, 1)?;
                    rootw(w, root)?;
                    byte(w, kind)?;
                    summaryw(w, q)?;
                }
                StoredAnalysis::Rules(root) => {
                    byte(w, 2)?;
                    rootw(w, root)?;
                }
                StoredAnalysis::Manual(d, s) => {
                    byte(w, 3)?;
                    byte(w, d)?;
                    byte(w, s as u8)?;
                }
                StoredAnalysis::Unknown(d, s, o) => {
                    byte(w, 4)?;
                    byte(w, d)?;
                    byte(w, s as u8)?;
                    byte(w, originw(o))?;
                }
            }
        }
        n64(w, self.values.len() as u64)?;
        for v in &self.values {
            match v.claim {
                ValueClaim::Estimate { score, bound } => {
                    byte(w, 0)?;
                    scorew(w, score)?;
                    byte(
                        w,
                        match bound {
                            EstimateBound::Point => 0,
                            EstimateBound::Lower => 1,
                            EstimateBound::Upper => 2,
                        },
                    )?;
                }
                ValueClaim::Certified(i) => {
                    byte(w, 1)?;
                    scorew(w, i.lower().unwrap_or(127))?;
                    scorew(w, i.upper().unwrap_or(127))?;
                }
            }
            w.write_all(&v.analysis.to_le_bytes())?;
            w.write_all(&v.profile.to_le_bytes())?;
        }
        Ok(())
    }
    pub fn read(r: &mut impl std::io::Read, limits: ReadLimits) -> Result<Self> {
        use value_codec::*;
        fn u32r(r: &mut impl std::io::Read) -> Result<u32> {
            let mut b = [0; 4];
            r.read_exact(&mut b)?;
            Ok(u32::from_le_bytes(b))
        }
        fn count(r: &mut impl std::io::Read, left: &mut u64, width: u64) -> Result<usize> {
            let n = u32r(r)? as u64;
            let bytes = n.checked_mul(width).ok_or(BookError::Limit("table size"))?;
            *left = left
                .checked_sub(bytes)
                .ok_or(BookError::Limit("table allocation"))?;
            Ok(n as usize)
        }
        let mut out = Self::default();
        let mut left = limits.max_bytes;
        let n = count(r, &mut left, 64)?;
        if n > 65536 {
            return Err(BookError::Limit("profile count"));
        }
        out.profiles
            .try_reserve_exact(n)
            .map_err(|_| BookError::Limit("profile allocation"))?;
        for _ in 0..n {
            out.profiles.push(idr(r)?);
        }
        let n = count(r, &mut left, std::mem::size_of::<CompletedSearch>() as u64)?;
        out.searches
            .try_reserve_exact(n)
            .map_err(|_| BookError::Limit("search allocation"))?;
        for _ in 0..n {
            let requested = requestedr(r)?;
            let planned = kindr(r)?;
            let completed = kindr(r)?;
            let root = rootr(r)?;
            let moves = r64(r)?;
            let best = read_byte(r)?;
            let result_score = scorer(r)?;
            let run = u32r(r)?;
            let t = CompletedSearch {
                requested,
                planned,
                completed,
                root,
                moves,
                best,
                result_score,
                run: (run != u32::MAX).then_some(run),
                nodes: r64(r)?,
                elapsed_micros: r64(r)?,
            };
            t.validate()?;
            out.searches.push(t);
        }
        let n = count(r, &mut left, std::mem::size_of::<StoredAnalysis>() as u64)?;
        out.analyses
            .try_reserve_exact(n)
            .map_err(|_| BookError::Limit("analysis allocation"))?;
        for _ in 0..n {
            let a = match read_byte(r)? {
                0 => {
                    let id = u32r(r)?;
                    if id as usize >= out.searches.len() {
                        return Err(invalid("search reference"));
                    }
                    let p = match read_byte(r)? {
                        0 => SearchProjection::Maximum,
                        1 => SearchProjection::BestMove,
                        2 => SearchProjection::OtherMoveUpper,
                        _ => return Err(invalid("search projection")),
                    };
                    StoredAnalysis::Search(id, p)
                }
                1 => {
                    let root = rootr(r)?;
                    let kind = read_byte(r)?;
                    CalculationSubject::from_code(kind)?;
                    StoredAnalysis::Derived(root, kind, summaryr(r)?)
                }
                2 => StoredAnalysis::Rules(rootr(r)?),
                3 => StoredAnalysis::Manual(read_byte(r)?, selectivity(read_byte(r)?)?),
                4 => StoredAnalysis::Unknown(
                    read_byte(r)?,
                    selectivity(read_byte(r)?)?,
                    originr(read_byte(r)?)?,
                ),
                _ => return Err(invalid("analysis kind")),
            };
            out.analyses.push(a);
        }
        for i in 0..out.analyses.len() {
            out.analysis(i as u32).validate()?;
        }
        let n = r64(r)?;
        let cap = limits
            .max_candidates
            .saturating_mul(5)
            .saturating_add(limits.max_positions.saturating_mul(3));
        if n > cap || n > left / (3 * std::mem::size_of::<Assessment>() as u64 + 256) {
            return Err(BookError::Limit("value allocation"));
        }
        out.values
            .try_reserve_exact(n as usize)
            .map_err(|_| BookError::Limit("value allocation"))?;
        for _ in 0..n {
            let claim = match read_byte(r)? {
                0 => {
                    let score = scorer(r)?;
                    let bound = match read_byte(r)? {
                        0 => EstimateBound::Point,
                        1 => EstimateBound::Lower,
                        2 => EstimateBound::Upper,
                        _ => return Err(invalid("estimate bound")),
                    };
                    ValueClaim::Estimate { score, bound }
                }
                1 => {
                    let l = scorer(r)?;
                    let u = scorer(r)?;
                    ValueClaim::Certified(ScoreInterval::new(
                        (l != 127).then_some(l),
                        (u != 127).then_some(u),
                    )?)
                }
                _ => return Err(invalid("value kind")),
            };
            let analysis = u32r(r)?;
            let profile = u32r(r)?;
            if analysis as usize >= out.analyses.len() || profile as usize >= out.profiles.len() {
                return Err(invalid("value reference"));
            }
            out.values.push(StoredValue {
                claim,
                analysis,
                profile,
            });
            let v = out.at(out.values.len() - 1);
            Assessment::new(v.claim(), v.analysis(), v.identity())?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{EngineOptions, Goal, Model, PatternModel, SearchRequest};
    fn result() -> Assessment {
        let b = Board::new().canonical();
        let mut e = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let r = e.search(b, &SearchRequest::new(Goal::Depth(2))).unwrap();
        Assessment::from_search_result(
            b,
            RequestedAnalysis::External,
            SearchKind::Midgame {
                depth: 2,
                selectivity: Selectivity::Full,
            },
            &r,
            analysis_identity(&e).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn projections_share_one_completed_search_and_survive_the_shared_codec() {
        let v = result();
        let mut store = ValueStore::default();
        let input = [
            v,
            v.project(SearchProjection::BestMove).unwrap(),
            v.project(SearchProjection::OtherMoveUpper).unwrap(),
        ];
        let ids = input.map(|v| store.intern(v));
        assert_eq!(store.searches.len(), 1);
        assert_eq!(store.profiles.len(), 1);
        assert_eq!(store.values.len(), 3);
        assert_eq!(std::mem::size_of::<Option<ValueId>>(), 4);
        for (id, v) in ids.into_iter().zip(input) {
            assert_eq!(store.get(id), v);
        }
        let mut bytes = Vec::new();
        store.write(&mut bytes).unwrap();
        let decoded = ValueStore::read(&mut bytes.as_slice(), ReadLimits::default()).unwrap();
        assert_eq!(decoded.searches.len(), 1);
        assert_eq!(decoded.iter().collect::<Vec<_>>(), input);
        // Last four bytes are the profile number, not a content hash.
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(ValueStore::read(&mut bytes.as_slice(), ReadLimits::default()).is_err());
    }
    #[test]
    fn shared_tables_reject_oversized_counts_and_truncation() {
        assert!(
            ValueStore::read(
                &mut u32::MAX.to_le_bytes().as_slice(),
                ReadLimits::default()
            )
            .is_err()
        );
        let mut s = ValueStore::default();
        s.intern(result());
        let mut bytes = Vec::new();
        s.write(&mut bytes).unwrap();
        for end in 0..bytes.len() {
            assert!(
                ValueStore::read(&mut &bytes[..end], ReadLimits::default()).is_err(),
                "cutoff {end}"
            );
        }
        assert!(
            ValueStore::read(
                &mut bytes.as_slice(),
                ReadLimits {
                    max_bytes: 64,
                    ..ReadLimits::default()
                }
            )
            .is_err()
        );
    }
}
