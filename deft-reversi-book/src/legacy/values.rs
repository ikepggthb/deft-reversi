//! work v8 / play v4の共有テーブルのバイト表現。
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
pub(crate) struct ValueStore {
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
    pub fn at(&self, index: usize) -> Assessment {
        self.get(ValueId::new(index as u32 + 1).unwrap())
    }
    pub fn iter(&self) -> impl Iterator<Item = Assessment> + '_ {
        (0..self.values.len()).map(|i| self.at(i))
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
