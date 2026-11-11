//! Single generation strategy: retained registered evidence plus the best
//! remaining root move and upper coverage for the rest. No all-move scan mode.
//!
//! 一巡の流れはprepare（対象手集合を決める）→BatchPool::run（探索）→publish
//! （完了結果を反映）→expand_and_schedule（登録と次の仕事）。
//! 仕事一覧・損失条件はgenerate.rs、採用する候補値と親への更新はeditor.rsを参照。
use super::editor::{MoveSearch, dominates};
use super::generate::*;
use super::*;
use deft_reversi_engine::{
    EngineOptions, Goal, SearchRequest, SearchResult, StopHandle, StopReason,
};
use std::time::Instant;

// 探索開始時点の対象と条件。ValueIdは保持しないため、評価一覧の圧縮後も参照が壊れない。
struct Work {
    context: Context,
    distance: u8,
    stage: usize,
    revision: u64,
    // maskは合法手配列の添字、request.root_movesは盤面のマス番号。両者は別の番号体系。
    mask: u64,
    burst: u8,
    request: SearchRequest,
    requested: RequestedAnalysis,
    planned: SearchKind,
}
fn quality_ok(
    v: Evaluation,
    identity: AnalysisIdentity,
    target: Stage,
    empties: u8,
    allow_imported: bool,
) -> bool {
    if v.is_final() {
        return true;
    }
    if v.identity() != identity {
        return false;
    }
    let kind = if target.depth >= empties {
        SearchKind::Endgame {
            selectivity: target.selectivity,
        }
    } else {
        SearchKind::Midgame {
            depth: target.depth,
            selectivity: target.selectivity,
        }
    };
    v.meets(kind)
        || allow_imported
            && target.depth < empties
            && v.conditions().has_imported()
            && v.depth() >= target.depth
            && v.selectivity() >= target.selectivity
}

