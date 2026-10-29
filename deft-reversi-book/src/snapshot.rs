use super::*;
#[derive(Clone, Copy, Debug)]
pub(super) struct StoredCandidate {
    pub action: Action,
    pub registered: bool,
    pub value: Option<CompactValue>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct CompactValue {
    pub score: i8,
    pub depth: u8,
    pub selectivity: u8,
    pub solved: bool,
    pub bound: Bound,
    pub origin: ValueOrigin,
}
impl CompactValue {
    pub fn new(v: Assessment) -> Self {
        Self {
            score: v.score as i8,
            depth: v.quality.depth,
            selectivity: v.quality.selectivity as u8,
            solved: v.quality.solved,
            bound: v.bound,
            origin: v.origin,
        }
    }
    pub fn expand(self, identity: AnalysisIdentity) -> Assessment {
        Assessment {
            score: i16::from(self.score),
            quality: AnalysisQuality {
                depth: self.depth,
                selectivity: selectivity(self.selectivity).expect("validated selectivity"),
                solved: self.solved,
            },
            bound: self.bound,
            identity,
            origin: self.origin,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Record {
    pub key: Key,
    pub start: usize,
    pub len: usize,
    pub complete: bool,
    pub value: Option<CompactValue>,
}
/// Immutable, compact play snapshot. No mutable graph, reverse links, or jobs.
pub struct Book {
    pub(super) identity: AnalysisIdentity,
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
    pub value: Option<Assessment>,
}
/// Borrowed lookup view; iteration restores coordinates without allocation.
pub struct BookEntry<'a> {
    book: &'a Book,
    record: &'a Record,
    transform: Symmetry,
}
impl BookEntry<'_> {
    pub fn candidates(&self) -> impl Iterator<Item = BookCandidate> + '_ {
        self.book.candidates[self.record.start..self.record.start + self.record.len]
            .iter()
            .map(|c| BookCandidate {
                action: self.transform.inverse().action(c.action),
                registered: c.registered,
                value: c.value.map(|v| v.expand(self.book.identity)),
            })
    }
    pub fn all_moves_evaluated(&self) -> bool {
        self.record.complete
    }
    pub fn value(&self) -> Option<Assessment> {
        self.record.value.map(|v| v.expand(self.book.identity))
    }
    pub fn is_terminal(&self) -> bool {
        self.record.len == 0
    }
}
impl Book {
    pub(super) fn from_editor(editor: &BookEditor) -> Result<Self> {
        editor.validate()?;
        // Reconstruct derived values rather than trusting caller-supplied summaries.
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
                    value: editor.effective(key, edge).map(CompactValue::new),
                });
            }
            records.push(Record {
                key,
                start,
                len: node.edges.len(),
                complete: node.complete,
                value: node.value.map(CompactValue::new),
            });
        }
        Self::from_parts(
            editor.identity,
            editor.producer.clone(),
            editor.extensions.clone(),
            records,
            candidates,
        )
    }
    pub(super) fn from_parts(
        identity: AnalysisIdentity,
        producer: String,
        extensions: Vec<Extension>,
        records: Vec<Record>,
        candidates: Vec<StoredCandidate>,
    ) -> Result<Self> {
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
            producer,
            extensions,
            records,
            candidates,
            index,
        })
    }
    pub fn producer(&self) -> &str {
        &self.producer
    }
    pub fn identity(&self) -> AnalysisIdentity {
        self.identity
    }
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
                .filter(|r| r.value.is_some_and(|v| v.solved))
                .count(),
            resident_bytes: self.producer.capacity()
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
