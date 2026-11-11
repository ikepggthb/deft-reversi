//! 局面グラフの編集と、子局面から親局面への値の更新。
//!
//! 読む入口はBookEditor::record_search / register_move。採用する値をevidenceで
//! 決め、refresh_fromで影響する親まで再計算する。評価記録そのものはvaluesが所有する。
//! 保存の入口は末尾のsave / loadで、形式の処理はformat.rsへ委譲する。
use super::*;
use std::collections::{BTreeMap, BTreeSet};
// 手固有の独立評価がある場合だけ確保する。子から得る値や集合探索の上限は
// ここへ全候補分コピーせず、evidenceで必要なときに組み合わせる。
#[derive(Clone, Debug, Default)]
struct CandidateNotes {
    direct: Option<ValueId>,
    manual: Option<ValueId>,
    certificate: Option<ValueId>,
}
#[derive(Clone, Debug)]
pub(super) struct Edge {
    pub action: Action,
    /// 着手後の正規化済み局面。未登録でも座標計算を省くために持つ。
    pub child: Key,
    /// 子局面が存在するだけではtrueにならない。この親からの進行としての登録。
    pub registered: bool,
    notes: Option<Box<CandidateNotes>>,
}
impl Edge {
    pub fn new(
        action: Action,
        child: Key,
        registered: bool,
        direct: Option<ValueId>,
        manual: Option<ValueId>,
        certificate: Option<ValueId>,
    ) -> Self {
        let notes = if direct.is_some() || manual.is_some() || certificate.is_some() {
            Some(Box::new(CandidateNotes {
                direct,
                manual,
                certificate,
            }))
        } else {
            None
        };
        Self {
            action,
            child,
            registered,
            notes,
        }
    }
    pub fn direct(&self) -> Option<ValueId> {
        self.notes.as_ref().and_then(|n| n.direct)
    }
    pub fn manual(&self) -> Option<ValueId> {
        self.notes.as_ref().and_then(|n| n.manual)
    }
    pub fn certificate(&self) -> Option<ValueId> {
        self.notes.as_ref().and_then(|n| n.certificate)
    }
    fn cleanup(&mut self) {
        if self
            .notes
            .as_ref()
            .is_some_and(|n| n.direct.is_none() && n.manual.is_none() && n.certificate.is_none())
        {
            self.notes = None;
        }
    }
    pub fn set_direct(&mut self, v: Option<ValueId>) {
        if v.is_some() {
            self.notes.get_or_insert_with(Default::default).direct = v;
        } else if let Some(n) = &mut self.notes {
            n.direct = None;
        }
        self.cleanup();
    }
    pub fn set_manual(&mut self, v: Option<ValueId>) {
        if v.is_some() {
            self.notes.get_or_insert_with(Default::default).manual = v;
        } else if let Some(n) = &mut self.notes {
            n.manual = None;
        }
        self.cleanup();
    }
    pub fn set_certificate(&mut self, v: Option<ValueId>) {
        if v.is_some() {
            self.notes.get_or_insert_with(Default::default).certificate = v;
        } else if let Some(n) = &mut self.notes {
            n.certificate = None;
        }
        self.cleanup();
    }
}
// 旧方式で保存された候補別の途中状態。新しい生成器は作らず、読込時に
// normalize_pendingで完了済みの値だけを通常の候補記録へ取り込む。
#[derive(Clone, Debug)]
pub(super) struct Pending {
    pub stage: usize,
    pub revision: u64,
    pub values: Vec<Option<ValueId>>,
}
/// 一回の集合探索の結果。Frontierはここでは「対象候補の中の最良手と残りの上限」。
/// bestの点評価と、それ以外の手の上限は同じ探索記録を異なる意味で参照する。
#[derive(Clone, Debug)]
pub(super) struct Frontier {
    /// Canonical edge indexes covered by a single restricted root search.
    pub mask: u64,
    pub best: Action,
    pub value: ValueId,
    pub pv: Vec<Action>,
    pub(super) point: ValueId,
    pub(super) upper: ValueId,
}
impl Frontier {
    pub(super) fn new(
        mask: u64,
        best: Action,
        value: Assessment,
        pv: Vec<Action>,
        values: &mut ValueStore,
    ) -> Result<Self> {
        Ok(Self {
            mask,
            best,
            value: values.intern(value),
            pv,
            point: values.intern(value.project_subset(false)?),
            upper: values.intern(value.project_subset(true)?),
        })
    }
    pub(super) fn assessment(
        &self,
        index: usize,
        action: Action,
        values: &ValueStore,
    ) -> Option<Assessment> {
        if self.mask & (1u64 << index) == 0 {
            return None;
        }
        Some(values.get(if action == self.best {
            self.point
        } else {
            self.upper
        }))
    }
}
#[derive(Clone, Debug)]
pub(super) struct Node {
    /// この局面の直接記録・登録状態の編集世代。子からの再計算だけでは変えない。
    /// 探索開始後に変わっていればgrow::publishは結果を採用しない。
    pub revision: u64,
    pub notes_bytes: u64,
    pub edges: Vec<Edge>,
    /// 現在採用する局面全体の値。手動値を含む場合がある。
    pub value: Option<ValueId>,
    /// 全候補に点評価があるか。局面全体の値が求まるだけではtrueにしない。
    pub complete: bool,
    /// 親の手番から見た値。添字0=パス経由、1=石を置く手経由。
    pub backups: [Option<ValueId>; 2],
    /// 手動値とは別に保持する、最終石差の確定値または上下限。
    pub proof: Option<ValueId>,
    pub proof_backups: Option<Box<[ValueId; 2]>>,
    pub completed_stages: usize,
    pub pending: Option<Pending>,
    pub frontier: Option<Frontier>,
}
impl Node {
    fn current_notes_bytes(&self) -> u64 {
        self.edges.iter().filter(|e| e.notes.is_some()).count() as u64
            * (std::mem::size_of::<CandidateNotes>() as u64 + 16)
    }