// 登録した子の評価を再利用し、足りない候補を一回の集合探索にまとめる。
// Noneは「新しい探索は不要」であって「全候補の点数を個別に求めた」ではない。
// 品質が足りる点評価と残りの上限で最良手が決まれば、劣る全候補を個別探索しない。
fn prepare(
    engine: &Engine,
    editor: &BookEditor,
    state: &GenerationState,
    context: Context,
    distance: u8,
    stage: usize,
    reuse: bool,
) -> Result<Option<Work>> {
    let node = &editor.nodes[&context.key];
    if node.edges.is_empty() {
        return Ok(None);
    }
    let board = context.key.board();
    let action = node
        .edges
        .iter()
        .map(|e| e.action)
        .find(|&a| !board.play(a).unwrap().is_terminal())
        .unwrap_or(node.edges[0].action);
    let (_, mut target) = candidate_plan(engine, board, action, &state.config, stage)?;
    // A retained registered child can lie outside a newly reduced placement
    // boundary. It has no scheduled analysis job there. Include missing or
    // insufficient coverage for those links in the root subset, otherwise a
    // retargeted root could remain unevaluable forever.
    let boundary_link = |e: &super::editor::Edge| {
        e.registered
            && !state.config.unregistered_only
            && !state.distances.contains_key(&Context {
                key: e.child,
                side: context.side.other(),
            })
    };
    for edge in node
        .edges
        .iter()
        .filter(|e| !e.registered || boundary_link(e))
    {
        if let Some(v) = editor.values.opt(edge.direct()) {
            if v.is_final() && v.bound() != Bound::Point {
                // A final-score bound cannot be replaced by a finite-horizon
                // point. Complete the subset at Full to refine it safely.
                target.depth = board.empty_count();
                target.selectivity = Selectivity::Full;
            } else if !v.is_final() {
                target.depth = target.depth.max(v.depth());
                if (v.selectivity() as u8) > (target.selectivity as u8) {
                    target.selectivity = v.selectivity();
                }
            }
        }
    }
    if let Some(f) = &node.move_search {
        if editor.values.get(f.value).is_final() {
            // Its non-best moves are final-score certificates too. Refining
            // the remaining subset must not replace them with horizon values.
            target.depth = target.depth.max(board.empty_count());
            target.selectivity = Selectivity::Full;
        } else {
            target.depth = target.depth.max(editor.values.get(f.value).depth());
            if (editor.values.get(f.value).selectivity() as u8) > (target.selectivity as u8) {
                target.selectivity = editor.values.get(f.value).selectivity();
            }
        }
    }
    let mut mask = 0;
    let mut squares = 0;
    let mut best_point = None;
    let mut max_upper = -65;
    let mut ready = true;
    let mut repair_registered = false;
    for (i, e) in node.edges.iter().enumerate() {
        if e.registered && !boundary_link(e) {
            continue;
        }
        let ev = editor.move_values_at(context.key, e);
        if e.registered {
            let sufficient = ev.selected.is_some_and(|v| {
                quality_ok(
                    v,
                    editor.identity,
                    target,
                    board.empty_count(),
                    state.config.allow_imported,
                ) && (v.bound() == Bound::Point
                    || (editor
                        .values
                        .opt(node.value)
                        .is_some_and(|v| v.bound() == Bound::Point)
                        && matches!(v.bound(), Bound::Upper)))
            });
            if reuse && sufficient {
                continue;
            }
            repair_registered = true;
        }
        if let Some(v) = ev.selected.filter(|v| {
            quality_ok(
                *v,
                editor.identity,
                target,
                board.empty_count(),
                state.config.allow_imported,
            )
        }) {
            match v.bound() {
                Bound::Point => {
                    best_point = Some(best_point.map_or(v.score(), |s: i16| s.max(v.score())))
                }
                Bound::Upper => max_upper = max_upper.max(v.score()),
                _ => ready = false,
            }
        } else {
            ready = false;
        }
        let fixed = editor
            .values
            .opt(e.manual())
            .is_some_and(|v| v.identity() == editor.identity)
            || editor.values.opt(e.direct()).is_some_and(|v| {
                v.bound() == Bound::Point
                    && quality_ok(
                        v,
                        editor.identity,
                        target,
                        board.empty_count(),
                        state.config.allow_imported,
                    )
                    && (reuse || v.is_final())
            });
        if !fixed {
            mask |= 1u64 << i;
            if let Action::Place(square) = e.action {
                squares |= 1u64 << square.index();
            }
        }
    }
    if reuse && !repair_registered && ready && best_point.is_some_and(|v| v >= max_upper) {
        return Ok(None);
    }
    // Covered upper bounds below every admissible new branch need no exact
    // scores. Keep them as upper bounds instead of searching rejected moves.
    if reuse
        && !repair_registered
        && ready
        && let Some(best) = editor
            .values
            .opt(node.value)
            .filter(|v| v.bound() == Bound::Point)
    {
        let tolerance = state
            .config
            .loss(context.side)
            .min(state.config.leaf_loss.unwrap_or(128));
        let cannot_expand = active_paths(editor, state, context)
            .iter()
            .all(|p| p.distance >= state.config.max_plies)
            && !board.must_pass();
        if (cannot_expand || max_upper < best.score() - i16::from(tolerance))
            && best_point.is_none_or(|v| v < best.score() - i16::from(tolerance))
        {
            return Ok(None);
        }
    }
    if mask == 0 {
        return Ok(None);
    }
    if let Some(f) = node
        .move_search
        .as_ref()
        .filter(|f| editor.values.get(f.value).is_final())
    {
        // A direct certified lower and this aggregate's upper are independent
        // certificates. One direct slot cannot store both. Keep such moves in
        // the new Full subset so retiring the old aggregate cannot lose their
        // upper coverage, even when those moves are already registered.
        for (i, edge) in node.edges.iter().enumerate() {
            if f.mask & (1u64 << i) != 0
                && editor
                    .values
                    .opt(edge.direct())
                    .is_some_and(|v| v.is_final() && v.bound() == Bound::Lower)
            {
                mask |= 1u64 << i;
                if let Action::Place(square) = edge.action {
                    squares |= 1u64 << square.index();
                }
            }
        }
    }
    let mut request = if target.depth >= board.empty_count() {
        SearchRequest::new(Goal::Endgame).selectivity(target.selectivity)
    } else {
        SearchRequest::new(Goal::Depth(target.depth)).selectivity(target.selectivity)
    }
    .root_moves(squares);
    if let Some(Action::Place(square)) = state
        .hints
        .get(&context)
        .or_else(|| node.move_search.as_ref().and_then(|f| f.pv.first()))
    {
        request = request.preferred_move(*square);
    }
    Ok(Some(Work {
        context,
        distance,
        stage,
        revision: node.revision,
        mask,
        burst: state.active_burst,
        request,
        requested: state.config.analysis.levels().map_or_else(
            || {
                let s = state.config.analysis.stages().unwrap()[stage];
                RequestedAnalysis::ParentDepth {
                    depth: s.depth,
                    selectivity: s.selectivity,
                }
            },
            |levels| RequestedAnalysis::ChildLevel(levels[stage]),
        ),
        planned: if target.depth >= board.empty_count() {
            SearchKind::Endgame {
                selectivity: target.selectivity,
            }
        } else {
            SearchKind::Midgame {
                depth: target.depth,
                selectivity: target.selectivity,
            }
        },
    }))
}

