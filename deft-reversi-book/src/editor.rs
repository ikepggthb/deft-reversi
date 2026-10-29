use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub(super) struct Edge {
    pub action: Action,
    pub registered: bool,
    pub direct: Option<Assessment>,
    pub manual: Option<Assessment>,
}
#[derive(Clone, Debug)]
pub(super) struct Pending {
    pub stage: usize,
    pub revision: u64,
    pub values: Vec<Option<Assessment>>,
}
#[derive(Clone, Debug)]
pub(super) struct Node {
    pub revision: u64,
    pub edges: Vec<Edge>,
    pub value: Option<Assessment>,
    pub complete: bool,
    pub completed_stages: usize,
    pub pending: Option<Pending>,
}
/// Mutable source of truth. Derived values and parent links are reconstructible.
/// A generator borrows this editor exclusively; readers use an exported Book.
pub struct BookEditor {
    pub(super) identity: AnalysisIdentity,
    pub(super) producer: String,
    pub(super) extensions: Vec<Extension>,
    pub(super) nodes: BTreeMap<Key, Node>,
    pub(super) parents: BTreeMap<Key, Vec<Key>>,
    pub(super) revision: u64,
    pub(super) workspace_bytes: u64,
    pub(super) registered_links: u64,
    pub(super) generation: Option<generate::GenerationState>,
}
impl BookEditor {
    pub fn new(identity: AnalysisIdentity) -> Self {
        Self {
            identity,
            producer: format!(
                "deft_reversi_engine/{};book-analysis-v1",
                env!("CARGO_PKG_VERSION")
            ),
            extensions: Vec::new(),
            nodes: BTreeMap::new(),
            parents: BTreeMap::new(),
            revision: 0,
            workspace_bytes: 0,
            registered_links: 0,
            generation: None,
        }
    }
    pub fn for_engine(engine: &Engine) -> Result<Self> {
        Ok(Self::new(analysis_identity(engine)?))
    }
    /// Invalidate incompatible estimates, retain proven game results, and rebuild.
    /// The old file is not changed until the caller explicitly saves.
    pub fn retarget(&mut self, engine: &Engine) -> Result<()> {
        let identity = analysis_identity(engine)?;
        if identity == self.identity {
            return Ok(());
        }
        self.bump()?;
        self.identity = identity;
        self.generation = None;
        for n in self.nodes.values_mut() {
            for e in &mut n.edges {
                if e.direct.is_some_and(|v| !v.quality.solved) {
                    e.direct = None;
                }
            }
            n.pending = None;
            n.completed_stages = 0;
            n.revision = self.revision;
        }
        self.repair()
    }
    pub fn generation_roots(&self) -> Vec<Board> {
        self.generation.as_ref().map_or_else(Vec::new, |g| {
            g.roots.iter().map(|k| k.key.board()).collect()
        })
    }
    pub fn generation_starts(&self) -> Vec<GenerationRoot> {
        self.generation.as_ref().map_or_else(Vec::new, |g| {
            g.roots
                .iter()
                .map(|r| GenerationRoot {
                    board: r.key.board(),
                    side: r.side,
                })
                .collect()
        })
    }
    pub fn producer(&self) -> &str {
        &self.producer
    }
    pub fn identity(&self) -> AnalysisIdentity {
        self.identity
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    /// Conservative storage estimate including tree/index overhead, excluding Engine.
    pub fn estimated_bytes(&self) -> u64 {
        extension_bytes(&self.extensions)
            + self.workspace_bytes
            + self.registered_links * 64
            + self.generation.as_ref().map_or(0, |g| g.estimated_bytes())
    }
    pub fn insert_position(&mut self, board: Board) -> Result<bool> {
        self.ensure(Key::new(board).0)
    }
    pub(super) fn ensure(&mut self, key: Key) -> Result<bool> {
        if key.board().empty_count() > 60 {
            return Err(invalid("book position must have at least four stones"));
        }
        if self.nodes.contains_key(&key) {
            return Ok(false);
        }
        self.bump()?;
        let board = key.board();
        let edges: Vec<_> = legal_actions(board)
            .into_iter()
            .map(|action| Edge {
                action,
                registered: false,
                direct: None,
                manual: None,
            })
            .collect();
        self.workspace_bytes += 256
            + std::mem::size_of::<Node>() as u64
            + edges.capacity() as u64
                * (std::mem::size_of::<Edge>() + std::mem::size_of::<Option<Assessment>>()) as u64;
        self.nodes.insert(
            key,
            Node {
                revision: self.revision,
                edges,
                value: board.is_terminal().then(|| terminal_value(board)),
                complete: board.is_terminal(),
                completed_stages: 0,
                pending: None,
            },
        );
        Ok(true)
    }
    fn bump(&mut self) -> Result<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("revision overflow"))?;
        self.extensions.clear();
        Ok(())
    }
    /// Register a branch without claiming it has been evaluated.
    pub fn register_move(&mut self, board: Board, action: Action) -> Result<()> {
        board
            .play(action)
            .map_err(|_| invalid("illegal registered move"))?;
        let (key, t) = Key::new(board);
        self.ensure(key)?;
        self.register(key, t.action(action))
    }
    pub(super) fn register(&mut self, key: Key, action: Action) -> Result<()> {
        let child = Key::new(
            key.board()
                .play(action)
                .map_err(|_| invalid("illegal link"))?,
        )
        .0;
        self.ensure(child)?;
        let idx = self.nodes[&key]
            .edges
            .iter()
            .position(|e| e.action == action)
            .ok_or_else(|| invalid("missing move"))?;
        if !self.nodes[&key].edges[idx].registered {
            self.bump()?;
            let node = self.nodes.get_mut(&key).unwrap();
            node.edges[idx].registered = true;
            node.revision = self.revision;
            self.registered_links += 1;
            let parents = self.parents.entry(child).or_default();
            if !parents.contains(&key) {
                parents.push(key);
            }
            self.refresh_from(key);
        }
        Ok(())
    }
    /// Sparse insertion of an independently completed candidate analysis.
    /// The score is from the input parent's side; action coordinates are transformed.
    pub fn record(&mut self, board: Board, action: Action, value: Assessment) -> Result<()> {
        board
            .play(action)
            .map_err(|_| invalid("illegal candidate"))?;
        value.validate_at(board)?;
        if !value.quality.solved && value.identity != self.identity {
            return Err(invalid("analysis identity mismatch"));
        }
        let (key, t) = Key::new(board);
        self.ensure(key)?;
        let action = t.action(action);
        let idx = self.nodes[&key]
            .edges
            .iter()
            .position(|e| e.action == action)
            .unwrap();
        if let Some(old) = self.nodes[&key].edges[idx]
            .direct
            .filter(|_| value.origin != ValueOrigin::Manual)
        {
            if old.quality.solved && value.quality.solved && old.score != value.score {
                return Err(invalid("conflicting solved values"));
            }
            if !dominates(value, old) {
                return Err(invalid("incomparable or weaker candidate analysis"));
            }
        }
        self.bump()?;
        let n = self.nodes.get_mut(&key).unwrap();
        if value.origin == ValueOrigin::Manual {
            n.edges[idx].manual = Some(value);
        } else {
            n.edges[idx].direct = Some(value);
        }
        n.pending = None;
        n.revision = self.revision;
        self.refresh_from(key);
        Ok(())
    }
    /// Remove a registered branch, retaining its direct analysis as a frontier.
    pub fn unregister_move(&mut self, board: Board, action: Action) -> Result<()> {
        let (key, t) = Key::new(board);
        let a = t.action(action);
        let n = self
            .nodes
            .get(&key)
            .ok_or_else(|| invalid("position not found"))?;
        let idx = n
            .edges
            .iter()
            .position(|e| e.action == a)
            .ok_or_else(|| invalid("illegal move"))?;
        self.bump()?;
        let n = self.nodes.get_mut(&key).unwrap();
        n.edges[idx].registered = false;
        n.revision = self.revision;
        n.pending = None;
        self.rebuild_parents();
        self.refresh_from(key);
        self.generation = None;
        Ok(())
    }
    pub(super) fn effective(&self, key: Key, edge: &Edge) -> Option<Assessment> {
        if let Some(v) = edge.manual.filter(|v| v.identity == self.identity) {
            return Some(v);
        }
        let direct = edge.direct;
        if !edge.registered {
            return direct;
        }
        let child = Key::new(key.board().play(edge.action).expect("validated edge")).0;
        let propagated = self
            .nodes
            .get(&child)
            .filter(|n| n.complete)
            .and_then(|n| n.value)
            .map(|v| v.child_to_parent(matches!(edge.action, Action::Place(_))));
        match (direct, propagated) {
            (Some(d), Some(p)) if dominates(p, d) => Some(p),
            (Some(d), _) => Some(d),
            (None, p) => p,
        }
    }
    fn calculate(&self, key: Key) -> (Option<Assessment>, bool) {
        let n = &self.nodes[&key];
        if n.edges.is_empty() {
            return (Some(terminal_value(key.board())), true);
        }
        let mut best: Option<Assessment> = None;
        let mut depth = 60;
        let mut selectivity = Selectivity::Full;
        let mut all_solved = true;
        for edge in &n.edges {
            let Some(v) = self.effective(key, edge) else {
                return (None, false);
            };
            if v.bound != Bound::Exact || (!v.quality.solved && v.identity != self.identity) {
                return (None, false);
            }
            if best.is_none_or(|b| v.score > b.score) {
                best = Some(v);
            }
            depth = depth.min(v.quality.depth);
            if (v.quality.selectivity as u8) < (selectivity as u8) {
                selectivity = v.quality.selectivity;
            }
            all_solved &= v.quality.solved;
        }
        let mut value = best.unwrap();
        value.quality = AnalysisQuality {
            depth,
            selectivity,
            solved: all_solved,
        };
        value.identity = self.identity;
        // A backed-up manual value never becomes a proof merely through propagation.
        value.origin = if all_solved {
            ValueOrigin::Search
        } else {
            value.origin
        };
        (Some(value), true)
    }
    pub(super) fn refresh_from(&mut self, key: Key) -> Vec<Key> {
        let mut affected = Vec::new();
        let mut todo = BTreeSet::new();
        todo.insert(order(key));
        while let Some((_, _, key)) = todo.pop_first() {
            affected.push(key);
            let calculated = self.calculate(key);
            let n = self.nodes.get_mut(&key).unwrap();
            if (n.value, n.complete) != calculated {
                (n.value, n.complete) = calculated;
                if let Some(parents) = self.parents.get(&key) {
                    for &p in parents {
                        todo.insert(order(p));
                    }
                }
            }
        }
        if let Some(g) = &mut self.generation {
            for &k in &affected {
                g.invalidate(k);
            }
        }
        affected
    }
    pub fn repair(&mut self) -> Result<()> {
        self.validate()?;
        self.rebuild_parents();
        let mut keys: Vec<_> = self.nodes.keys().copied().map(order).collect();
        keys.sort_unstable();
        for (_, _, k) in keys {
            let (v, c) = self.calculate(k);
            let n = self.nodes.get_mut(&k).unwrap();
            n.value = v;
            n.complete = c;
        }
        Ok(())
    }
    pub(super) fn rebuild_parents(&mut self) {
        self.parents.clear();
        self.registered_links = 0;
        for (&key, n) in &self.nodes {
            for e in n.edges.iter().filter(|e| e.registered) {
                self.registered_links += 1;
                let child = Key::new(key.board().play(e.action).unwrap()).0;
                let ps = self.parents.entry(child).or_default();
                if !ps.contains(&key) {
                    ps.push(key);
                }
            }
        }
    }
    pub fn validate(&self) -> Result<()> {
        for (&key, n) in &self.nodes {
            let board =
                Board::from_bits(key.0, key.1).ok_or_else(|| invalid("overlapping stones"))?;
            if Key::new(board).0 != key {
                return Err(invalid("noncanonical position"));
            }
            let legal = legal_actions(board);
            if legal.len() != n.edges.len()
                || !legal.iter().zip(&n.edges).all(|(a, e)| *a == e.action)
            {
                return Err(invalid("candidate set is not canonical legal move list"));
            }
            for e in &n.edges {
                if let Some(v) = e.direct {
                    v.validate_at(board)?;
                    if !v.quality.solved && v.identity != self.identity {
                        return Err(invalid("foreign analysis"));
                    }
                }
                if e.registered
                    && !self
                        .nodes
                        .contains_key(&Key::new(board.play(e.action).unwrap()).0)
                {
                    return Err(invalid("dangling child"));
                }
            }
            for e in &n.edges {
                if let Some(v) = e.manual {
                    v.validate_at(board)?;
                    if v.origin != ValueOrigin::Manual {
                        return Err(invalid("manual provenance"));
                    }
                }
            }
            if let Some(p) = &n.pending {
                if p.values.len() != n.edges.len() || p.revision != n.revision {
                    return Err(invalid("stale pending stage"));
                }
                for v in p.values.iter().flatten() {
                    v.validate_at(board)?;
                    if !v.quality.solved && v.identity != self.identity {
                        return Err(invalid("foreign pending value"));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn export(&self) -> Result<Book> {
        Book::from_editor(self)
    }
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        format::save_editor(self, path.as_ref())
    }
    pub fn load(path: impl AsRef<std::path::Path>, limits: ReadLimits) -> Result<Self> {
        format::load_editor(path.as_ref(), limits)
    }
}
fn order(key: Key) -> (u8, bool, Key) {
    let b = key.board();
    (b.empty_count(), b.must_pass(), key)
}
pub(super) fn dominates(new: Assessment, old: Assessment) -> bool {
    if new.quality.solved {
        return true;
    }
    if old.quality.solved {
        return false;
    }
    new.identity == old.identity
        && new.bound == Bound::Exact
        && (old.bound != Bound::Exact
            || (new.quality.depth >= old.quality.depth
                && (new.quality.selectivity as u8) >= (old.quality.selectivity as u8)))
}
