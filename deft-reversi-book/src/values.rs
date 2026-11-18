//! Shared, current records. Graph slots contain four-byte numbers, never copies
//! of search traces. The store owns the records; numbers do not escape its owner.
//!
//! 参照順はValueId→StoredEvaluation→StoredSource→CompletedSearch。
//! モデル等の識別値も別の一覧を参照する。Evaluationは読取・検査用に展開する型で、
//! 全候補がその構造体を一つずつ保持するわけではない。
use super::*;
use std::{collections::BTreeMap, num::NonZeroU32};

// 内部のValueIdは1始まりで、Option<ValueId>も4バイトに収まる。
// 対局用配列・ファイルの値参照は0始まり。変換はget/atとsnapshot/formatで行う。
pub(super) type ValueId = NonZeroU32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StoredEvaluation {
    score: Score,
    source: u32,
    profile: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum StoredSource {
    Search(u32, bool),
    Position([u64; 2], SearchConditions),
    Child([u64; 2], bool, SearchConditions),
    Move([u64; 2], u8, SearchConditions),
    Rules([u64; 2]),
    Manual(u8, Selectivity),
    Unknown(u8, Selectivity, legacy::ValueOrigin),
}

#[derive(Clone, Debug, Default)]
pub(super) struct ValueStore {
    values: Vec<StoredEvaluation>,
    sources: Vec<StoredSource>,
    searches: Vec<CompletedSearch>,
    profiles: Vec<AnalysisIdentity>,
    // 同じ内容を再登録した時に既存番号を探す補助索引。キーの複製分もbytesへ含む。
    // Bookの構築後は更新しないためread_onlyで全索引を解放する。
    value_index: BTreeMap<StoredEvaluation, ValueId>,
    source_index: BTreeMap<StoredSource, u32>,
    search_index: BTreeMap<CompletedSearch, u32>,
    profile_index: BTreeMap<AnalysisIdentity, u32>,
}

/// A borrowed value for normal play. Score/quality checks do not copy the full
/// search trace. Call source() or to_owned() when detailed records are needed.
#[derive(Clone, Copy)]
pub struct EvaluationRef<'a> {
    store: &'a ValueStore,
    index: u32,
}
impl std::fmt::Debug for EvaluationRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvaluationRef")
            .field("score", &self.result())
            .field("source", &self.source())
            .finish()
    }
}
impl EvaluationRef<'_> {
    #[inline]
    pub fn result(self) -> Score {
        self.store.values[self.index as usize].score
    }
    #[inline]
    /// 点評価なら点数、区間なら上限（なければ下限）。点の確定はresult/boundも確認する。
    pub fn score(self) -> i16 {
        self.result().value()
    }
    #[inline]
    pub fn bound(self) -> Bound {
        self.result().bound()
    }
    #[inline]
    pub fn identity(self) -> AnalysisIdentity {
        self.store.profiles[self.store.values[self.index as usize].profile as usize]
    }
    pub fn source(self) -> EvaluationSource {
        self.store
            .source(self.store.values[self.index as usize].source)
    }
    pub fn conditions(self) -> SearchConditions {
        self.source().conditions()
    }
    pub fn depth(self) -> u8 {
        self.conditions().depth()
    }
    pub fn selectivity(self) -> Selectivity {
        if self.result().final_bounds().is_some() {
            Selectivity::Full
        } else {
            self.conditions().minimum_selectivity()
        }
    }
    pub fn is_solved(self) -> bool {
        self.result().is_solved()
    }
    pub fn is_calculated(self) -> bool {
        matches!(
            self.source(),
            EvaluationSource::Position { .. }
                | EvaluationSource::Child { .. }
                | EvaluationSource::Move { .. }
        )
    }
    pub fn to_owned(self) -> Evaluation {
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
    // 検査済みのEvaluationを内容ごとに共有する。検索記録と「最大値／最良手／他手の上限」
    // は別の段階で共有するため、同じ探索の三つの使い方でCompletedSearchを複製しない。
    pub fn intern(&mut self, value: Evaluation) -> ValueId {
        let source = match value.source() {
            EvaluationSource::Search(trace) | EvaluationSource::SearchMove(trace) => {
                let n = *self.search_index.entry(trace).or_insert_with(|| {
                    let n = self.searches.len() as u32;
                    self.searches.push(trace);
                    n
                });
                StoredSource::Search(n, matches!(value.source(), EvaluationSource::SearchMove(_)))
            }
            EvaluationSource::Position { root, conditions } => {
                StoredSource::Position(root, conditions)
            }
            EvaluationSource::Child {
                root,
                placement,
                conditions,
            } => StoredSource::Child(root, placement, conditions),
            EvaluationSource::Move {
                root,
                action,
                conditions,
            } => StoredSource::Move(root, action_code(action), conditions),
            EvaluationSource::Terminal { root } => StoredSource::Rules(root),
            EvaluationSource::Manual { depth, selectivity } => {
                StoredSource::Manual(depth, selectivity)
            }
            EvaluationSource::Imported {
                depth,
                selectivity,
                recorded_origin,
            } => StoredSource::Unknown(depth, selectivity, recorded_origin),
        };
        let source = *self.source_index.entry(source).or_insert_with(|| {
            let n = self.sources.len() as u32;
            self.sources.push(source);
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
        let value = StoredEvaluation {
            score: value.result(),
            source,
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
    pub fn get(&self, id: ValueId) -> Evaluation {
        let v = self.values[id.get() as usize - 1];
        Evaluation::stored(
            v.score,
            self.source(v.source),
            self.profiles[v.profile as usize],
        )
    }
    fn source(&self, index: u32) -> EvaluationSource {
        match self.sources[index as usize] {
            StoredSource::Search(n, false) => EvaluationSource::Search(self.searches[n as usize]),
            StoredSource::Search(n, true) => {
                EvaluationSource::SearchMove(self.searches[n as usize])
            }
            StoredSource::Position(root, conditions) => {
                EvaluationSource::Position { root, conditions }
            }
            StoredSource::Child(root, placement, conditions) => EvaluationSource::Child {
                root,
                placement,
                conditions,
            },
            StoredSource::Move(root, action, conditions) => EvaluationSource::Move {
                root,
                action: action_from(action).expect("validated move"),
                conditions,
            },
            StoredSource::Rules(root) => EvaluationSource::Terminal { root },
            StoredSource::Manual(depth, selectivity) => {
                EvaluationSource::Manual { depth, selectivity }
            }
            StoredSource::Unknown(depth, selectivity, recorded_origin) => {
                EvaluationSource::Imported {
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
    pub fn at(&self, index: usize) -> Evaluation {
        self.get(ValueId::new(index as u32 + 1).unwrap())
    }
    pub fn iter(&self) -> impl Iterator<Item = Evaluation> + '_ {
        (0..self.values.len()).map(|i| self.at(i))
    }
    pub fn opt(&self, id: Option<ValueId>) -> Option<Evaluation> {
        id.map(|id| self.get(id))
    }
    pub fn put(&mut self, v: Option<Evaluation>) -> Option<ValueId> {
        v.map(|v| self.intern(v))
    }
    pub fn bytes(&self) -> u64 {
        (self.values.capacity() * std::mem::size_of::<StoredEvaluation>()
            + self.sources.capacity() * std::mem::size_of::<StoredSource>()
            + self.searches.capacity() * std::mem::size_of::<CompletedSearch>()
            + self.profiles.capacity() * 32
            + self.value_index.len() * (std::mem::size_of::<StoredEvaluation>() + 32)
            + self.source_index.len() * (std::mem::size_of::<StoredSource>() + 32)
            + self.search_index.len() * (std::mem::size_of::<CompletedSearch>() + 32)
            + self.profile_index.len() * 64) as u64
    }
    // 以後internしない所有者（Book）だけが呼ぶ。更新用の索引を作り直す処理ではない。
    pub fn read_only(&mut self) {
        self.value_index.clear();
        self.source_index.clear();
        self.search_index.clear();
        self.profile_index.clear();
        self.values.shrink_to_fit();
        self.sources.shrink_to_fit();
        self.searches.shrink_to_fit();
        self.profiles.shrink_to_fit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{EngineOptions, Goal, Model, PatternModel, SearchRequest};
    fn result() -> Evaluation {
        let b = Board::new().canonical();
        let mut e = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let r = e.search(b, &SearchRequest::new(Goal::Depth(2))).unwrap();
        Evaluation::from_search_result(
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
            v.best_move_value().unwrap(),
            v.other_moves_upper_bound().unwrap(),
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
        crate::legacy::ValueStore::from_values(store.iter().map(crate::legacy::Assessment::from))
            .0
            .write(&mut bytes)
            .unwrap();
        let decoded =
            crate::legacy::ValueStore::read(&mut bytes.as_slice(), ReadLimits::default()).unwrap();
        assert_eq!(
            decoded.iter().map(Evaluation::from).collect::<Vec<_>>(),
            input
        );
        // Last four bytes are the profile number, not a content hash.
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(
            crate::legacy::ValueStore::read(&mut bytes.as_slice(), ReadLimits::default()).is_err()
        );
    }
    #[test]
    fn shared_tables_reject_oversized_counts_and_truncation() {
        assert!(
            crate::legacy::ValueStore::read(
                &mut u32::MAX.to_le_bytes().as_slice(),
                ReadLimits::default()
            )
            .is_err()
        );
        let mut s = ValueStore::default();
        s.intern(result());
        let mut bytes = Vec::new();
        crate::legacy::ValueStore::from_values(s.iter().map(crate::legacy::Assessment::from))
            .0
            .write(&mut bytes)
            .unwrap();
        for end in 0..bytes.len() {
            assert!(
                crate::legacy::ValueStore::read(&mut &bytes[..end], ReadLimits::default()).is_err(),
                "cutoff {end}"
            );
        }
        assert!(
            crate::legacy::ValueStore::read(
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
