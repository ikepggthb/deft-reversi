//! 対局用Bookへの変換と、盤面からの読取参照。
//!
//! 局面一覧と候補一覧を連続配列へ置き、Record.start/lenで対応付ける。
//! 編集用の親一覧・仕事一覧は持たない。通常の参照はprobe→BookEntry::moves。
//! 保存形式の一時一覧を経由せず、現在使うEvaluationを共有一覧へ移す。
use super::*;
#[derive(Clone, Copy, Debug)]
pub(super) struct StoredCandidate {
    pub action: Action,
    pub registered: bool,
    pub value: Option<u32>,
    pub final_score: Option<u32>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Record {
    pub key: Key,
    pub start: usize,
    pub len: usize,
    pub complete: bool,
    pub value: Option<u32>,
    pub final_score: Option<u32>,
}
pub(super) struct SnapshotMetadata {
    pub identity: AnalysisIdentity,
    pub catalog: catalog::Catalog,
    pub producer: String,
}
/// Immutable, compact play snapshot. No mutable graph, reverse links, or jobs.
pub struct Book {
    pub(super) identity: AnalysisIdentity,
    pub(super) catalog: catalog::Catalog,
    pub(super) values: ValueStore,
    pub(super) producer: String,
    pub(super) extensions: Vec<Extension>,
    pub(super) records: Vec<Record>,
    pub(super) candidates: Vec<StoredCandidate>,
    index: Vec<usize>,
}
#[derive(Clone, Copy, Debug)]
pub struct BookStats {
    pub positions: usize,
    pub candidates: usize,
    pub registered_moves: usize,
    pub solved_positions: usize,
    pub resident_bytes: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct BookCandidate {
    pub action: Action,
    pub registered: bool,
    pub value: Option<Evaluation>,
    pub final_score: Option<Evaluation>,
}
/// Normal play view: detailed search metadata remains in the shared store.
#[derive(Clone, Copy, Debug)]
pub struct BookMove<'a> {
    pub action: Action,
    pub registered: bool,
    pub value: Option<EvaluationRef<'a>>,
    pub final_score: Option<EvaluationRef<'a>>,
}
/// Borrowed lookup view; iteration restores coordinates without allocation.
pub struct BookEntry<'a> {
    book: &'a Book,
    record: &'a Record,
    transform: Symmetry,
}
impl<'a> BookEntry<'a> {
    /// 通常の対局向け。着手は問い合わせ時の座標へ戻すが、評価内の盤面は正規化座標。
    /// 評価は借用参照なので、各手の詳しい探索記録をコピーせずに点数・品質を読める。
    pub fn moves(&self) -> impl Iterator<Item = BookMove<'a>> + '_ {
        self.book.candidates[self.record.start..self.record.start + self.record.len]
            .iter()
            .map(|c| BookMove {
                action: self.transform.inverse().action(c.action),
                registered: c.registered,
                value: c.value.map(|v| self.book.values.borrowed(v)),
                final_score: c.final_score.map(|v| self.book.values.borrowed(v)),
            })
    }
    /// 診断向け。各手の評価を独立したEvaluationへ展開する。
    pub fn candidates(&self) -> impl Iterator<Item = BookCandidate> + '_ {
        self.book.candidates[self.record.start..self.record.start + self.record.len]
            .iter()
            .map(|c| BookCandidate {
                action: self.transform.inverse().action(c.action),
                registered: c.registered,
                value: c.value.map(|v| self.book.values.at(v as usize)),
                final_score: c.final_score.map(|v| self.book.values.at(v as usize)),
            })
    }
    /// 全合法手が点評価を持つか。最良手と他手の上限だけで局面値が分かってもfalse。
    pub fn all_moves_evaluated(&self) -> bool {
        self.record.complete
    }
    pub fn value(&self) -> Option<Evaluation> {
        self.record.value.map(|v| self.book.values.at(v as usize))
    }
    pub fn final_score(&self) -> Option<Evaluation> {
        self.record
            .final_score
            .map(|v| self.book.values.at(v as usize))
    }
    pub fn is_terminal(&self) -> bool {
        self.record.len == 0
    }
}
impl Book {
    pub(super) fn from_editor(editor: &BookEditor) -> Result<Self> {
        editor.validate()?;
        // 編集APIで更新済みの採用値を集める。未採用の直接値や生成の仕事は対局用へ載せない。
        let mut values = ValueStore::default();
        let mut remap = std::collections::BTreeMap::new();
        let mut copy =
            |v: Evaluation| *remap.entry(v).or_insert_with(|| values.intern(v).get() - 1);
        let mut records = Vec::new();
        let mut candidates = Vec::new();
        records
            .try_reserve_exact(editor.len())
            .map_err(|_| BookError::Limit("snapshot allocation"))?;
        for (&key, node) in &editor.nodes {
            let start = candidates.len();
            candidates
                .try_reserve(node.edges.len())
                .map_err(|_| BookError::Limit("candidate allocation"))?;
            for edge in &node.edges {
                candidates.push(StoredCandidate {
                    action: edge.action,
                    registered: edge.registered,
                    value: editor.selected(key, edge).map(&mut copy),
                    final_score: edge.final_score().map(|v| copy(editor.values.get(v))),
                });
            }
            records.push(Record {
                key,
                start,
                len: node.edges.len(),
                complete: node.complete,
                value: node.value.map(|v| copy(editor.values.get(v))),
                final_score: node.final_score.map(|v| copy(editor.values.get(v))),
            });
        }
        Self::from_parts(
            SnapshotMetadata {
                identity: editor.identity,
                catalog: editor.catalog.clone(),
                producer: editor.producer.clone(),
            },
            values.iter().collect(),
            editor.extensions.clone(),
            records,
            candidates,
        )
    }
    pub(super) fn from_parts(
        metadata: SnapshotMetadata,
        values: Vec<Evaluation>,
        extensions: Vec<Extension>,
        mut records: Vec<Record>,
        mut candidates: Vec<StoredCandidate>,
    ) -> Result<Self> {
        let SnapshotMetadata {
            identity,
            catalog,
            producer,
        } = metadata;
        // 参照される値だけを共有一覧へ移す。構築用の旧番号を全て付け替えてから索引を捨てる。
        let original = values;
        let mut values = ValueStore::default();
        let mut remap = vec![None; original.len()];
        let mut map = |i: u32| {
            let id = *remap[i as usize].get_or_insert_with(|| values.intern(original[i as usize]));
            id.get() - 1
        };
        for r in &mut records {
            r.value = r.value.map(&mut map);
            r.final_score = r.final_score.map(&mut map);
        }
        for c in &mut candidates {
            c.value = c.value.map(&mut map);
            c.final_score = c.final_score.map(&mut map);
        }
        values.read_only();
        let requested = records
            .len()
            .checked_mul(10)
            .and_then(|n| n.checked_div(7))
            .and_then(|n| n.checked_add(1))
            .ok_or(BookError::Limit("index size"))?;
        let capacity = requested
            .checked_next_power_of_two()
            .ok_or(BookError::Limit("index size"))?
            .max(2);
        let mut index = Vec::new();
        index
            .try_reserve_exact(capacity)
            .map_err(|_| BookError::Limit("index allocation"))?;
        index.resize(capacity, usize::MAX);
        for (i, r) in records.iter().enumerate() {
            let mut slot = (r.key.hash() >> (64 - capacity.trailing_zeros())) as usize;
            while index[slot] != usize::MAX {
                slot = (slot + 1) & (capacity - 1);
            }
            index[slot] = i;
        }
        Ok(Self {
            identity,
            catalog,
            values,
            producer,
            extensions,
            records,
            candidates,
            index,
        })
    }
    pub fn analysis_profiles(&self) -> impl Iterator<Item = &AnalysisProfile> {
        self.catalog.profiles.values()
    }
    pub fn generation_runs(&self) -> &[GenerationRun] {
        &self.catalog.runs
    }
    pub fn positions(&self) -> impl Iterator<Item = Board> + '_ {
        self.records.iter().map(|r| r.key.board())
    }
    pub fn producer(&self) -> &str {
        &self.producer
    }
    pub fn identity(&self) -> AnalysisIdentity {
        self.identity
    }
    /// 回転・反転を正規化して検索する。返す着手はBookEntry側で入力座標へ戻す。
    pub fn probe(&self, board: Board) -> Option<BookEntry<'_>> {
        let (key, transform) = Key::new(board);
        let mut slot = (key.hash() >> (64 - self.index.len().trailing_zeros())) as usize;
        loop {
            let i = self.index[slot];
            if i == usize::MAX {
                return None;
            }
            let record = &self.records[i];
            if record.key == key {
                return Some(BookEntry {
                    book: self,
                    record,
                    transform,
                });
            }
            slot = (slot + 1) & (self.index.len() - 1);
        }
    }
    pub fn stats(&self) -> BookStats {
        BookStats {
            positions: self.records.len(),
            candidates: self.candidates.len(),
            registered_moves: self.candidates.iter().filter(|c| c.registered).count(),
            solved_positions: self
                .records
                .iter()
                .filter(|r| {
                    r.final_score
                        .is_some_and(|v| self.values.at(v as usize).is_solved())
                })
                .count(),
            resident_bytes: self.catalog.bytes() as usize
                + self.values.bytes() as usize
                + self.producer.capacity()
                + extension_bytes(&self.extensions) as usize
                + self.records.capacity() * std::mem::size_of::<Record>()
                + self.candidates.capacity() * std::mem::size_of::<StoredCandidate>()
                + self.index.capacity() * std::mem::size_of::<usize>(),
        }
    }
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        format::save_book(self, path.as_ref())
    }
    pub fn load(path: impl AsRef<std::path::Path>, limits: ReadLimits) -> Result<Self> {
        format::load_book(path.as_ref(), limits)
    }
}