// バッチ内でも先行結果が他局面を更新し得る。編集世代が変わった仕事は採用せず再投入。
// 採用時は最良手にだけ点評価を付け、残りはMoveSearch経由の上限として扱う。
// 手の登録はここで行わず、expand側で損失・手数・予算を確認してから行う。
fn publish(
    editor: &mut BookEditor,
    state: &mut GenerationState,
    work: &Work,
    result: deft_reversi_engine::SearchResult,
) -> Result<bool> {
    let key = work.context.key;
    if editor.nodes[&key].revision != work.revision {
        state.enqueue(work.context, work.stage, work.distance);
        return Ok(false);
    }
    let best = result
        .best_move
        .ok_or_else(|| invalid("frontier search returned no move"))?;
    let score = result
        .score
        .ok_or_else(|| invalid("frontier search returned no score"))?;
    let kind = result
        .kind
        .ok_or_else(|| invalid("frontier search returned no quality"))?;
    let completed = SearchKind::from_result(kind);
    let claim = if result.is_subset_solved() {
        Score::Final(ScoreInterval::point(score as i16)?)
    } else {
        Score::Estimate {
            score: score as i16,
            bound: Bound::Point,
        }
    };
    let value = Evaluation::new(
        claim,
        EvaluationSource::Search(CompletedSearch {
            requested: work.requested,
            planned: work.planned,
            completed,
            root: [key.0, key.1],
            moves: result.root_moves,
            best: action_code(best),
            result_score: score as i16,
            run: editor.active_run,
            nodes: result.stats.nodes,
            elapsed_micros: result.elapsed.as_micros().min(u128::from(u64::MAX)) as u64,
        }),
        editor.identity,
    )?;
    value.validate_at(key.board())?;
    let index = editor.nodes[&key]
        .edges
        .iter()
        .position(|e| e.action == best)
        .ok_or_else(|| invalid("frontier move is illegal"))?;
    if work.mask & (1u64 << index) == 0 {
        return Err(invalid("frontier returned an excluded move"));
    }
    let frontier = MoveSearch::new(work.mask, best, value, result.pv, &mut editor.values)?;
    let old = editor.checkpoint(key);
    editor.bump()?;
    let n = editor.nodes.get_mut(&key).unwrap();
    n.preserve_search_final_scores(&mut editor.values);
    if n.edges[index]
        .direct()
        .is_none_or(|old| dominates(value, editor.values.get(old)))
    {
        n.edges[index].set_direct(Some(frontier.point));
    }
    n.move_search = Some(frontier);

    n.revision = editor.revision;
    editor.extensions.clear();
    for k in editor.commit_checkpoint(old)? {
        state.invalidate(k);
    }
    Ok(true)
}
fn terminal_moves(editor: &mut BookEditor, state: &mut GenerationState, key: Key) -> Result<()> {
    let board = key.board();
    let changes: Vec<_> = editor.nodes[&key]
        .edges
        .iter()
        .enumerate()
        .filter_map(|(i, edge)| {
            let child = board.play(edge.action).unwrap();
            if edge.registered || !child.is_terminal() {
                return None;
            }
            let rules = terminal_value(child);
            let value = rules.child_to_parent(matches!(edge.action, Action::Place(_)));
            edge.direct()
                .is_none_or(|old| {
                    editor.values.get(old) != value && dominates(value, editor.values.get(old))
                })
                .then_some((i, rules, value))
        })
        .collect();
    if changes.is_empty() {
        return Ok(());
    }
    let old = editor.checkpoint(key);
    editor.bump()?;
    for (i, _rules, value) in changes {
        editor.nodes.get_mut(&key).unwrap().edges[i].set_direct(Some(editor.values.intern(value)));
    }
    editor.nodes.get_mut(&key).unwrap().revision = editor.revision;
    for key in editor.commit_checkpoint(old)? {
        state.invalidate(key);
    }
    Ok(())
}