    /// Retiring an aggregate must not discard its individual certificates.
    /// Non-best moves retain upper bounds, never fabricated point values.
    /// Incomparable lower/upper pairs still need aggregate coverage from the
    /// retained frontier or its replacement subset.
    pub(super) fn preserve_frontier_proofs(&mut self, values: &mut ValueStore) {
        let Some(frontier) = self
            .frontier
            .as_ref()
            .filter(|f| values.get(f.value).proven())
        else {
            return;
        };
        for (index, edge) in self.edges.iter_mut().enumerate() {
            if let Some(value) = frontier.assessment(index, edge.action, values)
                && edge
                    .direct()
                    .is_none_or(|old| dominates(value, values.get(old)))
            {
                edge.set_direct(Some(values.intern(value)));
            }
        }
    }
}
/// 旧ファイルの採用規則を復元・比較するための区別。
/// 現在の編集・生成は常にBookを使い、公開の切替設定はない。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum PropagationPolicy {
    /// Use a child backup only when it dominates the direct search quality.
    #[default]
    Quality,
    /// Prefer a value-covered child backup, preserving proven direct points.
    Book,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueSource {
    Direct,
    Manual,
    Propagated,
    Frontier,
    Certificate,
}
/// All values use the input parent's perspective, including `propagated`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateEvidence {
    /// 登録した子に残る最終石差の情報を、親の手番へ変換したもの。
    pub propagated_certificate: Option<Assessment>,
    /// この手について集めた最終石差の上下限の共通範囲。手動値とは独立。
    pub certificate: Option<Assessment>,
    pub registered: bool,
    /// この手を独立に評価した記録。子の値に採用が切り替わっても保持する。
    pub direct: Option<Assessment>,
    /// Retained even when its identity is no longer active.
    pub manual: Option<Assessment>,
    /// 登録した子の現在値を、親の手番へ変換したもの。
    pub propagated: Option<Assessment>,
    /// 集合探索から得る、この手の点評価または上限。
    pub frontier: Option<Assessment>,
    /// 上記から現在採用する値。sourceがその選択元を示す。
    pub effective: Option<Assessment>,
    pub source: Option<ValueSource>,
}
pub(super) struct EditCheckpoint {
    key: Key,
    node: Node,
    revision: u64,
    extensions: Vec<Extension>,
    workspace_bytes: u64,
}
/// Summary of a one-time old-file conversion; never used to select moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub source_version: u32,
    pub positions: u64,
    pub registered_moves: u64,
    pub changed_position_values: u64,
    pub changed_candidate_values: u64,
    pub jobs_rebuilt: bool,
}
/// Mutable source of truth. Derived values and parent links are reconstructible.
/// A generator borrows this editor exclusively; readers use an exported Book.
pub struct BookEditor {
    pub(super) migration: Option<MigrationReport>,
    pub(super) identity: AnalysisIdentity,
    pub(super) catalog: catalog::Catalog,
    pub(super) values: ValueStore,
    pub(super) active_run: Option<u32>,
    pub(super) pending_count: usize,
    pub(super) propagation: PropagationPolicy,
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
            migration: None,
            catalog: catalog::Catalog::default(),
            values: ValueStore::default(),
            active_run: None,
            pending_count: 0,
            propagation: PropagationPolicy::Book,
            producer: format!(
                "deft_reversi_book/{};search-revision/{}",
                env!("CARGO_PKG_VERSION"),
                deft_reversi_engine::SEARCH_SEMANTICS_REVISION
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
        let mut editor = Self::new(analysis_identity(engine)?);
        editor.catalog.engine(editor.identity);
        Ok(editor)
    }
    /// Invalidate incompatible estimates, retain proven game results, and rebuild.
    /// The old file is not changed until the caller explicitly saves.
    pub fn retarget(&mut self, engine: &Engine) -> Result<()> {
        let identity = analysis_identity(engine)?;
        self.normalize_if_pending()?;
        if identity == self.identity {
            return Ok(());
        }
        self.bump()?;
        self.identity = identity;
        self.catalog.engine(identity);
        self.generation = None;
        for n in self.nodes.values_mut() {
            for e in &mut n.edges {
                if e.direct().is_some_and(|v| !self.values.get(v).proven()) {
                    e.set_direct(None);
                }
            }
            n.pending = None;
            n.frontier = n
                .frontier
                .take()
                .filter(|f| self.values.get(f.value).proven());
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
    // 編集対象の変更前状態。親への伝播はrefresh_fromが別途巻き戻しを担当する。
    // 新たに追加した子局面の削除など、操作固有の後始末は呼出し元が担当する。
    pub(super) fn checkpoint(&self, key: Key) -> EditCheckpoint {
        EditCheckpoint {
            key,
            node: self.nodes[&key].clone(),
            revision: self.revision,
            extensions: self.extensions.clone(),
            workspace_bytes: self.workspace_bytes,
        }
    }
    pub(super) fn commit_checkpoint(&mut self, old: EditCheckpoint) -> Result<Vec<Key>> {
        match self.refresh_from(old.key) {
            Ok(keys) => Ok(keys),
            Err(error) => {
                self.nodes.insert(old.key, old.node);
                self.revision = old.revision;
                self.extensions = old.extensions;
                self.workspace_bytes = old.workspace_bytes;
                self.rebuild_parents();
                self.compact_evidence()?;
                Err(error)
            }
        }
    }
    pub(super) fn normalize_if_pending(&mut self) -> Result<()> {
        if self.pending_count != 0 {
            self.normalize_pending()?;
        }
        Ok(())
    }
    pub(super) fn normalize_pending(&mut self) -> Result<()> {
        let keys: Vec<_> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.pending.is_some())
            .map(|(&k, _)| k)
            .collect();
        self.pending_count = keys.len();
        let mut affected = BTreeSet::new();
        for key in keys {
            let old = self.checkpoint(key);
            self.bump()?;
            let pending = self.nodes.get_mut(&key).unwrap().pending.take().unwrap();
            let node = self.nodes.get_mut(&key).unwrap();
            node.preserve_frontier_proofs(&mut self.values);
            for (edge, value) in node.edges.iter_mut().zip(pending.values) {
                if let Some(value_id) = value {
                    let value = self.values.get(value_id);
                    if value.origin() == ValueOrigin::Manual {
                        if edge.manual().is_none() {
                            edge.set_manual(Some(self.values.intern(value)));
                        }
                    } else if edge.direct().is_none_or(|old| {
                        dominates(value, self.values.get(old))
                            || value.proven() && self.values.get(old).proven()
                    }) {
                        edge.set_direct(Some(self.values.intern(value)));
                    }
                }
            }
            node.revision = self.revision;
            node.frontier = node
                .frontier
                .take()
                .filter(|f| self.values.get(f.value).proven());
            affected.extend(self.commit_checkpoint(old)?);
            self.pending_count -= 1;
        }
        self.reschedule_edit(&affected.into_iter().collect::<Vec<_>>());
        Ok(())
    }
    pub fn analysis_profiles(&self) -> impl Iterator<Item = &AnalysisProfile> {
        self.catalog.profiles.values()
    }
    pub fn generation_runs(&self) -> &[GenerationRun] {
        &self.catalog.runs
    }
    pub fn set_model_label(&mut self, label: String) -> Result<()> {
        if label.len() > 4096 {
            return Err(invalid("model label too long"));
        }
        self.catalog
            .profiles
            .entry(self.identity)
            .or_insert(AnalysisProfile {
                identity: self.identity,
                engine_version: None,
                search_revision: None,
                model_label: None,
                build_label: None,
            })
            .model_label = Some(label);
        Ok(())
    }
    pub fn set_build_label(&mut self, label: String) -> Result<()> {
        if label.len() > 4096 {
            return Err(invalid("build label too long"));
        }
        let profile = self
            .catalog
            .profiles
            .get_mut(&self.identity)
            .ok_or_else(|| invalid("analysis profile required for build label"))?;
        profile.build_label = Some(label);
        Ok(())
    }
    /// 現在の局面・候補・再開状態が参照する評価だけで一覧を作り直す。
    /// 記録番号は全参照を同時に付け替えるため、外部へ公開しない。
    /// NodeなどにValueIdの保持先を追加する場合は、ここにも必ず追加する。
    pub fn compact_evidence(&mut self) -> Result<()> {
        let mut replacement = ValueStore::default();
        let mut remap = BTreeMap::new();
        let old = &self.values;
        let mut move_id = |id: ValueId| {
            *remap
                .entry(id)
                .or_insert_with(|| replacement.intern(old.get(id)))
        };
        for n in self.nodes.values_mut() {
            n.value = n.value.map(&mut move_id);
            n.proof = n.proof.map(&mut move_id);
            for v in &mut n.backups {
                *v = v.map(&mut move_id);
            }
            if let Some(v) = &mut n.proof_backups {
                for id in v.iter_mut() {
                    *id = move_id(*id);
                }
            }
            for e in &mut n.edges {
                e.set_direct(e.direct().map(&mut move_id));
                e.set_manual(e.manual().map(&mut move_id));
                e.set_certificate(e.certificate().map(&mut move_id));
            }
            if let Some(f) = &mut n.frontier {
                f.value = move_id(f.value);
                f.point = move_id(f.point);
                f.upper = move_id(f.upper);
            }
            if let Some(p) = &mut n.pending {
                for v in &mut p.values {
                    *v = v.map(&mut move_id);
                }
            }
        }
        self.values = replacement;
        self.catalog.derivations.clear();
        Ok(())
    }
    pub fn migration_report(&self) -> Option<MigrationReport> {
        self.migration
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
    /// Canonical stored boards, including retained positions outside current roots.
    pub fn positions(&self) -> impl Iterator<Item = Board> + '_ {
        self.nodes.keys().map(|k| k.board())
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    /// Conservative storage estimate including tree/index overhead, excluding Engine.
    pub fn estimated_bytes(&self) -> u64 {
        self.catalog.bytes()
            + self.values.bytes()
            + extension_bytes(&self.extensions)
            + self.nodes.len() as u64 * 256 * std::mem::size_of::<Action>() as u64
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
            .map(|action| {
                Edge::new(
                    action,
                    Key::new(board.play(action).unwrap()).0,
                    false,
                    None,
                    None,
                    None,
                )
            })
            .collect();
        self.workspace_bytes += if board.is_terminal() {
            proof_cache_bytes()
        } else {
            0
        };
        self.workspace_bytes += 256
            + std::mem::size_of::<Node>() as u64
            + edges.capacity() as u64
                * (std::mem::size_of::<Edge>() + std::mem::size_of::<Option<ValueId>>()) as u64;
        let terminal = board
            .is_terminal()
            .then(|| self.values.intern(terminal_value(board)));
        let terminal_backups = board.is_terminal().then(|| {
            let v = terminal_value(board);
            [
                self.values.intern(v.child_to_parent(false)),
                self.values.intern(v.child_to_parent(true)),
            ]
        });
        self.nodes.insert(
            key,
            Node {
                revision: self.revision,
                notes_bytes: 0,
                edges,
                value: terminal,
                proof: terminal,
                proof_backups: terminal_backups.map(Box::new),
                complete: board.is_terminal(),
                backups: terminal_backups.map_or([None; 2], |v| [Some(v[0]), Some(v[1])]),
                completed_stages: 0,
                pending: None,
                frontier: None,
            },
        );
        Ok(true)
    }
    pub(super) fn bump(&mut self) -> Result<()> {
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
        self.normalize_if_pending()?;
        let (key, t) = Key::new(board);
        self.ensure(key)?;
        let action = t.action(action);
        self.register(key, action)?;
        Ok(())
    }
    pub(super) fn register(&mut self, key: Key, action: Action) -> Result<Vec<Key>> {
        let child = self.nodes[&key]
            .edges
            .iter()
            .find(|e| e.action == action)
            .ok_or_else(|| invalid("illegal link"))?
            .child;
        let old = self.checkpoint(key);
        let added = self.ensure(child)?;
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
            node.pending = None;
            self.registered_links += 1;
            let parents = self.parents.entry(child).or_default();
            if !parents.contains(&key) {
                parents.push(key);
            }
            let affected = match self.commit_checkpoint(old) {
                Ok(v) => v,
                Err(error) => {
                    if added {
                        self.nodes.remove(&child);
                    }
                    return Err(error);
                }
            };
            self.reschedule_edit(&affected);
            self.enqueue_registered_link(key, action);
            return Ok(affected);
        }
        Ok(Vec::new())
    }
    /// 特定の手の検査済み評価を追加する。点数は親の手番側から見た値。
    /// board/actionは入力座標から正規化するが、value内の盤面・手は正規化済みが必要。
    /// 集合全体の最大値は直接渡せない。手動値はrecord_manualを使う。
    pub fn record_candidate(
        &mut self,
        board: Board,
        action: Action,
        value: Assessment,
    ) -> Result<()> {
        if matches!(value.analysis(), ValueAnalysis::Manual { .. }) {
            return Err(invalid("use record_manual for manual values"));
        }
        self.record_value(board, action, value)
    }
    /// Import explicitly asserted external data. Unknown search modes stay unknown.
    pub fn record_imported(
        &mut self,
        board: Board,
        action: Action,
        value: ImportedValue,
    ) -> Result<()> {
        let value = Assessment::import(value)?;
        if matches!(value.analysis(), ValueAnalysis::Manual { .. }) {
            self.record_manual(board, action, value)
        } else {
            self.record_candidate(board, action, value)
        }
    }
    pub fn record_manual(&mut self, board: Board, action: Action, value: Assessment) -> Result<()> {
        if !matches!(value.analysis(), ValueAnalysis::Manual { .. }) || value.proven() {
            return Err(invalid("manual estimate required"));
        }
        self.record_value(board, action, value)
    }
    /// 手動の点評価を設定する。深さ0は表示用で、探索を完了したという申告ではない。
    pub fn set_manual(&mut self, board: Board, action: Action, score: i16) -> Result<()> {
        let value = Assessment::new(
            ValueClaim::Estimate {
                score,
                bound: EstimateBound::Point,
            },
            ValueAnalysis::Manual {
                depth: 0,
                selectivity: Selectivity::Full,
            },
            self.identity,
        )?;
        self.record_manual(board, action, value)
    }
    /// Publish a completed root search, preserving its actual subject and result.
    /// The caller supplies a result from this book's analysis profile, in canonical coordinates.
    pub fn record_search(
        &mut self,
        board: Board,
        requested: RequestedAnalysis,
        planned: SearchKind,
        result: &deft_reversi_engine::SearchResult,
    ) -> Result<()> {
        let value =
            Assessment::from_search_result(board, requested, planned, result, self.identity)?;
        if board.is_terminal() {
            self.ensure(Key::new(board).0)?;
            return Ok(());
        }
        self.normalize_if_pending()?;
        let key = Key::new(board).0;
        self.ensure(key)?;
        let best = result
            .best_move
            .ok_or_else(|| invalid("completed search has no move"))?;
        let mask = self.nodes[&key]
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| match e.action {
                Action::Place(s) => result.root_moves & (1u64 << s.index()) != 0,
                Action::Pass => true,
            })
            .fold(0, |m, (i, _)| m | (1u64 << i));
        let frontier = Frontier::new(mask, best, value, result.pv.clone(), &mut self.values)?;
        let old = self.checkpoint(key);
        self.bump()?;
        let node = self.nodes.get_mut(&key).unwrap();
        node.preserve_frontier_proofs(&mut self.values);
        let edge = node
            .edges
            .iter_mut()
            .find(|e| e.action == best)
            .ok_or_else(|| invalid("search move"))?;
        edge.set_direct(Some(frontier.point));
        node.frontier = Some(frontier);
        node.pending = None;
        node.revision = self.revision;
        let affected = self.commit_checkpoint(old)?;
        self.reschedule_edit(&affected);
        Ok(())
    }
    fn record_value(&mut self, board: Board, action: Action, value: Assessment) -> Result<()> {
        board
            .play(action)
            .map_err(|_| invalid("illegal candidate"))?;
        value.validate_at(board)?;
        if !value.proven() && value.identity() != self.identity {
            return Err(invalid("analysis identity mismatch"));
        }
        let (key, t) = Key::new(board);
        let action = t.action(action);
        value.validate_candidate(key, action)?;
        self.normalize_if_pending()?;
        self.ensure(key)?;
        let idx = self.nodes[&key]
            .edges
            .iter()
            .position(|e| e.action == action)
            .unwrap();
        let node = &self.nodes[&key];
        if let (Some(a), Some(b)) = (
            node.edges[idx]
                .certificate()
                .and_then(|v| self.values.get(v).claim().certificate()),
            value.claim().certificate(),
        ) {
            a.intersect(b)?;
        }
        let mut previous = self.values.opt(node.edges[idx].direct());
        if let Some(proof) = node
            .frontier
            .as_ref()
            .and_then(|f| f.assessment(idx, action, &self.values))
            .filter(|v| v.proven())
            && previous.is_none_or(|old| dominates(proof, old))
        {
            previous = Some(proof);
        }
        if let Some(old) = previous.filter(|_| value.origin() != ValueOrigin::Manual) {
            if old.quality().solved && value.quality().solved && old.score() != value.score() {
                return Err(invalid("conflicting solved values"));
            }
            if !dominates(value, old) && !(old.proven() && value.proven()) {
                return Err(invalid("incomparable or weaker candidate analysis"));
            }
        }
        let old = self.checkpoint(key);
        self.bump()?;

        let n = self.nodes.get_mut(&key).unwrap();
        n.preserve_frontier_proofs(&mut self.values);
        if value.origin() == ValueOrigin::Manual {
            n.edges[idx].set_manual(Some(self.values.intern(value)));
        } else {
            n.edges[idx].set_direct(Some(self.values.intern(value)));
        }
        n.pending = None;
        n.frontier = n
            .frontier
            .take()
            .filter(|f| self.values.get(f.value).proven());
        n.revision = self.revision;
        let affected = self.commit_checkpoint(old)?;
        self.reschedule_edit(&affected);
        Ok(())
    }
    pub fn clear_manual(&mut self, board: Board, action: Action) -> Result<()> {
        let (key, t) = Key::new(board);
        let action = t.action(action);
        let n = self
            .nodes
            .get(&key)
            .ok_or_else(|| invalid("position not found"))?;
        let index = n
            .edges
            .iter()
            .position(|e| e.action == action)
            .ok_or_else(|| invalid("illegal candidate"))?;
        if n.edges[index].manual().is_none() {
            return Ok(());
        }
        self.normalize_if_pending()?;
        let old = self.checkpoint(key);
        self.bump()?;
        let n = self.nodes.get_mut(&key).unwrap();
        n.edges[index].set_manual(None);
        n.revision = self.revision;
        n.pending = None;
        let affected = self.commit_checkpoint(old)?;
        self.reschedule_edit(&affected);
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
        self.normalize_if_pending()?;
        let old = self.checkpoint(key);
        self.bump()?;
        let n = self.nodes.get_mut(&key).unwrap();
        n.preserve_frontier_proofs(&mut self.values);
        n.edges[idx].registered = false;
        n.revision = self.revision;
        n.pending = None;
        n.frontier = n
            .frontier
            .take()
            .filter(|f| self.values.get(f.value).proven());
        self.rebuild_parents();
        self.commit_checkpoint(old)?;
        if let Some(g) = self.generation.take() {
            let roots: Vec<_> = g
                .roots
                .iter()
                .map(|c| GenerationRoot {
                    board: c.key.board(),
                    side: c.side,
                })
                .collect();
            self.configure_roots_checked(&roots, g.config, false)?;
        }
        Ok(())
    }
    pub fn candidate_evidence(&self, board: Board, action: Action) -> Option<CandidateEvidence> {
        let (key, transform) = Key::new(board);
        let action = transform.action(action);
        self.nodes
            .get(&key)?
            .edges
            .iter()
            .find(|e| e.action == action)
            .map(|e| self.evidence(key, e))
    }
    // 採用順序の中心。手動値→確定した直接値→登録した子の値→直接値を基本とし、
    // その後に集合探索の結果と最終石差の共通範囲を確認する。
    // 子の推定値を、単に深さが大きいという理由で直接推定値へ戻さない。
    // 詳しい例と例外はdocs/book-v2/propagation.mdを参照。
    pub(super) fn evidence(&self, key: Key, edge: &Edge) -> CandidateEvidence {
        let propagated_id = if edge.registered {
            self.nodes
                .get(&edge.child)
                .and_then(|n| n.backups[usize::from(matches!(edge.action, Action::Place(_)))])
        } else {
            None
        };
        let propagated_certificate_id = if edge.registered {
            self.nodes.get(&edge.child).and_then(|n| {
                n.proof_backups
                    .as_ref()
                    .map(|values| values[usize::from(matches!(edge.action, Action::Place(_)))])
            })
        } else {
            None
        };
        let propagated = self.values.opt(propagated_id);
        let propagated_certificate = self.values.opt(propagated_certificate_id);
        let frontier = self.nodes[&key].frontier.as_ref().and_then(|f| {
            let index = self.nodes[&key]
                .edges
                .iter()
                .position(|e| e.action == edge.action)?;
            f.assessment(index, edge.action, &self.values)
        });
        let (mut effective, mut source) = if let Some(v) = self
            .values
            .opt(edge.manual())
            .filter(|v| v.identity() == self.identity)
        {
            (Some(v), Some(ValueSource::Manual))
        } else {
            match (self.values.opt(edge.direct()), propagated) {
                (Some(d), _) if d.quality().solved => (Some(d), Some(ValueSource::Direct)),
                (Some(d), Some(p)) if (!d.proven() || p.proven()) => {
                    (Some(p), Some(ValueSource::Propagated))
                }
                (Some(d), _) => (Some(d), Some(ValueSource::Direct)),
                (None, Some(p)) => (Some(p), Some(ValueSource::Propagated)),
                (None, None) => (None, None),
            }
        };
        if self.values.opt(edge.manual()).is_none_or(|v| v.identity() != self.identity)
            && let Some(f) = frontier
            // Book deliberately prefers even a shallower child backup. Only
            // certificates may override that choice, not estimated aggregates.
            && !(source == Some(ValueSource::Propagated)
                && !f.proven())
            && effective.is_none_or(|d| {
                if d.quality().solved {
                    return false;
                }
                if d.proven() {
                    // Retain independent bounds as evidence, while using a
                    // tighter certified upper to cover the maximum. An upper
                    // complements a certified lower without erasing it.
                    f.proven()
                        && (dominates(f, d)
                            || (f.bound() == Bound::ProvenUpper && d.bound() == Bound::ProvenLower))
                } else {
                    dominates(self.values.get(self.nodes[&key].frontier.as_ref().unwrap().value), d)
                }
            })
        {
            // A newer search of this subset supersedes old non-proven points
            // inside it, but its non-best moves receive upper bounds only.
            if frontier != effective {
                effective = Some(f);
                source = Some(ValueSource::Frontier);
            }
        }
        if self
            .values
            .opt(edge.manual())
            .is_none_or(|v| v.identity() != self.identity)
            && let Some(c) = self.values.opt(edge.certificate())
        {
            effective = Some(c);
            source = Some(ValueSource::Certificate);
        }
        CandidateEvidence {
            propagated_certificate,
            certificate: self.values.opt(edge.certificate()),
            frontier,
            registered: edge.registered,
            direct: self.values.opt(edge.direct()),
            manual: self.values.opt(edge.manual()),
            propagated,
            effective,
            source,
        }
    }
    pub(super) fn effective(&self, key: Key, edge: &Edge) -> Option<Assessment> {
        self.evidence(key, edge).effective
    }
    fn calculate(&mut self, key: Key) -> (Option<Assessment>, bool) {
        let n = &self.nodes[&key];
        if n.edges.is_empty() {
            return (Some(terminal_value(key.board())), true);
        }
        let inputs: Vec<_> = n.edges.iter().map(|e| self.effective(key, e)).collect();
        self.calculate_inputs(key, inputs)
    }
    fn calculate_proof(&mut self, key: Key, effective: Option<Assessment>) -> Option<Assessment> {
        if self.nodes[&key].edges.is_empty() {
            return Some(terminal_value(key.board()));
        }
        if self.nodes[&key]
            .edges
            .iter()
            .any(|e| e.certificate().is_none())
        {
            return None;
        }
        if self.nodes[&key].edges.iter().all(|e| {
            self.values
                .opt(e.manual())
                .is_none_or(|v| v.identity() != self.identity)
        }) {
            return effective;
        }
        let inputs = self.nodes[&key]
            .edges
            .iter()
            .map(|e| self.values.opt(e.certificate()))
            .collect();
        self.calculate_inputs(key, inputs).0
    }
    fn calculate_inputs(
        &mut self,
        key: Key,
        inputs: Vec<Option<Assessment>>,
    ) -> (Option<Assessment>, bool) {
        combine_values(inputs.iter().copied(), self.identity, key, self.propagation)
    }

