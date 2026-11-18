//! 生成の設定、保存する仕事一覧、登録範囲と損失条件の判定。
//!
//! このファイルは「何を、どこまで育てるか」を担当し、探索の実行はgrow.rsへ渡す。
//! candidate_planがレベル／深さ指定をEngineの予定条件へ変換する唯一の場所。
//! active_pathsは新しい手を登録してよい根拠、analysis_distanceは既存の不明値を
//! 調べ直すための範囲であり、後者だけで新しい手を登録してはいけない。
use super::*;
use deft_reversi_engine::{Goal, ResultKind, SearchRequest, StopHandle};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Instant,
};
/// Absolute side is generation context, never part of the relative-board key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum BookSide {
    Black,
    White,
}
impl BookSide {
    pub(super) fn other(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct GenerationRoot {
    pub board: Board,
    pub side: BookSide,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
// 相対盤面は黒白を含まないが、色別の損失条件には手番の絶対色が必要。
// 同じ局面を二色の開始条件で育てる場合もNodeは共有し、仕事のContextだけを分ける。
pub(super) struct Context {
    pub key: Key,
    pub side: BookSide,
}
impl Context {
    pub(super) fn child(self, action: Action) -> Self {
        Self {
            key: Key::new(self.key.board().play(action).unwrap()).0,
            side: self.side.other(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    pub depth: u8,
    pub selectivity: Selectivity,
}
/// Exactly one input method. Levels retain the engine's current child-based
/// interpretation; only the planner translates it to an executable root search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisSchedule {
    Levels(Vec<u8>),
    Stages(Vec<Stage>),
}
impl AnalysisSchedule {
    pub fn levels(&self) -> Option<&Vec<u8>> {
        if let Self::Levels(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn stages(&self) -> Option<&Vec<Stage>> {
        if let Self::Stages(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn stages_mut(&mut self) -> Option<&mut Vec<Stage>> {
        if let Self::Stages(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Levels(v) => v.len(),
            Self::Stages(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Levels(v) => v.capacity(),
            Self::Stages(v) => v.capacity() * std::mem::size_of::<Stage>(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Levels(v)
                if v.is_empty()
                    || v.len() > 60
                    || v.iter().any(|l| !(1..=60).contains(l))
                    || v.windows(2).any(|l| l[0] >= l[1]) =>
            {
                Err(invalid("levels must strictly increase within 1..=60"))
            }
            Self::Stages(v)
                if v.is_empty()
                    || v.len() > 60
                    || v.iter().any(|s| !(2..=60).contains(&s.depth))
                    || v.windows(2).any(|s| {
                        s[0].depth >= s[1].depth || s[0].selectivity > s[1].selectivity
                    }) =>
            {
                Err(invalid(
                    "stages must increase in depth and not decrease in selectivity",
                ))
            }
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationConfig {
    /// 現在の条件を満たす完了済み評価を再利用する。falseでも確定情報は捨てない。
    pub reuse_completed: bool,
    /// 探索方法が不明な旧データを、記録された条件だけで再利用してよいか。
    pub allow_imported: bool,
    /// Recompute unregistered candidates over the saved registered graph only.
    /// Does not expand links and ignores loss filters during reachability.
    pub unregistered_only: bool,
    pub schedule: SchedulePolicy,
    /// Maximum consecutive best-line additions before returning to deviations.
    pub line_plies: u8,
    /// Zero chooses up to the engine's total thread and TT budget automatically.
    pub max_parallel_jobs: u16,
    /// レベル系列か深さ・選択率系列のどちらか一つ。段階番号はこの系列の添字。
    pub analysis: AnalysisSchedule,
    /// 現局面の最良値との差として許す1手の損失。色別指定があればそちらを使う。
    pub expansion_loss: u8,
    /// 黒の手番での1手損失。Noneはexpansion_lossを使う。
    pub black_loss: Option<u8>,
    /// 白の手番での1手損失。Noneはexpansion_lossを使う。
    pub white_loss: Option<u8>,
    /// Sum of positive local losses along a root path (both sides).
    pub cumulative_loss: Option<u16>,
    /// Independent threshold for adding an unregistered candidate as a link.
    pub leaf_loss: Option<u8>,
    pub new_job_weight: u8,
    pub deepen_job_weight: u8,
    /// 開始局面から石を置く手数の上限。パスは数えない。
    pub max_plies: u8,
    /// bookの局面・評価・仕事一覧等のメモリ見積もりの上限。EngineやTTは含まない。
    pub max_memory_bytes: u64,
    /// 実行中の保存コールバックを呼ぶ間隔。保存先への書込はコールバック側で行う。
    pub checkpoint_interval: Duration,
}
impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            reuse_completed: true,
            allow_imported: false,
            unregistered_only: false,
            schedule: SchedulePolicy::Breadth,
            line_plies: 8,
            max_parallel_jobs: 0,
            analysis: AnalysisSchedule::Stages(vec![
                Stage {
                    depth: 12,
                    selectivity: Selectivity::Percent95,
                },
                Stage {
                    depth: 16,
                    selectivity: Selectivity::Percent95,
                },
                Stage {
                    depth: 20,
                    selectivity: Selectivity::Percent95,
                },
            ]),
            expansion_loss: 2,
            black_loss: None,
            white_loss: None,
            cumulative_loss: None,
            leaf_loss: None,
            new_job_weight: 3,
            deepen_job_weight: 1,
            max_plies: 24,
            max_memory_bytes: 512 * 1024 * 1024,
            checkpoint_interval: Duration::from_secs(60),
        }
    }
}
impl GenerationConfig {
    pub(super) fn loss(&self, side: BookSide) -> u8 {
        match side {
            BookSide::Black => self.black_loss,
            BookSide::White => self.white_loss,
        }
        .unwrap_or(self.expansion_loss)
    }
    pub fn stage_count(&self) -> usize {
        self.analysis.len()
    }
    pub fn validate(&self) -> Result<()> {
        self.analysis.validate()?;
        if self.black_loss.is_some_and(|n| n > 128)
            || self.white_loss.is_some_and(|n| n > 128)
            || self.leaf_loss.is_some_and(|n| n > 128)
            || self.cumulative_loss.is_some_and(|n| n > 7680)
            || u16::from(self.new_job_weight) + u16::from(self.deepen_job_weight) == 0
            || self.expansion_loss > 128
            || self.max_plies > 60
            || self.line_plies > 60
            || self.max_parallel_jobs > 256
            || self.max_memory_bytes < 16384
            || self.checkpoint_interval.is_zero()
        {
            return Err(invalid("generation settings"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct GenerationLimits {
    pub time: Option<Duration>,
    pub nodes: Option<u64>,
    pub new_positions: Option<u64>,
}
impl GenerationLimits {
    pub(super) fn validate(self) -> Result<()> {
        if self.time.is_none() && self.nodes.is_none() && self.new_positions.is_none() {
            return Err(invalid("explicit generation budget required"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationStop {
    Exhausted,
    Requested,
    TimeLimit,
    NodeLimit,
    PositionLimit,
    MemoryLimit,
}
#[derive(Clone, Debug, Default)]
pub struct GenerationProfile {
    pub move_searches: u64,
    pub engine_wall: Duration,
    pub resources_wall: Duration,
    pub expansion_wall: Duration,
    pub scheduling_wall: Duration,
    pub planning_wall: Duration,
    pub reachability_wall: Duration,
    pub publish_wall: Duration,
    pub checkpoint_wall: Duration,
    pub max_parallel_jobs: usize,
    pub reachability_hits: u64,
    pub reachability_misses: u64,
}
#[derive(Clone, Debug)]
pub struct GenerationReport {
    pub reason: GenerationStop,
    pub nodes: u64,
    pub new_positions: u64,
    pub completed_stages: u64,
    pub elapsed: Duration,
    pub profile: GenerationProfile,
}
// Derived only; never serialized. Registration and value changes bump the editor
// revision. Adding a reachable context changes the membership epoch as well.
#[derive(Clone, Debug, Default)]
pub(super) struct ReachabilityCache {
    epoch: Option<(u64, usize)>,
    exact: BTreeMap<Context, Vec<PathCost>>,
    analysis: BTreeMap<Context, Vec<(u8, u16, bool)>>,
    pub hits: u64,
    pub misses: u64,
}
impl ReachabilityCache {
    fn sync(&mut self, editor: &BookEditor, state: &GenerationState) {
        let epoch = (editor.revision, state.distances.len());
        if self.epoch != Some(epoch) {
            self.exact.clear();
            self.analysis.clear();
            self.epoch = Some(epoch);
        }
    }
}
#[derive(Clone, Debug)]
/// 再開する仕事と、そこから作り直せる補助データ。
/// fresh/deepen/expansions/urgent/hintsは保存する。priority/reachability/expandedは
/// 読込後に作り直す。distancesの最短距離だけでは損失条件を満たす経路とは言えない。
pub(super) struct GenerationState {
    pub reachability: RefCell<ReachabilityCache>,
    pub config: GenerationConfig,
    pub roots: Vec<Context>,
    pub iteration: u64,
    /// 登録できる手を再確認する局面。探索待ちのfresh/deepenとは別の仕事。
    pub expansions: BTreeSet<(u8, Context)>,
    pub expanded: BTreeMap<Context, ExpansionStamp>,
    pub distances: BTreeMap<Context, u8>,
    /// 最初の段階の探索待ち。(開始局面からの石を置く手数, 対象)
    pub fresh: BTreeSet<(u8, Context)>,
    /// 次の段階の探索待ち。(段階番号, 石を置く手数, 対象)
    pub deepen: BTreeSet<(usize, u8, Context)>,
    pub priority: Option<super::schedule::PriorityIndex>,
    pub urgent: VecDeque<(Context, u8, usize, u8)>,
    pub hints: BTreeMap<Context, Action>,
    pub active_burst: u8,
}
impl GenerationState {
    pub(super) fn invalidate(&mut self, key: Key) {
        for side in [BookSide::Black, BookSide::White] {
            let c = Context { key, side };
            if let Some(&d) = self.distances.get(&c) {
                if let Some(index) = &mut self.priority {
                    index.invalidate(c);
                }
                self.expansions.insert((d, c));
            }
        }
    }
    pub(super) fn schedule(&mut self, key: Context, stages: usize, distance: u8) {
        if stages < self.config.stage_count() {
            self.enqueue(key, stages, distance);
        } else {
            self.expansions.insert((distance, key));
        }
    }
    pub fn estimated_bytes(&self) -> u64 {
        (self.expanded.len() as u64
            + self.expansions.len() as u64
            + self.distances.len() as u64
            + self.fresh.len() as u64
            + self.deepen.len() as u64)
            * 128
            + if self.config.schedule == SchedulePolicy::Promising {
                (self.fresh.len() + self.deepen.len() + self.distances.len()) as u64 * 512
            } else { 0 }
            + (self.urgent.capacity() as u64 * 64 + self.hints.len() as u64 * 128)
            + self.roots.capacity() as u64 * std::mem::size_of::<Context>() as u64
            + self.expanded.len() as u64
                * (61 * std::mem::size_of::<PathCost>()
                    + 64 * std::mem::size_of::<(Action, u16)>()) as u64
            // Reserve for the temporary ancestor reachability memo. Each context
            // has at most one minimal loss per placement distance (0..=60).
            + self.distances.len() as u64 * (256 + 61 * 4)
    }
    pub(super) fn enqueue(&mut self, key: Context, stages: usize, distance: u8) {
        if stages < self.config.stage_count()
            && let Some(index) = &mut self.priority
        {
            index.insert(key, distance, stages);
        }
        if stages == 0 {
            self.fresh.insert((distance, key));
        } else if stages < self.config.stage_count() {
            self.deepen.insert((stages, distance, key));
        }
    }
    pub(super) fn pop(&mut self, editor: &BookEditor) -> Option<(Context, u8, usize)> {
        self.active_burst = self.config.line_plies;
        while self.iteration % 16 != 15
            && let Some((c, d, s, left)) = self.urgent.pop_front()
        {
            let queued = if s == 0 {
                self.fresh.remove(&(d, c))
            } else {
                self.deepen.remove(&(s, d, c))
            };
            if queued {
                if let Some(index) = &mut self.priority {
                    index.remove(c, d, s);
                }
                self.active_burst = left;
                return Some((c, d, s));
            }
        }
        let total =
            u64::from(self.config.new_job_weight) + u64::from(self.config.deepen_job_weight);
        let prefer_new = self.iteration % total < u64::from(self.config.new_job_weight);
        // One in sixteen jobs retains ordinary breadth/stage order for fairness.
        let selected =
            if self.config.schedule == SchedulePolicy::Promising && self.iteration % 16 != 15 {
                self.priority_job(editor, prefer_new)
            } else {
                if prefer_new {
                    self.fresh.first().map(|&(d, c)| (c, d, 0))
                } else {
                    self.deepen.first().map(|&(s, d, c)| (c, d, s))
                }
                .or_else(|| self.deepen.first().map(|&(s, d, c)| (c, d, s)))
                .or_else(|| self.fresh.first().map(|&(d, c)| (c, d, 0)))
            };
        if let Some((c, d, s)) = selected {
            if s == 0 {
                self.fresh.remove(&(d, c));
            } else {
                self.deepen.remove(&(s, d, c));
            }
            if let Some(index) = &mut self.priority {
                index.remove(c, d, s);
            }
        }
        selected
    }
}
impl BookEditor {
    /// Public graph edits happen while the generation state is in the editor;
    /// internal generation owns that state separately and queues its own links.
    /// Walk the retained subgraph too: a newly connected node may already have
    /// registered descendants or may shorten their geometric root distances.
    pub(super) fn enqueue_registered_link(&mut self, parent: Key, action: Action) {
        let Some(state) = &mut self.generation else {
            return;
        };
        let child = self.nodes[&parent]
            .edges
            .iter()
            .find(|e| e.action == action)
            .unwrap()
            .child;
        let placement = u8::from(matches!(action, Action::Place(_)));
        let mut todo: VecDeque<_> = [BookSide::Black, BookSide::White]
            .into_iter()
            .filter_map(|side| {
                state
                    .distances
                    .get(&Context { key: parent, side })
                    .map(|&d| {
                        (
                            Context {
                                key: child,
                                side: side.other(),
                            },
                            d + placement,
                        )
                    })
            })
            .collect();
        let mut visited = BTreeMap::new();
        while let Some((context, distance)) = todo.pop_front() {
            if distance > state.config.max_plies {
                continue;
            }
            let distance = state
                .distances
                .get(&context)
                .map_or(distance, |&old| old.min(distance));
            if visited.get(&context).is_some_and(|&old| old <= distance) {
                continue;
            }
            visited.insert(context, distance);
            state.distances.insert(context, distance);
            let node = &self.nodes[&context.key];
            if !context.key.board().is_terminal() {
                state.expanded.remove(&context);
                state.schedule(context, node.completed_stages, distance);
                state.expansions.insert((distance, context));
            }
            for edge in node.edges.iter().filter(|e| e.registered) {
                todo.push_back((
                    Context {
                        key: edge.child,
                        side: context.side.other(),
                    },
                    distance + u8::from(matches!(edge.action, Action::Place(_))),
                ));
            }
        }
    }
    /// Replace the scheduling configuration. Completed compatible analyses are retained;
    /// stage publication restarts, and the serialized old work queue is replaced.
    /// 開始局面の手番をすべて黒として設定する簡易入口。
    /// 色別の損失条件を使う場合はconfigure_generation_rootsで色も明示する。
    pub fn configure_generation(
        &mut self,
        roots: &[Board],
        config: GenerationConfig,
    ) -> Result<()> {
        let roots: Vec<_> = roots
            .iter()
            .map(|&board| GenerationRoot {
                board,
                side: BookSide::Black,
            })
            .collect();
        self.configure_generation_roots(&roots, config)
    }
    /// 開始局面・手番の絶対色・生成設定を設定し、既存の登録範囲から仕事を作り直す。
    /// 完了済みの評価は残すが、段階の完了印と古い仕事一覧は引き継がない。
    pub fn configure_generation_roots(
        &mut self,
        roots: &[GenerationRoot],
        config: GenerationConfig,
    ) -> Result<()> {
        self.configure_roots_checked(roots, config, true)
    }
    pub(super) fn configure_roots_checked(
        &mut self,
        roots: &[GenerationRoot],
        config: GenerationConfig,
        check_memory: bool,
    ) -> Result<()> {
        config.validate()?;
        if roots.is_empty() {
            return Err(invalid("at least one starting position required"));
        }
        let mut keys: Vec<_> = roots
            .iter()
            .map(|r| Context {
                key: Key::new(r.board).0,
                side: r.side,
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        // Count before inserting roots so a small memory budget cannot be bypassed.
        if check_memory
            && self
                .estimated_bytes()
                .saturating_add(keys.len() as u64 * 16384)
                > config.max_memory_bytes
        {
            return Err(BookError::Limit("generation memory"));
        }
        if keys.iter().any(|c| c.key.board().empty_count() > 60) {
            return Err(invalid("starting position has fewer than four stones"));
        }
        for &k in &keys {
            self.ensure(k.key)?;
        }
        for n in self.nodes.values_mut() {
            n.completed_stages = 0;
        }
        let mut state = GenerationState {
            reachability: RefCell::default(),
            config,
            priority: None,
            urgent: VecDeque::new(),
            hints: BTreeMap::new(),
            active_burst: 0,
            roots: keys.clone(),
            iteration: 0,
            expansions: BTreeSet::new(),
            expanded: BTreeMap::new(),
            distances: BTreeMap::new(),
            fresh: BTreeSet::new(),
            deepen: BTreeSet::new(),
        };
        let mut todo: VecDeque<_> = keys.into_iter().map(|k| (k, 0)).collect();
        while let Some((k, d)) = todo.pop_front() {
            if state.distances.get(&k).is_some_and(|old| *old <= d) {
                continue;
            }
            state.distances.insert(k, d);
            if !k.key.board().is_terminal() {
                state.enqueue(k, 0, d);
            }
            for e in self.nodes[&k.key].edges.iter().filter(|e| e.registered) {
                let next = d + u8::from(matches!(e.action, Action::Place(_)));
                if next <= state.config.max_plies {
                    todo.push_back((k.child(e.action), next));
                }
            }
        }
        self.extensions.clear();
        self.generation = Some(state);
        Ok(())
    }
    pub fn generation_config(&self) -> Option<&GenerationConfig> {
        self.generation.as_ref().map(|g| &g.config)
    }
    pub fn generate(
        &mut self,
        engine: &mut Engine,
        limits: GenerationLimits,
        stop: StopHandle,
    ) -> Result<GenerationReport> {
        self.generate_with_checkpoint(engine, limits, stop, |_, _| Ok(()))
    }
    /// 探索の合間と正常終了時に、一貫した状態のeditorをコールバックへ渡す。
    /// 保存するならコールバック内でsaveを呼ぶ。中断探索の途中値は候補へ採用せず、
    /// 診断用の中断記録と再実行する仕事を残す。エラー時の最終コールバックは保証しない。
    pub fn generate_with_checkpoint(
        &mut self,
        engine: &mut Engine,
        limits: GenerationLimits,
        stop: StopHandle,
        mut checkpoint: impl FnMut(&BookEditor, &GenerationReport) -> Result<()>,
    ) -> Result<GenerationReport> {
        super::grow::generate(self, engine, limits, stop, &mut checkpoint)
    }
}

pub(super) fn budget_reason(
    l: GenerationLimits,
    r: &GenerationReport,
    start: Instant,
    stop: &StopHandle,
) -> Option<GenerationStop> {
    if stop.is_stopped() {
        Some(GenerationStop::Requested)
    } else if l.time.is_some_and(|t| start.elapsed() >= t) {
        Some(GenerationStop::TimeLimit)
    } else if l.nodes.is_some_and(|n| r.nodes >= n) {
        Some(GenerationStop::NodeLimit)
    } else {
        None
    }
}
// 新規登録を許す最終判定。毎回active_pathsと現在値を確認する。
// 一つ登録しただけで合流先の子から値が戻り、残り候補の損失が変わる場合がある。
pub(super) fn expand(
    editor: &mut BookEditor,
    state: &mut GenerationState,
    context: Context,
    _distance: u8,
    limits: GenerationLimits,
    report: &mut GenerationReport,
) -> Result<()> {
    let key = context.key;
    let paths = active_paths(editor, state, context);
    let mask = eligible_mask(editor, key, state.config.loss(context.side));
    let actions: Vec<_> = editor.nodes[&key]
        .edges
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1u64 << i) != 0)
        .filter_map(|(_, e)| {
            let loss = local_loss(editor, key, e.action)?;
            if !e.registered
                && state
                    .config
                    .leaf_loss
                    .is_some_and(|limit| loss > u16::from(limit))
            {
                return None;
            }
            paths
                .iter()
                .any(|p| advance(*p, e.action, loss, &state.config).is_some())
                .then_some((e.action, loss))
        })
        .collect();
    let stamp = ExpansionStamp {
        paths: paths.clone(),
        actions: actions.clone(),
    };
    if state.expanded.get(&context) == Some(&stamp) {
        return Ok(());
    }
    for (action, _) in actions {
        // Registering a transposed complete child can immediately change this
        // node and its ancestors. Recheck the remaining batch against those
        // current values rather than adding newly ineligible stale candidates.
        let edge = editor.nodes[&key]
            .edges
            .iter()
            .find(|e| e.action == action)
            .unwrap();
        let Some(loss) = local_loss(editor, key, action) else {
            continue;
        };
        if loss > u16::from(state.config.loss(context.side))
            || (!edge.registered
                && state
                    .config
                    .leaf_loss
                    .is_some_and(|limit| loss > u16::from(limit)))
        {
            continue;
        }
        let Some(d) = active_paths(editor, state, context)
            .iter()
            .filter_map(|p| advance(*p, action, loss, &state.config))
            .map(|p| p.distance)
            .min()
        else {
            continue;
        };
        let child = Key::new(key.board().play(action).unwrap()).0;
        if !editor.nodes.contains_key(&child) {
            if limits
                .new_positions
                .is_some_and(|n| report.new_positions >= n)
            {
                report.reason = GenerationStop::PositionLimit;
                return Ok(());
            }
            if editor
                .estimated_bytes()
                .saturating_add(state.estimated_bytes())
                .saturating_add(16384)
                > state.config.max_memory_bytes
            {
                report.reason = GenerationStop::MemoryLimit;
                return Ok(());
            }
            editor.ensure(child)?;
            report.new_positions += 1;
        }
        let was_registered = editor.nodes[&key]
            .edges
            .iter()
            .find(|e| e.action == action)
            .unwrap()
            .registered;
        for affected in editor.register(key, action)? {
            state.invalidate(affected);
        }
        if !was_registered {
            state
                .expansions
                .insert((state.distances[&context], context));
        }
        let child_context = context.child(action);
        if state
            .distances
            .get(&child_context)
            .is_none_or(|old| *old > d)
        {
            state.distances.insert(child_context, d);
        }
        let queued_distance = state.distances[&child_context];
        if !child.board().is_terminal() {
            state.schedule(
                child_context,
                editor.nodes[&child].completed_stages,
                queued_distance,
            );
        }
    }
    state.expanded.insert(context, stamp);
    Ok(())
}

pub(super) fn eligible_mask(editor: &BookEditor, key: Key, loss: u8) -> u64 {
    let n = &editor.nodes[&key];
    if editor
        .values
        .opt(n.value)
        .is_none_or(|v| v.bound() != Bound::Point)
    {
        return 0;
    }
    let best = editor.values.opt(n.value).unwrap().score();
    n.edges.iter().enumerate().fold(0, |mask, (i, e)| {
        if editor
            .selected(key, e)
            .is_some_and(|v| v.bound() == Bound::Point && v.score() >= best - i16::from(loss))
        {
            mask | (1u64 << i)
        } else {
            mask
        }
    })
}
// The shortest path and the cheapest path need not be the same when several
// roots or transpositions meet. Keep the nondominated (distance, loss) frontier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 開始局面からの一経路の「石を置く手数」と「累積損失」。パスは手数を増やさない。
pub(super) struct PathCost {
    pub distance: u8,
    pub loss: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExpansionStamp {
    paths: Vec<PathCost>,
    actions: Vec<(Action, u16)>,
}
pub(super) fn local_loss(editor: &BookEditor, key: Key, action: Action) -> Option<u16> {
    let node = &editor.nodes[&key];
    let best = editor
        .values
        .opt(node.value)
        .filter(|v| v.bound() == Bound::Point)?
        .score();
    let value = node
        .edges
        .iter()
        .find(|e| e.action == action)
        .and_then(|e| editor.selected(key, e))?;
    (value.bound() == Bound::Point).then_some((best - value.score()).max(0) as u16)
}
fn advance(
    path: PathCost,
    action: Action,
    loss: u16,
    config: &GenerationConfig,
) -> Option<PathCost> {
    let next = PathCost {
        distance: path.distance + u8::from(matches!(action, Action::Place(_))),
        loss: if config.cumulative_loss.is_some() {
            path.loss + loss
        } else {
            0
        },
    };
    (next.distance <= config.max_plies
        && config
            .cumulative_loss
            .is_none_or(|limit| next.loss <= limit))
    .then_some(next)
}
// 同じ局面に「短いが損失が大きい経路」と「長いが損失が小さい経路」が合流し得る。
// 一方の最小値だけでは登録の可否を誤るので、両条件で劣る経路だけを捨てる。
fn pareto(paths: &mut Vec<PathCost>) {
    paths.sort_unstable_by_key(|p| (p.distance, p.loss));
    let mut cheapest = u16::MAX;
    paths.retain(|p| {
        if p.loss < cheapest {
            cheapest = p.loss;
            true
        } else {
            false
        }
    });
}
// 開始局面から登録済みの手を通り、現在の損失・手数制限を満たす実在の経路を返す。
// 空なら新しい子を登録できない。キャッシュは局面の編集世代が変われば無効になる。
pub(super) fn active_paths(
    editor: &BookEditor,
    state: &GenerationState,
    context: Context,
) -> Vec<PathCost> {
    fn visit(
        editor: &BookEditor,
        state: &GenerationState,
        context: Context,
        memo: &mut BTreeMap<Context, Vec<PathCost>>,
    ) -> Vec<PathCost> {
        if state.roots.binary_search(&context).is_ok() {
            return vec![PathCost {
                distance: 0,
                loss: 0,
            }];
        }
        if let Some(v) = memo.get(&context) {
            return v.clone();
        }
        let mut paths = Vec::new();
        if let Some(parents) = editor.parents.get(&context.key) {
            for &parent in parents {
                let parent_context = Context {
                    key: parent,
                    side: context.side.other(),
                };
                if !state.distances.contains_key(&parent_context) {
                    continue;
                }
                let mask = if state.config.unregistered_only {
                    u64::MAX
                } else {
                    eligible_mask(editor, parent, state.config.loss(parent_context.side))
                };
                for (i, e) in editor.nodes[&parent].edges.iter().enumerate() {
                    if !e.registered
                        || mask & (1u64 << i) == 0
                        || (Context {
                            key: e.child,
                            side: parent_context.side.other(),
                        }) != context
                    {
                        continue;
                    }
                    let loss = if state.config.unregistered_only {
                        0
                    } else {
                        local_loss(editor, parent, e.action).unwrap()
                    };
                    for p in visit(editor, state, parent_context, memo) {
                        if let Some(p) = advance(p, e.action, loss, &state.config) {
                            paths.push(p);
                        }
                    }
                    pareto(&mut paths);
                }
            }
        }
        memo.insert(context, paths.clone());
        paths
    }
    let mut cache = state.reachability.borrow_mut();
    cache.sync(editor, state);
    if cache.exact.contains_key(&context) {
        cache.hits += 1;
    } else {
        cache.misses += 1;
    }
    visit(editor, state, context, &mut cache.exact)
}
pub(super) fn active_distance(
    editor: &BookEditor,
    state: &GenerationState,
    context: Context,
) -> Option<u8> {
    active_paths(editor, state, context)
        .first()
        .map(|p| p.distance)
}

/// Existing unknown branches may be analyzed to restore their values, but only
/// exact active_paths can authorize the registration of new descendants.
pub(super) fn analysis_distance(
    editor: &BookEditor,
    state: &GenerationState,
    context: Context,
) -> Option<u8> {
    type Path = (u8, u16, bool);
    fn visit(
        editor: &BookEditor,
        state: &GenerationState,
        c: Context,
        memo: &mut BTreeMap<Context, Vec<Path>>,
    ) -> Vec<Path> {
        if state.roots.binary_search(&c).is_ok() {
            return vec![(0, 0, false)];
        }
        if let Some(paths) = memo.get(&c) {
            return paths.clone();
        }
        let mut paths = Vec::new();
        if let Some(parents) = editor.parents.get(&c.key) {
            for &key in parents {
                let parent = Context {
                    key,
                    side: c.side.other(),
                };
                if !state.distances.contains_key(&parent) {
                    continue;
                }
                for edge in editor.nodes[&key].edges.iter().filter(|e| e.registered) {
                    if (Context {
                        key: edge.child,
                        side: parent.side.other(),
                    }) != c
                    {
                        continue;
                    }
                    let (loss, uncertain) = if state.config.unregistered_only {
                        (0, false)
                    } else {
                        match (
                            editor.values.opt(editor.nodes[&key].value),
                            editor.selected(key, edge),
                        ) {
                            (Some(v), Some(e))
                                if v.bound() == Bound::Point
                                    && matches!(e.bound(), Bound::Point | Bound::Upper) =>
                            {
                                (
                                    (v.score() - e.score()).max(0) as u16,
                                    e.bound() != Bound::Point,
                                )
                            }
                            _ => (0, true),
                        }
                    };
                    if loss > u16::from(state.config.loss(parent.side)) {
                        continue;
                    }
                    for (d, l, u) in visit(editor, state, parent, memo) {
                        let d = d + u8::from(matches!(edge.action, Action::Place(_)));
                        let l = if state.config.cumulative_loss.is_some() {
                            l + loss
                        } else {
                            0
                        };
                        if d <= state.config.max_plies
                            && state.config.cumulative_loss.is_none_or(|limit| l <= limit)
                        {
                            paths.push((d, l, u || uncertain));
                        }
                    }
                    paths.sort_unstable();
                    let mut minima = [u16::MAX; 2];
                    paths.retain(|&(_, loss, uncertain)| {
                        let old = &mut minima[usize::from(uncertain)];
                        if loss < *old {
                            *old = loss;
                            true
                        } else {
                            false
                        }
                    });
                }
            }
        }
        memo.insert(c, paths.clone());
        paths
    }
    active_distance(editor, state, context).or_else(|| {
        let mut cache = state.reachability.borrow_mut();
        cache.sync(editor, state);
        if cache.analysis.contains_key(&context) {
            cache.hits += 1;
        } else {
            cache.misses += 1;
        }
        visit(editor, state, context, &mut cache.analysis)
            .into_iter()
            .filter(|p| p.2)
            .map(|p| p.0)
            .min()
    })
}

// Resolve at the child, using the engine's own table. Fixed-depth stages retain
// their historical exact-endgame behavior for compatibility.
// 現在のLevelは「候補を打った子」で解決し、石を置く手なら深さ+1して親へ戻す。
// Stageは最初から親の深さ。grow::prepareはここで求めた条件で親の集合探索を組む。
// 将来の「中盤レベル=同じ探索元での深さ」は現時点では実装していない。
pub(super) fn candidate_plan(
    engine: &Engine,
    board: Board,
    action: Action,
    config: &GenerationConfig,
    index: usize,
) -> Result<(SearchRequest, Stage)> {
    let child = board.play(action).unwrap();
    let placement = u8::from(matches!(action, Action::Place(_)));
    if child.is_terminal() {
        return Ok((
            SearchRequest::new(Goal::Depth(0)),
            Stage {
                depth: terminal_value(child)
                    .child_to_parent(placement != 0)
                    .depth(),
                selectivity: Selectivity::Full,
            },
        ));
    }
    let request = if let Some(levels) = config.analysis.levels() {
        SearchRequest::new(Goal::Level(levels[index]))
    } else {
        let stage = config.analysis.stages().unwrap()[index];
        let depth = stage.depth - placement;
        if depth >= child.empty_count() {
            SearchRequest::new(Goal::Endgame).selectivity(stage.selectivity)
        } else {
            SearchRequest::new(Goal::Depth(depth)).selectivity(stage.selectivity)
        }
    };
    let plan = engine.search_plan(child, &request)?;
    let depth = match plan {
        ResultKind::Depth { depth, .. } => depth,
        ResultKind::Endgame { .. } => child.empty_count(),
    };
    Ok((
        request,
        Stage {
            depth: (depth + placement).min(60),
            selectivity: plan.selectivity(),
        },
    ))
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use crate::legacy::{AnalysisQuality, ImportedValue, ValueOrigin};
    use std::collections::BTreeMap;

    fn value(score: i16) -> Evaluation {
        Evaluation::import(ImportedValue {
            score,
            quality: AnalysisQuality {
                depth: 50,
                selectivity: Selectivity::Full,
                solved: false,
            },
            bound: legacy::Bound::Exact,
            identity: [0; 32],
            origin: ValueOrigin::Search,
        })
        .unwrap()
    }
    // All path steps are in canonical coordinates, as are the stored edges.
    type Steps = Vec<(Context, Action)>;
    fn diamond(
        c: Context,
        steps: &mut Steps,
        seen: &mut BTreeMap<Context, Steps>,
    ) -> Option<(Steps, Steps, Context)> {
        if steps.len() == 4 {
            if let Some(old) = seen.get(&c) {
                let short_root = old[2].0;
                if !steps.iter().any(|(p, _)| *p == short_root)
                    && legal_actions(short_root.key.board()).len() > 1
                {
                    return Some((old.clone(), steps.clone(), c));
                }
            } else {
                seen.insert(c, steps.clone());
            }
            return None;
        }
        for a in legal_actions(c.key.board()) {
            steps.push((c, a));
            if let Some(found) = diamond(c.child(a), steps, seen) {
                return Some(found);
            }
            steps.pop();
        }
        None
    }
    fn random_position(mut seed: u64, empties: u8) -> Board {
        let mut b = Board::new();
        while b.empty_count() > empties && !b.is_terminal() {
            let moves = legal_actions(b);
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            b = b.play(moves[seed as usize % moves.len()]).unwrap();
        }
        b
    }
    fn fill(editor: &mut BookEditor, c: Context) {
        for a in legal_actions(c.key.board()) {
            editor.record_candidate(c.key.board(), a, value(0)).unwrap();
        }
    }
    #[test]
    fn transposition_retains_short_costly_and_long_cheap_root_paths() {
        let board = random_position(94, 30);
        let root = Context {
            key: Key::new(board).0,
            side: BookSide::Black,
        };
        let (short, long, target) =
            diamond(root, &mut Vec::new(), &mut BTreeMap::new()).expect("four-ply transposition");
        let mut editor = BookEditor::new([0; 32]);
        let short = &short[2..];
        for &(c, a) in short.iter().chain(&long) {
            fill(&mut editor, c);
            editor.register(c.key, a).unwrap();
        }
        fill(&mut editor, target);
        // The short root is absent from the long path, so only the short
        // continuation pays three points. Manual evidence keeps it independent
        // of values_for_parent, allowing the reachability rule to be tested directly.
        let mut v = ImportedValue::from(value(-3));
        v.origin = ValueOrigin::Manual;
        editor
            .record_imported(short[0].0.key.board(), short[0].1, v)
            .unwrap();
        editor
            .configure_generation_roots(
                &[
                    GenerationRoot {
                        board,
                        side: BookSide::Black,
                    },
                    GenerationRoot {
                        board: short[0].0.key.board(),
                        side: short[0].0.side,
                    },
                ],
                GenerationConfig {
                    max_plies: 4,
                    expansion_loss: 3,
                    cumulative_loss: Some(3),
                    ..Default::default()
                },
            )
            .unwrap();
        let state = editor.generation.as_ref().unwrap();
        assert_eq!(
            active_paths(&editor, state, target),
            vec![
                PathCost {
                    distance: 2,
                    loss: 3
                },
                PathCost {
                    distance: 4,
                    loss: 0
                },
            ]
        );
        let cfg = &state.config;
        assert!(
            advance(
                PathCost {
                    distance: 2,
                    loss: 3
                },
                Action::Place(board.legal_moves().next().unwrap()),
                1,
                cfg
            )
            .is_none()
        );
        assert!(
            advance(
                PathCost {
                    distance: 4,
                    loss: 0
                },
                Action::Place(board.legal_moves().next().unwrap()),
                1,
                cfg
            )
            .is_none()
        );
    }
    #[test]
    fn expansion_rechecks_other_candidates_after_a_transposition_backup_changes_the_best() {
        let board = Key::new(random_position(94, 30)).0.board();
        let c = Context {
            key: Key::new(board).0,
            side: BookSide::Black,
        };
        let actions = legal_actions(board);
        let first = actions[0];
        let child = c.child(first);
        assert!(actions.iter().skip(1).all(|&a| c.child(a) != child));
        let mut editor = BookEditor::new([0; 32]);
        fill(&mut editor, c);
        fill(&mut editor, child);
        for a in legal_actions(child.key.board()) {
            editor
                .record_candidate(child.key.board(), a, value(-20))
                .unwrap();
        }
        editor
            .configure_generation(
                &[board],
                GenerationConfig {
                    max_plies: 1,
                    expansion_loss: 0,
                    ..Default::default()
                },
            )
            .unwrap();
        let mut state = editor.generation.take().unwrap();
        let mut report = GenerationReport {
            reason: GenerationStop::Exhausted,
            nodes: 0,
            new_positions: 0,
            completed_stages: 0,
            elapsed: Duration::ZERO,
            profile: GenerationProfile::default(),
        };
        expand(
            &mut editor,
            &mut state,
            c,
            0,
            GenerationLimits {
                new_positions: Some(100),
                ..Default::default()
            },
            &mut report,
        )
        .unwrap();
        assert_eq!(
            editor
                .move_values(board, first)
                .unwrap()
                .selected
                .unwrap()
                .score(),
            20
        );
        assert_eq!(
            editor.nodes[&c.key]
                .edges
                .iter()
                .filter(|e| e.registered)
                .count(),
            1
        );
    }
    #[test]
    fn interrupted_subset_does_not_publish_partial_evidence_and_link_edit_remains_valid() {
        use deft_reversi_engine::{EngineOptions, Model, PatternModel};
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let board = Board::new();
        let key = Key::new(board).0;
        let mut editor = BookEditor::for_engine(&engine).unwrap();
        editor
            .configure_generation(
                &[board],
                GenerationConfig {
                    analysis: AnalysisSchedule::Stages(vec![Stage {
                        depth: 2,
                        selectivity: Selectivity::Full,
                    }]),
                    max_plies: 0,
                    ..Default::default()
                },
            )
            .unwrap();
        editor
            .generate(
                &mut engine,
                GenerationLimits {
                    nodes: Some(1),
                    ..Default::default()
                },
                StopHandle::new(),
            )
            .unwrap();

        assert!(editor.nodes[&key].move_search.is_none());
        editor
            .register_move(board, legal_actions(board)[0])
            .unwrap();

        editor.validate().unwrap();
    }
    #[test]
    fn external_pass_link_queues_both_sides_without_using_placement_budget() {
        let board = (0..1000)
            .map(|seed| random_position(seed, 3))
            .find(|b| b.must_pass())
            .expect("forced pass");
        let mut editor = BookEditor::new([0; 32]);
        editor
            .configure_generation_roots(
                &[
                    GenerationRoot {
                        board,
                        side: BookSide::Black,
                    },
                    GenerationRoot {
                        board,
                        side: BookSide::White,
                    },
                ],
                GenerationConfig {
                    max_plies: 0,
                    ..Default::default()
                },
            )
            .unwrap();
        editor.register_move(board, Action::Pass).unwrap();
        let child = board.play(Action::Pass).unwrap();
        let key = Key::new(child).0;
        for side in [BookSide::Black, BookSide::White] {
            let context = Context { key, side };
            let state = editor.generation.as_ref().unwrap();
            assert_eq!(state.distances.get(&context), Some(&0));
            assert!(state.fresh.contains(&(0, context)));
        }
        let reply = legal_actions(child)[0];
        editor.register_move(child, reply).unwrap();
        let beyond = Key::new(child.play(reply).unwrap()).0;
        let state = editor.generation.as_ref().unwrap();
        assert!(state.distances.keys().all(|c| c.key != beyond));
    }
    #[test]
    fn pass_keeps_distance_and_budget_while_switching_side() {
        let board = (0..1000)
            .map(|seed| random_position(seed, 2))
            .find(|b| b.must_pass())
            .expect("forced pass");
        let c = Context {
            key: Key::new(board).0,
            side: BookSide::Black,
        };
        let child = c.child(Action::Pass);
        assert_eq!(child.side, BookSide::White);
        let mut editor = BookEditor::new([0; 32]);
        fill(&mut editor, c);
        editor.register(c.key, Action::Pass).unwrap();
        editor
            .configure_generation_roots(
                &[GenerationRoot {
                    board,
                    side: BookSide::Black,
                }],
                GenerationConfig {
                    cumulative_loss: Some(0),
                    max_plies: 0,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            active_paths(&editor, editor.generation.as_ref().unwrap(), child),
            vec![PathCost {
                distance: 0,
                loss: 0
            }]
        );
    }
}