fn empty(config: GenerationConfig) -> GenerationState {
    use std::collections::{BTreeMap, BTreeSet};
    GenerationState {
        reachability: Default::default(),
        config,
        roots: Vec::new(),
        iteration: 0,
        expansions: BTreeSet::new(),
        expanded: BTreeMap::new(),
        distances: BTreeMap::new(),
        fresh: BTreeSet::new(),
        deepen: BTreeSet::new(),
        priority: None,
        urgent: Default::default(),
        hints: Default::default(),
        active_burst: 0,
    }
}
fn save(
    editor: &mut BookEditor,
    state: &mut GenerationState,
    report: &mut GenerationReport,
    callback: &mut impl FnMut(&BookEditor, &GenerationReport) -> Result<()>,
) -> Result<()> {
    editor.compact_evidence()?;
    let placeholder = empty(state.config.clone());
    editor.generation = Some(std::mem::replace(state, placeholder));
    let started = Instant::now();
    if let Some(id) = editor.active_run {
        let run = &mut editor.catalog.runs[id as usize];
        run.nodes = report.nodes;
        run.elapsed_micros = report.elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
    }
    let outcome = callback(editor, report);
    report.profile.checkpoint_wall += started.elapsed();
    *state = editor.generation.take().unwrap();
    outcome
}
fn completed(
    editor: &mut BookEditor,
    state: &mut GenerationState,
    c: Context,
    d: u8,
    s: usize,
    report: &mut GenerationReport,
) -> Result<()> {
    let next = s + 1;
    editor.nodes.get_mut(&c.key).unwrap().completed_stages = next;
    state.schedule(c, next, d);
    report.completed_stages += 1;
    Ok(())
}
fn expand_and_schedule(
    editor: &mut BookEditor,
    state: &mut GenerationState,
    (c, d): (Context, u8),
    engine: &Engine,
    limits: GenerationLimits,
    report: &mut GenerationReport,
    burst: u8,
) -> Result<()> {
    if state.config.unregistered_only {
        return Ok(());
    }
    let selected = editor.nodes[&c.key].move_search.as_ref().and_then(|f| {
        editor.nodes[&c.key]
            .edges
            .iter()
            .find(|e| e.action == f.best && !e.registered)
            .map(|_| (f.best, f.pv.get(1).copied()))
    });
    let check_started = Instant::now();
    let actual = active_distance(editor, state, c);
    report.profile.reachability_wall += check_started.elapsed();
    if let Some(actual) = actual {
        let timer = Instant::now();
        expand(editor, state, c, actual, limits, report)?;
        report.profile.expansion_wall += timer.elapsed();
    }
    if burst > 0
        && let Some((action, hint)) = selected
        && editor.nodes[&c.key]
            .edges
            .iter()
            .any(|e| e.action == action && e.registered)
    {
        let child = c.child(action);
        let stage = editor.nodes[&child.key].completed_stages;
        if !child.key.board().is_terminal() && stage < state.config.stage_count() {
            let distance = state.distances[&child];
            state.urgent.retain(|job| job.0 != child);
            state.urgent.push_front((child, distance, stage, burst - 1));
            if let Some(hint) = hint {
                let (_, transform) = Key::new(c.key.board().play(action).unwrap());
                let hint = transform.action(hint);
                if child.key.board().play(hint).is_ok() {
                    state.hints.insert(child, hint);
                }
            }
        }
    }
    if report.reason != GenerationStop::Exhausted {
        return Ok(());
    }
    let count = state.config.stage_count();
    let s = editor.nodes[&c.key].completed_stages;
    // After child values_for_parent, a formerly rejected remaining subset can become
    // relevant again. Reopen the last target rather than trusting old masks.
    if s >= count
        && !c.key.board().is_terminal()
        && prepare(engine, editor, state, c, d, count - 1, true)?.is_some()
    {
        editor.nodes.get_mut(&c.key).unwrap().completed_stages = count - 1;
        state.enqueue(c, count - 1, d);
    }
    Ok(())
}