    // 独立に得た最終石差の上下限は、採用値から外れても失わない。
    // 全情報の共通範囲を保存し、空になる矛盾は編集エラーとして巻き戻す。
    fn reconcile_certificates(&mut self, key: Key) -> Result<()> {
        let mut updates = Vec::new();
        for (i, edge) in self.nodes[&key].edges.iter().enumerate() {
            let ev = self.evidence(key, edge);
            let mut certificate = self.values.opt(edge.certificate());
            for value in [
                ev.direct,
                ev.frontier,
                ev.propagated,
                ev.propagated_certificate,
            ]
            .into_iter()
            .flatten()
            .filter(|v| v.proven())
            {
                certificate = Some(if let Some(old) = certificate {
                    let interval = old
                        .claim()
                        .certificate()
                        .unwrap()
                        .intersect(value.claim().certificate().unwrap())?;
                    if Some(interval) == old.claim().certificate() {
                        old
                    } else {
                        let mut summary = old.analysis().summary();
                        summary.include(value.analysis().summary());
                        Assessment::new(
                            ValueClaim::Certified(interval),
                            ValueAnalysis::Derived {
                                basis: Derivation::Current {
                                    root: [key.0, key.1],
                                    subject: CalculationSubject::Candidate(edge.action),
                                },
                                summary,
                            },
                            self.identity,
                        )?
                    }
                } else {
                    value
                });
            }
            updates.push((i, certificate));
        }
        for (i, v) in updates {
            let v = self.values.put(v);
            self.nodes.get_mut(&key).unwrap().edges[i].set_certificate(v);
        }
        Ok(())
    }
    fn reschedule_edit(&mut self, keys: &[Key]) {
        if let Some(g) = &mut self.generation {
            for &key in keys {
                if let Some(n) = self.nodes.get_mut(&key) {
                    n.completed_stages = 0;
                    n.pending = None;
                }
                for side in [BookSide::Black, BookSide::White] {
                    let c = generate::Context { key, side };
                    if let Some(&distance) = g.distances.get(&c) {
                        g.enqueue(c, 0, distance);
                        g.invalidate(key);
                    }
                }
            }
        }
    }
    // 子から親の順に再計算する。親への通知は値・全点評価の状態・確定情報が変化した時。
    // 途中の祖先で矛盾が見つかった場合も、それまでに触れた局面を全て戻す。
    pub(super) fn refresh_from(&mut self, key: Key) -> Result<Vec<Key>> {
        let mut affected = Vec::new();
        let mut todo = BTreeSet::new();
        todo.insert(order(key));
        let previous_bytes = self.workspace_bytes;
        let mut rollback = Vec::new();
        let result = (|| -> Result<()> {
            while let Some((_, _, key)) = todo.pop_first() {
                rollback.push((key, self.nodes[&key].clone()));
                affected.push(key);
                self.reconcile_certificates(key)?;
                let node = self.nodes.get_mut(&key).unwrap();
                let bytes = node.current_notes_bytes();
                self.workspace_bytes = self.workspace_bytes - node.notes_bytes + bytes;
                node.notes_bytes = bytes;
                let calculated = self.calculate(key);
                let proof = self.calculate_proof(key, calculated.0);
                let n = self.nodes.get_mut(&key).unwrap();
                if (self.values.opt(n.value), n.complete) != calculated
                    || self.values.opt(n.proof) != proof
                {
                    match (n.proof.is_some(), proof.is_some()) {
                        (false, true) => self.workspace_bytes += proof_cache_bytes(),
                        (true, false) => self.workspace_bytes -= proof_cache_bytes(),
                        _ => {}
                    }
                    n.proof = self.values.put(proof);
                    n.proof_backups = proof.map(|v| {
                        Box::new([
                            self.values.intern(v.child_to_parent(false)),
                            self.values.intern(v.child_to_parent(true)),
                        ])
                    });
                    (n.value, n.complete) = (self.values.put(calculated.0), calculated.1);
                    n.backups = [
                        n.value.map(|v| {
                            let v = self.values.get(v);
                            self.values.intern(v.child_to_parent(false))
                        }),
                        n.value.map(|v| {
                            let v = self.values.get(v);
                            self.values.intern(v.child_to_parent(true))
                        }),
                    ];
                    if let Some(parents) = self.parents.get(&key) {
                        for &p in parents {
                            todo.insert(order(p));
                        }
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.workspace_bytes = previous_bytes;
            for (key, node) in rollback.into_iter().rev() {
                self.nodes.insert(key, node);
            }
            return Err(error);
        }
        if let Some(g) = &mut self.generation {
            for &k in &affected {
                g.invalidate(k);
            }
        }
        Ok(affected)
    }
    /// 登録関係から親の一覧と現在値を全件再構築する。新しい探索は行わない。
    pub fn repair(&mut self) -> Result<()> {
        self.validate()?;
        self.rebuild_parents();
        let mut keys: Vec<_> = self.nodes.keys().copied().map(order).collect();
        keys.sort_unstable();
        for (_, _, k) in keys {
            self.reconcile_certificates(k)?;
            let node = self.nodes.get_mut(&k).unwrap();
            let bytes = node.current_notes_bytes();
            self.workspace_bytes = self.workspace_bytes - node.notes_bytes + bytes;
            node.notes_bytes = bytes;
            let (v, c) = self.calculate(k);
            let proof = self.calculate_proof(k, v);
            let n = self.nodes.get_mut(&k).unwrap();
            n.value = self.values.put(v);
            match (n.proof.is_some(), proof.is_some()) {
                (false, true) => self.workspace_bytes += proof_cache_bytes(),
                (true, false) => self.workspace_bytes -= proof_cache_bytes(),
                _ => {}
            }
            n.proof = self.values.put(proof);
            n.proof_backups = proof.map(|v| {
                Box::new([
                    self.values.intern(v.child_to_parent(false)),
                    self.values.intern(v.child_to_parent(true)),
                ])
            });
            n.backups = [
                v.map(|v| self.values.intern(v.child_to_parent(false))),
                v.map(|v| self.values.intern(v.child_to_parent(true))),
            ];
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
                let child = e.child;
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
                || !legal
                    .iter()
                    .zip(&n.edges)
                    .all(|(a, e)| *a == e.action && e.child == Key::new(board.play(*a).unwrap()).0)
            {
                return Err(invalid("candidate set is not canonical legal move list"));
            }
            for e in &n.edges {
                if let Some(v) = self.values.opt(e.certificate()) {
                    v.validate_candidate(key, e.action)?;
                    if !v.proven() {
                        return Err(invalid("non-certificate in proof slot"));
                    }
                }
                if let Some(v) = self.values.opt(e.direct()) {
                    v.validate_candidate(key, e.action)?;
                    if !v.proven() && v.identity() != self.identity {
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
                if let Some(v) = self.values.opt(e.manual()) {
                    v.validate_at(board)?;
                    if v.origin() != ValueOrigin::Manual {
                        return Err(invalid("manual provenance"));
                    }
                }
            }
            if let Some(f) = &n.frontier {
                let fv = self.values.get(f.value);
                fv.validate_at(board)?;
                if let ValueAnalysis::Search { trace, projection } = fv.analysis() {
                    let squares = n
                        .edges
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| f.mask & (1u64 << i) != 0)
                        .filter_map(|(_, e)| {
                            if let Action::Place(s) = e.action {
                                Some(1u64 << s.index())
                            } else {
                                None
                            }
                        })
                        .fold(0, |a, b| a | b);
                    if projection != SearchProjection::Maximum
                        || trace.root != [key.0, key.1]
                        || trace.moves != squares
                        || trace.best != action_code(f.best)
                    {
                        return Err(invalid("frontier search subject"));
                    }
                }
                let best = n
                    .edges
                    .iter()
                    .position(|e| e.action == f.best)
                    .ok_or_else(|| invalid("frontier action"))?;
                if f.mask == 0
                    || f.mask >> n.edges.len() != 0
                    || f.mask & (1u64 << best) == 0
                    || fv.bound() != Bound::Exact
                    || (!fv.proven() && fv.identity() != self.identity)
                    || f.pv.len() > 120
                    || (!f.pv.is_empty() && f.pv[0] != f.best)
                {
                    return Err(invalid("frontier coverage"));
                }
                let mut pv = board;
                for &a in &f.pv {
                    pv = pv.play(a).map_err(|_| invalid("frontier PV"))?;
                }
            }
            if let Some(p) = &n.pending {
                if p.values.len() != n.edges.len() || p.revision != n.revision {
                    return Err(invalid("stale pending stage"));
                }
                for id in p.values.iter().flatten() {
                    let v = self.values.get(*id);
                    v.validate_at(board)?;
                    if !v.proven() && v.identity() != self.identity {
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
// 空きマスが少ない子を先に処理する。パスでは空き数が減らないため、
// 同数ならパス先（must_pass=false）をパス元より先に置く。
pub(super) fn order(key: Key) -> (u8, bool, Key) {
    let b = key.board();
    (b.empty_count(), b.must_pass(), key)
}
pub(super) fn dominates(new: Assessment, old: Assessment) -> bool {
    if new.quality().solved {
        return true;
    }
    if old.quality().solved {
        return false;
    }
    if old.proven() {
        return new.proven()
            && match (new.bound(), old.bound()) {
                (Bound::ProvenUpper, Bound::ProvenUpper) => new.score() <= old.score(),
                (Bound::ProvenLower, Bound::ProvenLower) => new.score() >= old.score(),
                _ => false,
            };
    }
    if new.proven() {
        return true;
    }
    if new.identity() != old.identity() || new.bound() != Bound::Exact {
        return false;
    }
    if old.bound() != Bound::Exact {
        return true;
    }
    let q = old.analysis().summary();
    if q.families & (QualitySummary::LEGACY | QualitySummary::MANUAL) != 0 {
        // Compatibility selection retains the recorded requirements. It does
        // not reclassify the old evidence into a guessed search kind.
        return (new.quality().depth >= old.quality().depth
            || new.meets(SearchKind::Endgame {
                selectivity: old.quality().selectivity,
            }))
            && new.quality().selectivity >= old.quality().selectivity;
    }
    (!q.has_midgame()
        || new.meets(SearchKind::Midgame {
            depth: q.mid_depth,
            selectivity: q.midgame_selectivity().unwrap(),
        }))
        && (!q.has_endgame()
            || new.meets(SearchKind::Endgame {
                selectivity: q.endgame_selectivity().unwrap(),
            }))
}

fn proof_cache_bytes() -> u64 {
    (2 * std::mem::size_of::<ValueId>() + 32) as u64
}

#[cfg(test)]
mod shared_storage_tests {
    use super::*;
    use deft_reversi_engine::{EngineOptions, Goal, Model, PatternModel, SearchRequest};
    #[test]
    fn empty_candidates_allocate_no_notes_and_compaction_preserves_current_results() {
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let b = Board::new().canonical();
        let key = Key::new(b).0;
        let mut editor = BookEditor::for_engine(&engine).unwrap();
        editor.insert_position(b).unwrap();
        assert!(editor.nodes[&key].edges.iter().all(|e| e.notes.is_none()));
        for depth in 2..=4 {
            let r = engine
                .search(b, &SearchRequest::new(Goal::Depth(depth)))
                .unwrap();
            editor
                .record_search(
                    b,
                    RequestedAnalysis::ParentDepth {
                        depth,
                        selectivity: Selectivity::Full,
                    },
                    SearchKind::Midgame {
                        depth,
                        selectivity: Selectivity::Full,
                    },
                    &r,
                )
                .unwrap();
        }
        assert_eq!(
            editor.nodes[&key]
                .edges
                .iter()
                .filter(|e| e.notes.is_some())
                .count(),
            1
        );
        let before = editor
            .export()
            .unwrap()
            .probe(b)
            .unwrap()
            .candidates()
            .collect::<Vec<_>>();
        editor.compact_evidence().unwrap();
        let after = editor
            .export()
            .unwrap()
            .probe(b)
            .unwrap()
            .candidates()
            .collect::<Vec<_>>();
        assert_eq!(
            before
                .iter()
                .map(|c| (c.action, c.value, c.certificate))
                .collect::<Vec<_>>(),
            after
                .iter()
                .map(|c| (c.action, c.value, c.certificate))
                .collect::<Vec<_>>()
        );
        let mut bytes = Vec::new();
        editor.values.write(&mut bytes).unwrap();
        // Current max/best/upper views share one remaining search after collection.
        let mut trace_count = 0;
        let mut traces = BTreeSet::new();
        for v in editor.values.iter() {
            if let ValueAnalysis::Search { trace, .. } = v.analysis() {
                trace_count += 1;
                traces.insert(trace);
            }
        }
        assert_eq!(traces.len(), 1);
        assert!(trace_count >= 3);
        assert_eq!(
            editor.nodes[&key].notes_bytes,
            editor.nodes[&key].current_notes_bytes()
        );
    }
}