pub(super) fn generate(
    editor: &mut BookEditor,
    engine: &mut Engine,
    limits: GenerationLimits,
    stop: StopHandle,
    callback: &mut impl FnMut(&BookEditor, &GenerationReport) -> Result<()>,
) -> Result<GenerationReport> {
    limits.validate()?;
    if editor
        .generation
        .as_ref()
        .is_some_and(|g| g.config.unregistered_only)
        && limits.time.is_none()
        && limits.nodes.is_none()
    {
        return Err(invalid(
            "frontier recalculation requires a time or node budget",
        ));
    }
    if analysis_identity(engine)? != editor.identity {
        return Err(invalid(
            "engine/model/search semantics do not match workspace; use retarget or CLI reanalyze",
        ));
    }
    // 実行中は仕事一覧を取り出して更新する。保存コールバックの直前と終了処理で
    // editorへ戻すので、保存側から未完の仕事が消えて見える状態を避ける。
    let mut state = editor
        .generation
        .take()
        .ok_or_else(|| invalid("configure generation first"))?;
    editor.catalog.engine(editor.identity);
    let run_id =
        u32::try_from(editor.catalog.runs.len()).map_err(|_| BookError::Limit("run count"))?;
    editor.catalog.runs.push(GenerationRun {
        engine_version: format!("deft_reversi_engine/{}", deft_reversi_engine::VERSION),
        search_revision: deft_reversi_engine::SEARCH_SEMANTICS_REVISION,
        build_label: editor
            .catalog
            .profiles
            .get(&editor.identity)
            .and_then(|p| p.build_label.clone()),
        identity: editor.identity,
        config: state.config.clone(),
        roots: state
            .roots
            .iter()
            .map(|c| GenerationRoot {
                board: c.key.board(),
                side: c.side,
            })
            .collect(),
        started_unix_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        threads: engine.options().threads as u16,
        main_tt_mib: engine.options().tt_mib as u64,
        limits,
        status: RunStatus::Running,
        nodes: 0,
        elapsed_micros: 0,
        interrupted: Vec::new(),
    });
    editor.active_run = Some(run_id);
    let started = Instant::now();
    let mut last_checkpoint = started;
    let mut report = GenerationReport {
        reason: GenerationStop::Exhausted,
        nodes: 0,
        new_positions: 0,
        completed_stages: 0,
        elapsed: Duration::ZERO,
        profile: GenerationProfile::default(),
    };
    let cache_start = {
        let c = state.reachability.borrow();
        (c.hits, c.misses)
    };
    let resources = engine.options();
    let mut pool = BatchPool::new(resources);
    // キューから取り出した後、完了または再投入するまでの仕事を追跡する。
    // エラーでループを抜けても、末尾で残りを戻して次回の再開に残す。
    let mut active: Vec<(Context, u8, usize)> = Vec::new();
    let outcome = (|| -> Result<()> {
        loop {
            if let Some(reason) = budget_reason(limits, &report, started, &stop) {
                report.reason = reason;
                break;
            }
            if editor
                .estimated_bytes()
                .saturating_add(state.estimated_bytes())
                .saturating_add(16384)
                > state.config.max_memory_bytes
            {
                editor.compact_evidence()?;
                if editor
                    .estimated_bytes()
                    .saturating_add(state.estimated_bytes())
                    .saturating_add(16384)
                    > state.config.max_memory_bytes
                {
                    report.reason = GenerationStop::MemoryLimit;
                    break;
                }
            }
            if let Some((d, c)) = state.expansions.pop_first() {
                if state.distances.get(&c) == Some(&d) {
                    expand_and_schedule(
                        editor,
                        &mut state,
                        (c, d),
                        engine,
                        limits,
                        &mut report,
                        0,
                    )?;
                    if report.reason != GenerationStop::Exhausted {
                        state.expansions.insert((d, c));
                        break;
                    }
                }
                continue;
            }
            let width = pool.width(
                state.config.max_parallel_jobs,
                limits.nodes.map(|n| n.saturating_sub(report.nodes)),
            );
            let mut jobs = Vec::new();
            let mut keys = std::collections::BTreeSet::new();
            let mut deferred = Vec::new();
            while jobs.len() < width {
                if budget_reason(limits, &report, started, &stop).is_some() {
                    break;
                }
                let timer = Instant::now();
                let next = state.pop(editor);
                report.profile.scheduling_wall += timer.elapsed();
                let Some((c, d, s)) = next else {
                    break;
                };
                if state.distances.get(&c) != Some(&d) {
                    continue;
                }
                let stage = editor.nodes[&c.key].completed_stages;
                if stage != s {
                    state.schedule(c, stage, d);
                    continue;
                }
                if !keys.insert(c.key) {
                    deferred.push((c, d, s));
                    continue;
                }
                let timer = Instant::now();
                let reachable = analysis_distance(editor, &state, c).is_some();
                report.profile.reachability_wall += timer.elapsed();
                if c.key.board().is_terminal() || !reachable {
                    continue;
                }
                state.iteration = state.iteration.wrapping_add(1);
                active.push((c, d, s));
                terminal_moves(editor, &mut state, c.key)?;
                let timer = Instant::now();
                let work = prepare(
                    engine,
                    editor,
                    &state,
                    c,
                    d,
                    s,
                    state.config.reuse_completed,
                )?;
                report.profile.planning_wall += timer.elapsed();
                if let Some(work) = work {
                    jobs.push(work);
                } else {
                    let burst = state.active_burst;
                    expand_and_schedule(
                        editor,
                        &mut state,
                        (c, d),
                        engine,
                        limits,
                        &mut report,
                        burst,
                    )?;
                    if report.reason == GenerationStop::Exhausted {
                        completed(editor, &mut state, c, d, s, &mut report)?;
                    } else {
                        state.enqueue(c, s, d);
                    }
                    active.retain(|job| job.0 != c);
                    if report.reason != GenerationStop::Exhausted {
                        break;
                    }
                }
            }
            for (c, d, s) in deferred {
                state.enqueue(c, s, d);
            }
            if report.reason != GenerationStop::Exhausted {
                break;
            }
            if jobs.is_empty() {
                if let Some(reason) = budget_reason(limits, &report, started, &stop) {
                    report.reason = reason;
                }
                if state.fresh.is_empty() && state.deepen.is_empty() {
                    break;
                }
                if report.reason != GenerationStop::Exhausted {
                    break;
                }
                continue;
            }
            if let Some(reason) = budget_reason(limits, &report, started, &stop) {
                report.reason = reason;
                break;
            }
            let timer = Instant::now();
            let results = pool.run(engine, &jobs, limits, &report, started, &stop)?;
            report.profile.engine_wall += timer.elapsed();
            report.profile.move_searches += jobs.len() as u64;
            report.profile.max_parallel_jobs = report.profile.max_parallel_jobs.max(jobs.len());
            for (work, result) in jobs.iter().zip(results) {
                let c = work.context;
                let d = work.distance;
                let s = work.stage;
                let result = result?;
                report.nodes = report.nodes.saturating_add(result.stats.nodes);
                // 途中反復のスコアは診断用に記録し、この段階の完了値としてpublishしない。
                if result.stop_reason != StopReason::Completed {
                    match result.stop_reason {
                        StopReason::Requested => report.reason = GenerationStop::Requested,
                        StopReason::TimeLimit => report.reason = GenerationStop::TimeLimit,
                        StopReason::NodeLimit => {}
                        _ => return Err(invalid("frontier search worker failed")),
                    }
                    let reason = match result.stop_reason {
                        StopReason::Requested => GenerationStop::Requested,
                        StopReason::TimeLimit => GenerationStop::TimeLimit,
                        _ => GenerationStop::NodeLimit,
                    };
                    let run = &mut editor.catalog.runs[run_id as usize];
                    run.interrupted.push(InterruptedAnalysis {
                        root: c.key.board(),
                        side: c.side,
                        stage: s as u8,
                        requested: work.requested,
                        planned: work.planned,
                        moves: result.root_moves,
                        completed: result.kind.map(SearchKind::from_result),
                        score: result.score.map(|v| v as i16),
                        reason,
                        nodes: result.stats.nodes,
                    });
                    // Only the final batch can be interrupted. Bound metadata
                    // by the total worker count rather than retaining a log.
                    if run.interrupted.len() > 256 {
                        run.interrupted.remove(0);
                    }
                    state.enqueue(c, s, d);
                    active.retain(|job| job.0 != c);
                    continue;
                }
                let timer = Instant::now();
                let published = publish(editor, &mut state, work, result)?;
                report.profile.publish_wall += timer.elapsed();
                if !published {
                    active.retain(|job| job.0 != c);
                    continue;
                }
                if budget_reason(limits, &report, started, &stop).is_none() {
                    expand_and_schedule(
                        editor,
                        &mut state,
                        (c, d),
                        engine,
                        limits,
                        &mut report,
                        work.burst,
                    )?;
                }
                if report.reason == GenerationStop::Exhausted {
                    let timer = Instant::now();
                    let done = state.config.unregistered_only
                        || prepare(engine, editor, &state, c, d, s, true)?.is_none();
                    report.profile.planning_wall += timer.elapsed();
                    if done {
                        completed(editor, &mut state, c, d, s, &mut report)?;
                    } else {
                        state.enqueue(c, s, d);
                    }
                } else {
                    state.enqueue(c, s, d);
                }
                active.retain(|job| job.0 != c);
            }
            if report.reason != GenerationStop::Exhausted {
                break;
            }
            if last_checkpoint.elapsed() >= state.config.checkpoint_interval {
                report.elapsed = started.elapsed();
                save(editor, &mut state, &mut report, callback)?;
                last_checkpoint = Instant::now();
            }
        }
        Ok(())
    })();
    for (c, d, s) in active {
        state.enqueue(c, s, d);
    }
    report.profile.resources_wall = pool.setup_wall;
    report.profile.engine_wall = report.profile.engine_wall.saturating_sub(pool.setup_wall);
    drop(pool);
    let restore = engine.set_resources(resources).map_err(BookError::from);
    report.elapsed = started.elapsed();
    {
        let cache = state.reachability.borrow();
        report.profile.reachability_hits = cache.hits - cache_start.0;
        report.profile.reachability_misses = cache.misses - cache_start.1;
    }
    editor.generation = Some(state);
    let run = &mut editor.catalog.runs[run_id as usize];
    run.status = if outcome.is_ok() && restore.is_ok() {
        RunStatus::Stopped(report.reason)
    } else {
        RunStatus::Failed
    };
    run.nodes = report.nodes;
    run.elapsed_micros = report.elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
    editor.active_run = None;
    editor.compact_evidence()?;
    outcome?;
    restore?;
    let timer = Instant::now();
    callback(editor, &report)?;
    report.profile.checkpoint_wall += timer.elapsed();
    Ok(report)
}

// 独立局面を同時探索するEngine群。totalは全体のスレッド数とTT（探索の置換表）の予算。
// 各Engineへ予算を分配し、generate終了時には元のEngineの資源設定を復元する。
struct BatchPool {
    total: EngineOptions,
    others: Vec<Engine>,
    setup_wall: Duration,
}
impl BatchPool {
    fn new(total: EngineOptions) -> Self {
        Self {
            total,
            others: Vec::new(),
            setup_wall: Duration::ZERO,
        }
    }
    fn width(&self, configured: u16, nodes: Option<u64>) -> usize {
        let cap = if configured == 0 {
            self.total.threads
        } else {
            usize::from(configured).min(self.total.threads)
        };
        cap.min(self.total.tt_mib)
            .min(nodes.unwrap_or(u64::MAX).min(usize::MAX as u64) as usize)
            .max(1)
    }
    fn run(
        &mut self,
        main: &mut Engine,
        jobs: &[Work],
        limits: GenerationLimits,
        report: &GenerationReport,
        start: Instant,
        stop: &StopHandle,
    ) -> Result<Vec<std::result::Result<SearchResult, deft_reversi_engine::EngineError>>> {
        let setup = Instant::now();
        let count = jobs.len();
        self.others.truncate(count.saturating_sub(1));
        let options = |i: usize| {
            EngineOptions::default()
                .threads(self.total.threads / count + usize::from(i < self.total.threads % count))
                .tt_mib(self.total.tt_mib / count + usize::from(i < self.total.tt_mib % count))
        };
        // Shrink every existing allocation before enlarging any. Stable batch
        // widths keep their tables and worker pools, avoiding repeated setup.
        for (i, engine) in std::iter::once(&mut *main)
            .chain(self.others.iter_mut())
            .enumerate()
        {
            let old = engine.options();
            let target = options(i);
            engine.set_resources(
                EngineOptions::default()
                    .threads(old.threads.min(target.threads))
                    .tt_mib(old.tt_mib.min(target.tt_mib)),
            )?;
        }
        main.set_resources(options(0))?;
        for i in 1..count {
            if i - 1 == self.others.len() {
                self.others.push(main.fork(options(i))?);
            } else {
                self.others[i - 1].set_resources(options(i))?;
            }
        }
        let left = limits.nodes.map(|n| n.saturating_sub(report.nodes));
        let requests: Vec<_> = jobs
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let mut r = w.request.clone().with_stop(stop.clone());
                if let Some(left) = left {
                    r = r.node_limit(
                        left / count as u64 + u64::from((i as u64) < left % count as u64),
                    );
                }
                if let Some(t) = limits.time {
                    r = r.time_limit(t.saturating_sub(start.elapsed()));
                }
                r
            })
            .collect();
        self.setup_wall += setup.elapsed();
        std::thread::scope(|scope| {
            let handles: Vec<_> = self
                .others
                .iter_mut()
                .zip(jobs.iter().skip(1))
                .zip(requests.iter().skip(1))
                .map(|((engine, work), request)| {
                    scope.spawn(move || engine.search(work.context.key.board(), request))
                })
                .collect();
            let mut results = vec![main.search(jobs[0].context.key.board(), &requests[0])];
            let mut failed = false;
            for handle in handles {
                match handle.join() {
                    Ok(r) => results.push(r),
                    Err(_) => {
                        stop.stop();
                        failed = true;
                    }
                }
            }
            if failed {
                Err(invalid("book batch worker panicked"))
            } else {
                Ok(results)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Model, PatternModel};
    fn engine() -> Engine {
        Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().threads(7).tt_mib(7),
        )
        .unwrap()
    }
    #[test]
    fn changing_batch_width_partitions_both_resources_without_oversubscription() {
        let mut main = engine();
        let mut pool = BatchPool::new(main.options());
        let c = Context {
            key: Key::new(Board::new()).0,
            side: BookSide::Black,
        };
        for count in [3, 1, 4, 4, 2] {
            let jobs: Vec<_> = (0..count)
                .map(|_| Work {
                    context: c,
                    distance: 0,
                    stage: 0,
                    revision: 0,
                    mask: 15,
                    burst: 0,
                    request: SearchRequest::new(Goal::Depth(2)),
                    requested: RequestedAnalysis::External,
                    planned: SearchKind::Midgame {
                        depth: 2,
                        selectivity: Selectivity::Full,
                    },
                })
                .collect();
            let report = GenerationReport {
                reason: GenerationStop::Exhausted,
                nodes: 0,
                new_positions: 0,
                completed_stages: 0,
                elapsed: Duration::ZERO,
                profile: GenerationProfile::default(),
            };
            let results = pool
                .run(
                    &mut main,
                    &jobs,
                    GenerationLimits::default(),
                    &report,
                    Instant::now(),
                    &StopHandle::new(),
                )
                .unwrap();
            assert_eq!(results.len(), count);
            assert!(results.into_iter().all(|r| r.unwrap().score == Some(0)));
            let budgets: Vec<_> = std::iter::once(main.options())
                .chain(pool.others.iter().map(Engine::options))
                .collect();
            assert_eq!(budgets.len(), count);
            assert_eq!(budgets.iter().map(|o| o.threads).sum::<usize>(), 7);
            assert_eq!(budgets.iter().map(|o| o.tt_mib).sum::<usize>(), 7);
            assert!(budgets.iter().all(|o| o.threads >= 1 && o.tt_mib >= 1));
        }
    }
    #[test]
    fn stale_batch_result_is_requeued_without_publishing_or_advancing_stage() {
        let mut eng = engine();
        let board = Board::new();
        let mut ed = BookEditor::for_engine(&eng).unwrap();
        ed.configure_generation(
            &[board],
            GenerationConfig {
                analysis: AnalysisSchedule::Stages(vec![Stage {
                    depth: 2,
                    selectivity: Selectivity::Full,
                }]),
                ..Default::default()
            },
        )
        .unwrap();
        let mut state = ed.generation.take().unwrap();
        let c = state.roots[0];
        let work = prepare(&eng, &ed, &state, c, 0, 0, true).unwrap().unwrap();
        let result = eng.search(c.key.board(), &work.request).unwrap();
        ed.register(c.key, ed.nodes[&c.key].edges[0].action)
            .unwrap();
        assert!(!publish(&mut ed, &mut state, &work, result).unwrap());
        assert!(ed.nodes[&c.key].move_search.is_none());
        assert_eq!(ed.nodes[&c.key].completed_stages, 0);
        assert!(state.fresh.contains(&(0, c)));
    }
}
