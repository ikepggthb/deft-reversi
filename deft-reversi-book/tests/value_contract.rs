//! Acceptance scenarios for the value model, independent of generator heuristics.
use deft_reversi_book::legacy::{self, AnalysisQuality, ImportedValue, ValueOrigin};
use deft_reversi_book::*;
use deft_reversi_engine::*;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "deft-value-contract-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self, s: &str) -> PathBuf {
        self.0.join(s)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn engine() -> Engine {
    Engine::new(
        Model::from_pattern(PatternModel::zero())
            .with_calibration(legacy_main_rules(), ProbCutSigmas::NORMAL)
            .unwrap(),
        EngineOptions::default().tt_mib(1),
    )
    .unwrap()
}
fn actions(b: Board) -> Vec<Action> {
    let mut a: Vec<_> = b.legal_moves().map(Action::Place).collect();
    if b.must_pass() {
        a.push(Action::Pass);
    }
    a
}
fn position(mut seed: u64, empty: u8) -> Board {
    let mut b = Board::new();
    while b.empty_count() > empty && !b.is_terminal() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a = actions(b);
        b = b.play(a[seed as usize % a.len()]).unwrap();
    }
    b.canonical()
}
fn brute(b: Board) -> i16 {
    if b.is_terminal() {
        b.final_score() as i16
    } else {
        actions(b)
            .into_iter()
            .map(|a| -brute(b.play(a).unwrap()))
            .max()
            .unwrap()
    }
}
fn config(depth: u8, sel: Selectivity, plies: u8) -> GenerationConfig {
    GenerationConfig {
        analysis: AnalysisSchedule::Stages(vec![Stage {
            depth,
            selectivity: sel,
        }]),
        max_plies: plies,
        expansion_loss: 4,
        ..Default::default()
    }
}
fn budget() -> GenerationLimits {
    GenerationLimits {
        nodes: Some(2_000_000),
        ..Default::default()
    }
}
fn legacy(id: AnalysisIdentity, b: Board, score: i16, bound: legacy::Bound) -> ImportedValue {
    ImportedValue {
        identity: id,
        score,
        bound,
        origin: ValueOrigin::Imported,
        quality: AnalysisQuality {
            depth: b.empty_count(),
            selectivity: Selectivity::Full,
            solved: bound == legacy::Bound::Exact,
        },
    }
}
fn search(eng: &mut Engine, b: Board, kind: SearchKind, mask: u64) -> Evaluation {
    let request = match kind {
        SearchKind::Midgame { depth, selectivity } => {
            SearchRequest::new(Goal::Depth(depth)).selectivity(selectivity)
        }
        SearchKind::Endgame { selectivity } => {
            SearchRequest::new(Goal::Endgame).selectivity(selectivity)
        }
    }
    .root_moves(mask);
    let result = eng.search(b, &request).unwrap();
    assert_eq!(result.stop_reason, StopReason::Completed);
    Evaluation::from_search_result(
        b,
        RequestedAnalysis::External,
        kind,
        &result,
        analysis_identity(eng).unwrap(),
    )
    .unwrap()
}
fn mask(a: Action) -> u64 {
    if let Action::Place(s) = a {
        1u64 << s.index()
    } else {
        0
    }
}
#[test]
fn a1_requested_level_plan_completed_kind_and_profiles_survive_both_profiles() {
    let mut eng = engine();
    let dir = Temp::new();
    for (i, b) in [position(73, 56), position(73, 15), position(73, 6)]
        .into_iter()
        .enumerate()
    {
        let mut ed = BookEditor::for_engine(&eng).unwrap();
        let mut cfg = config(2, Selectivity::Full, 0);
        cfg.analysis = AnalysisSchedule::Levels(vec![19]);
        ed.configure_generation(&[b], cfg).unwrap();
        assert_eq!(
            ed.generate(&mut eng, budget(), StopHandle::new())
                .unwrap()
                .reason,
            GenerationStop::Exhausted
        );
        let direct = actions(b)
            .into_iter()
            .find_map(|a| ed.move_values(b, a).unwrap().direct)
            .unwrap();
        let EvaluationSource::SearchMove(trace) = direct.source() else {
            panic!("lost completed search")
        };
        assert_eq!(trace.requested, RequestedAnalysis::ChildLevel(19));
        assert_eq!(trace.planned, trace.completed);
        assert_eq!(trace.root, [b.player_bits(), b.opponent_bits()]);
        assert_eq!(trace.run, Some(0));
        ed.save(dir.path(&format!("{i}.work"))).unwrap();
        let loaded =
            BookEditor::load(dir.path(&format!("{i}.work")), ReadLimits::default()).unwrap();
        assert!(
            actions(b)
                .into_iter()
                .any(|a| loaded.move_values(b, a).unwrap().direct == Some(direct))
        );
        let run = &loaded.generation_runs()[0];
        assert_eq!(run.config.analysis.levels(), Some(&vec![19]));
        assert_eq!(run.status, RunStatus::Stopped(GenerationStop::Exhausted));
        assert!(
            loaded
                .analysis_profiles()
                .any(|p| p.search_revision == Some(SEARCH_SEMANTICS_REVISION))
        );
        let book = loaded.export().unwrap();
        book.save(dir.path(&format!("{i}.book"))).unwrap();
        let book = Book::load(dir.path(&format!("{i}.book")), ReadLimits::default()).unwrap();
        assert!(
            book.probe(b)
                .unwrap()
                .candidates()
                .any(|c| c.value == Some(direct))
        );
        assert_eq!(book.generation_runs().len(), 1);
    }
}
#[test]
fn a2_midgame_endgame_and_legacy_have_distinct_use_requirements() {
    let mut eng = engine();
    let b = position(123, 8);
    let all = b.legal_moves().fold(0, |m, s| m | (1u64 << s.index()));
    let mid = search(
        &mut eng,
        b,
        SearchKind::Midgame {
            depth: 4,
            selectivity: Selectivity::Full,
        },
        all,
    );
    let end = search(
        &mut eng,
        b,
        SearchKind::Endgame {
            selectivity: Selectivity::Percent85,
        },
        all,
    );
    let full = search(
        &mut eng,
        b,
        SearchKind::Endgame {
            selectivity: Selectivity::Full,
        },
        all,
    );
    let policy = UsePolicy {
        min_depth: 16,
        min_selectivity: Selectivity::Full,
        min_endgame_selectivity: Selectivity::Percent85,
        ..Default::default()
    };
    let id = analysis_identity(&eng).unwrap();
    assert_eq!(policy.check(mid, id), Err(BookMiss::MidgameDepth));
    assert_eq!(policy.check(end, id), Ok(()));
    assert_eq!(policy.check(full, [99; 32]), Ok(()));
    assert!(!end.is_solved());
    assert!(full.is_solved());
    let mut staged = BookEditor::for_engine(&eng).unwrap();
    staged
        .configure_generation(&[b], config(8, Selectivity::Percent85, 0))
        .unwrap();
    staged
        .generate(&mut eng, budget(), StopHandle::new())
        .unwrap();
    let direct = actions(b)
        .iter()
        .find_map(|&a| {
            staged.move_values(b, a).unwrap().direct.filter(|v| {
                matches!(
                    v.source(),
                    EvaluationSource::Search(_) | EvaluationSource::SearchMove(_)
                )
            })
        })
        .unwrap();
    let (EvaluationSource::Search(trace) | EvaluationSource::SearchMove(trace)) = direct.source()
    else {
        unreachable!()
    };
    assert_eq!(
        trace.completed,
        SearchKind::Endgame {
            selectivity: Selectivity::Percent85
        }
    );
    assert!(!direct.is_solved());
    let mut old = ImportedValue::from(mid);
    old.quality.depth = 16;
    let old = Evaluation::import(old).unwrap();
    assert!(matches!(old.source(), EvaluationSource::Imported { .. }));
    assert_eq!(policy.check(old, id), Err(BookMiss::LegacyUnknown));
    assert_eq!(
        UsePolicy {
            allow_imported: true,
            ..policy
        }
        .check(old, id),
        Ok(())
    );
}
#[test]
fn a3_mixed_derivation_keeps_leaf_modes_and_round_trips_with_play_decision() {
    let mut eng = engine();
    let b = position(123, 9);
    let aa = actions(b);
    assert!(aa.len() > 1);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    for (i, &a) in aa.iter().enumerate() {
        let kind = if i == 0 {
            SearchKind::Endgame {
                selectivity: Selectivity::Percent85,
            }
        } else {
            SearchKind::Midgame {
                depth: 2,
                selectivity: Selectivity::Full,
            }
        };
        let v = search(&mut eng, b, kind, mask(a))
            .best_move_value()
            .unwrap();
        ed.record_candidate(b, a, v).unwrap();
        ed.register_move(b, a).unwrap();
    }
    let book = ed.export().unwrap();
    let value = book.probe(b).unwrap().value().unwrap();
    assert!(value.is_calculated());
    let summary = value.conditions();
    assert!(summary.has_midgame() && summary.has_endgame());
    assert_eq!(summary.midgame_depth(), Some(2));
    assert_eq!(summary.endgame_selectivity(), Some(Selectivity::Percent85));
    let policy = UsePolicy {
        min_depth: 2,
        min_selectivity: Selectivity::Full,
        moves: MovePolicy::RandomWithinLoss { max_loss: 128 },
        ..Default::default()
    };
    assert_eq!(policy.check(value, ed.identity()), Ok(()));
    assert_eq!(
        UsePolicy {
            min_depth: 3,
            ..policy
        }
        .check(value, ed.identity()),
        Err(BookMiss::MidgameDepth)
    );
    assert_eq!(
        UsePolicy {
            allow_calculated: false,
            ..policy
        }
        .check(value, ed.identity()),
        Err(BookMiss::DerivedRejected)
    );
    let dir = Temp::new();
    book.save(dir.path("play")).unwrap();
    let loaded = Book::load(dir.path("play"), ReadLimits::default()).unwrap();
    let mut a = BookPlayer::new(&eng, Some(Arc::new(book)), policy, 42).unwrap();
    let mut c = BookPlayer::new(&eng, Some(Arc::new(loaded)), policy, 42).unwrap();
    assert_eq!(a.choose(b).unwrap().action, c.choose(b).unwrap().action);
}
#[test]
fn a4_subset_search_cannot_be_recorded_as_a_full_position_or_excluded_move() {
    let mut eng = engine();
    let b = position(123, 6);
    let aa = actions(b);
    assert!(aa.len() > 1);
    let a = aa[0];
    let raw = search(
        &mut eng,
        b,
        SearchKind::Endgame {
            selectivity: Selectivity::Full,
        },
        mask(a),
    );
    let exact = -brute(b.play(a).unwrap());
    assert_eq!(
        raw.result(),
        Score::Final(ScoreInterval::point(exact).unwrap())
    );
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    assert!(ed.record_candidate(b, a, raw).is_err());
    assert!(
        ed.record_candidate(b, aa[1], raw.best_move_value().unwrap())
            .is_err()
    );
    ed.record_candidate(b, a, raw.best_move_value().unwrap())
        .unwrap();
    assert!(ed.export().unwrap().probe(b).unwrap().value().is_none());
}
#[test]
fn a5_a10_generated_edit_sequences_match_an_independent_interval_reference() {
    let mut eng = engine();
    let b = position(123, 5);
    let aa = actions(b);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.configure_generation(&[b], config(2, Selectivity::Full, 1))
        .unwrap();
    let mut expected = vec![None; aa.len()];
    let dir = Temp::new();
    let mut random = 917u64;
    for step in 0..96 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let index = random as usize % aa.len();
        let a = aa[index];
        let oracle = -brute(b.play(a).unwrap());
        match (random >> 32) % 6 {
            0 | 1 => {
                let lower = (random >> 32).is_multiple_of(6);
                let score = if lower {
                    (oracle - 2).max(-64)
                } else {
                    (oracle + 2).min(64)
                };
                let interval =
                    ScoreInterval::new(lower.then_some(score), (!lower).then_some(score)).unwrap();
                let old = expected[index];
                expected[index] = Some(old.map_or(interval, |old: ScoreInterval| {
                    old.intersect(interval).unwrap()
                }));
                ed.record_imported(
                    b,
                    a,
                    legacy(
                        ed.identity(),
                        b,
                        score,
                        if lower {
                            legacy::Bound::ProvenLower
                        } else {
                            legacy::Bound::ProvenUpper
                        },
                    ),
                )
                .unwrap();
            }
            2 => {
                ed.register_move(b, a).unwrap();
            }
            3 => {
                ed.unregister_move(b, a).unwrap();
                assert!(ed.generation_config().is_some());
            }
            4 => {
                let mut v = legacy(ed.identity(), b, 0, legacy::Bound::Upper);
                v.bound = legacy::Bound::Exact;
                v.quality.solved = false;
                v.origin = ValueOrigin::Manual;
                ed.record_imported(b, a, v).unwrap();
            }
            _ => {
                ed.clear_manual(b, a).unwrap();
            }
        }
        for (&a, &expected) in aa.iter().zip(&expected) {
            let ev = ed.move_values(b, a).unwrap();
            assert_eq!(
                ev.final_score.and_then(|v| v.result().final_bounds()),
                expected,
                "operation {step}"
            );
            if let Some(v) = expected {
                assert!(v.lower().is_none_or(|l| l <= -brute(b.play(a).unwrap())));
                assert!(v.upper().is_none_or(|u| u >= -brute(b.play(a).unwrap())));
            }
        }
        if step % 8 == 7 {
            ed.compact_evidence().unwrap();
            ed.save(dir.path("work")).unwrap();
            ed = BookEditor::load(dir.path("work"), ReadLimits::default()).unwrap();
            ed.export().unwrap().save(dir.path("play")).unwrap();
            Book::load(dir.path("play"), ReadLimits::default()).unwrap();
        }
    }
    // Equal bounds become a point; weaker certificates cannot widen it.
    for &a in &aa {
        let exact = -brute(b.play(a).unwrap());
        ed.clear_manual(b, a).unwrap();
        ed.record_imported(b, a, legacy(ed.identity(), b, exact, legacy::Bound::Exact))
            .unwrap();
        assert_eq!(
            ed.move_values(b, a).unwrap().final_score.unwrap().result(),
            Score::Final(ScoreInterval::point(exact).unwrap())
        );
    }
    assert_eq!(
        ed.export()
            .unwrap()
            .probe(b)
            .unwrap()
            .value()
            .unwrap()
            .score(),
        brute(b)
    );
    eng.set_resources(EngineOptions::default().threads(2).tt_mib(1))
        .unwrap();
    ed.retarget(&eng).unwrap();
}
#[test]
fn a5_conflicting_bounds_are_rejected_without_changing_the_candidate_or_revision() {
    let mut ed = BookEditor::new([7; 32]);
    let b = position(123, 5);
    let a = actions(b)[0];
    ed.record_imported(b, a, legacy([7; 32], b, 8, legacy::Bound::ProvenLower))
        .unwrap();
    let before = ed.move_values(b, a);
    let revision = ed.revision();
    assert!(
        ed.record_imported(b, a, legacy([7; 32], b, 6, legacy::Bound::ProvenUpper))
            .is_err()
    );
    assert_eq!(ed.revision(), revision);
    assert_eq!(ed.move_values(b, a), before);
}
#[test]
fn a8_interruption_keeps_the_plan_and_last_iteration_separate_from_published_evidence() {
    let mut eng = engine();
    let b = position(73, 40);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.configure_generation(&[b], config(16, Selectivity::Percent95, 0))
        .unwrap();
    let result = ed
        .generate(
            &mut eng,
            GenerationLimits {
                nodes: Some(700),
                ..Default::default()
            },
            StopHandle::new(),
        )
        .unwrap();
    assert_eq!(result.reason, GenerationStop::NodeLimit);
    let run = &ed.generation_runs()[0];
    assert!(!run.interrupted.is_empty());
    assert!(
        actions(b)
            .iter()
            .all(|&a| ed.move_values(b, a).unwrap().direct.is_none())
    );
    let job = &run.interrupted[0];
    assert_eq!(
        job.planned,
        SearchKind::Midgame {
            depth: 16,
            selectivity: Selectivity::Percent95
        }
    );
    assert!(
        job.completed
            .is_none_or(|k| k.horizon(b.empty_count()) < 16)
    );
    let dir = Temp::new();
    ed.save(dir.path("work")).unwrap();
    let mut ed = BookEditor::load(dir.path("work"), ReadLimits::default()).unwrap();
    assert_eq!(
        ed.generation_runs()[0].interrupted[0].completed,
        job.completed
    );
    ed.configure_generation(&[b], config(2, Selectivity::Full, 0))
        .unwrap();
    assert_eq!(
        ed.generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .reason,
        GenerationStop::Exhausted
    );
    assert_eq!(ed.generation_runs().len(), 2);
}
#[test]
fn a8_external_stopped_search_records_only_its_completed_iteration() {
    let mut eng = engine();
    let b = position(73, 40);
    let planned = SearchKind::Midgame {
        depth: 16,
        selectivity: Selectivity::Percent95,
    };
    let request = SearchRequest::new(Goal::Depth(16)).selectivity(Selectivity::Percent95);
    let stop = request.stop_handle();
    let result = eng.search_with(b, &request, |_| stop.stop()).unwrap();
    assert_eq!(result.stop_reason, StopReason::Requested);
    let value = Evaluation::from_search_result(
        b,
        RequestedAnalysis::ParentDepth {
            depth: 16,
            selectivity: Selectivity::Percent95,
        },
        planned,
        &result,
        analysis_identity(&eng).unwrap(),
    )
    .unwrap();
    let (EvaluationSource::Search(trace) | EvaluationSource::SearchMove(trace)) = value.source()
    else {
        panic!()
    };
    assert_eq!(
        trace.completed,
        SearchKind::from_result(result.kind.unwrap())
    );
    assert!(trace.completed.horizon(40) < 16);
    assert!(!value.is_solved());
}
#[test]
fn a9_legacy_fixtures_preserve_unknown_modes_without_guessing_from_depth() {
    let mut rules = BookEditor::new([7; 32]);
    let board = Board::from_bits(u64::MAX, 0).unwrap();
    rules.insert_position(board).unwrap();
    let value = rules
        .export()
        .unwrap()
        .probe(board)
        .unwrap()
        .value()
        .unwrap();
    assert!(matches!(value.source(), EvaluationSource::Terminal { .. }));
    assert_eq!(value.score(), board.final_score() as i16);

    let dir = Temp::new();
    std::fs::write(
        dir.path("old.work"),
        include_bytes!("fixtures/empty-v6.work"),
    )
    .unwrap();
    std::fs::write(
        dir.path("old.book"),
        include_bytes!("fixtures/empty-v2.book"),
    )
    .unwrap();
    let mut ed = BookEditor::load(dir.path("old.work"), ReadLimits::default()).unwrap();
    let b = Board::new();
    let a = actions(b)[0];
    let mut value = legacy(ed.identity(), b, 0, legacy::Bound::Upper);
    value.quality.solved = false;
    value.bound = legacy::Bound::Exact;
    ed.record_imported(b, a, value).unwrap();
    ed.save(dir.path("new.work")).unwrap();
    let ed = BookEditor::load(dir.path("new.work"), ReadLimits::default()).unwrap();
    assert!(matches!(
        ed.move_values(b, a).unwrap().direct.unwrap().source(),
        EvaluationSource::Imported { depth: 60, .. }
    ));
    assert!(ed.analysis_profiles().next().is_none());
    assert_eq!(
        Book::load(dir.path("old.book"), ReadLimits::default())
            .unwrap()
            .stats()
            .positions,
        1
    );
}
#[test]
fn a2_decisions_distinguish_no_book_unknown_mode_and_unregistered_move() {
    let eng = engine();
    let b = position(123, 8);
    let a = actions(b)[0];
    let mut player = BookPlayer::new(&eng, None, UsePolicy::default(), 1).unwrap();
    assert!(matches!(
        player.decide(b),
        BookDecision::Fallback(BookMiss::NoBook)
    ));
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let mut v = legacy(ed.identity(), b, 0, legacy::Bound::Upper);
    v.bound = legacy::Bound::Exact;
    v.quality.solved = false;
    ed.record_imported(b, a, v).unwrap();
    player.set_book(Some(Arc::new(ed.export().unwrap())));
    assert!(matches!(
        player.decide(b),
        BookDecision::Fallback(BookMiss::NoRegisteredMove)
    ));
    ed.register_move(b, a).unwrap();
    player.set_book(Some(Arc::new(ed.export().unwrap())));
    assert!(matches!(
        player.decide(b),
        BookDecision::Fallback(BookMiss::LegacyUnknown)
    ));
}
#[test]
fn interval_negation_and_pass_reach_preserve_actual_search_kind() {
    let mut eng = engine();
    let b = position(123, 7);
    let a = actions(b)[0];
    let child = b.play(a).unwrap().canonical();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.register_move(b, a).unwrap();
    for a in actions(child) {
        let score = -brute(child.play(a).unwrap());
        ed.record_imported(
            child,
            a,
            legacy(
                ed.identity(),
                child,
                (score - 2).max(-64),
                legacy::Bound::ProvenLower,
            ),
        )
        .unwrap();
        ed.record_imported(
            child,
            a,
            legacy(
                ed.identity(),
                child,
                (score + 2).min(64),
                legacy::Bound::ProvenUpper,
            ),
        )
        .unwrap();
    }
    let value = ed.export().unwrap().probe(child).unwrap().value().unwrap();
    let expected = value.result().final_bounds().unwrap();
    let backup = ed
        .move_values(b, a)
        .unwrap()
        .child
        .unwrap()
        .result()
        .final_bounds()
        .unwrap();
    assert_eq!(backup.lower(), expected.upper().map(|v| -v));
    assert_eq!(backup.upper(), expected.lower().map(|v| -v));
    // New typed child search is preserved at its raw completed depth.
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.register_move(b, a).unwrap();
    ed.configure_generation(&[child], config(2, Selectivity::Full, 0))
        .unwrap();
    ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let raw = actions(child)
        .iter()
        .find_map(|&a| ed.move_values(child, a).unwrap().direct)
        .unwrap();
    let (EvaluationSource::Search(trace) | EvaluationSource::SearchMove(trace)) = raw.source()
    else {
        panic!()
    };
    assert_eq!(
        trace.completed,
        SearchKind::Midgame {
            depth: 2,
            selectivity: Selectivity::Full
        }
    );
    let backup = ed.move_values(b, a).unwrap().child.unwrap();
    assert_eq!(backup.conditions().midgame_depth(), Some(3));
    assert!(
        actions(child)
            .into_iter()
            .any(|a| ed.move_values(child, a).unwrap().direct == Some(raw))
    );
    eng.set_resources(EngineOptions::default().tt_mib(1))
        .unwrap();
}
#[test]
fn conflicts_during_ancestor_propagation_and_registration_roll_back_the_edit() {
    let eng = engine();
    let b = position(123, 5);
    let a = actions(b)[0];
    let child = b.play(a).unwrap().canonical();
    let expected = -brute(child);
    let wrong = if expected < 64 {
        expected + 2
    } else {
        expected - 2
    };
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.register_move(b, a).unwrap();
    ed.record_imported(b, a, legacy(ed.identity(), b, wrong, legacy::Bound::Exact))
        .unwrap();
    let aa = actions(child);
    for &x in &aa[..aa.len() - 1] {
        ed.record_imported(
            child,
            x,
            legacy(
                ed.identity(),
                child,
                -brute(child.play(x).unwrap()),
                legacy::Bound::Exact,
            ),
        )
        .unwrap();
    }
    let last = *aa.last().unwrap();
    let revision = ed.revision();
    let before = ed.move_values(child, last);
    assert!(
        ed.record_imported(
            child,
            last,
            legacy(
                ed.identity(),
                child,
                -brute(child.play(last).unwrap()),
                legacy::Bound::Exact
            )
        )
        .is_err()
    );
    assert_eq!(ed.revision(), revision);
    assert_eq!(ed.move_values(child, last), before);
    assert_eq!(
        ed.move_values(b, a).unwrap().final_score.unwrap().score(),
        wrong
    );
    let dir = Temp::new();
    ed.save(dir.path("rolled-back.work")).unwrap();
    BookEditor::load(dir.path("rolled-back.work"), ReadLimits::default()).unwrap();
    // The same conflict is caught when a completed retained child is linked.
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.record_imported(b, a, legacy(ed.identity(), b, wrong, legacy::Bound::Exact))
        .unwrap();
    for x in actions(child) {
        ed.record_imported(
            child,
            x,
            legacy(
                ed.identity(),
                child,
                -brute(child.play(x).unwrap()),
                legacy::Bound::Exact,
            ),
        )
        .unwrap();
    }
    let before = ed.move_values(b, a);
    let revision = ed.revision();
    assert!(ed.register_move(b, a).is_err());
    assert_eq!(ed.revision(), revision);
    assert_eq!(ed.move_values(b, a), before);
    ed.validate().unwrap();
}
#[test]
fn a7_full_depth_midgame_is_not_reused_as_a_completed_endgame_certificate() {
    let mut eng = engine();
    let b = position(123, 6);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    for a in actions(b) {
        let value = search(
            &mut eng,
            b,
            SearchKind::Midgame {
                depth: 6,
                selectivity: Selectivity::Full,
            },
            mask(a),
        )
        .best_move_value()
        .unwrap();
        assert!(!value.is_solved());
        ed.record_candidate(b, a, value).unwrap();
    }
    assert!(
        !ImportedValue::from(ed.export().unwrap().probe(b).unwrap().value().unwrap())
            .quality
            .solved
    );
    ed.configure_generation(&[b], config(6, Selectivity::Full, 0))
        .unwrap();
    let report = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(report.profile.move_searches, 1);
    let value = ed.export().unwrap().probe(b).unwrap().value().unwrap();
    assert!(value.is_solved());
    assert_eq!(value.score(), brute(b));
}
#[test]
fn a7_modes_are_incomparable_and_unknown_estimates_require_explicit_generation_reuse() {
    let mut eng = engine();
    let b = position(123, 8);
    let a = actions(b)[0];
    let end = search(
        &mut eng,
        b,
        SearchKind::Endgame {
            selectivity: Selectivity::Percent85,
        },
        mask(a),
    )
    .best_move_value()
    .unwrap();
    let mid = search(
        &mut eng,
        b,
        SearchKind::Midgame {
            depth: 2,
            selectivity: Selectivity::Full,
        },
        mask(a),
    )
    .best_move_value()
    .unwrap();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.record_candidate(b, a, end).unwrap();
    assert!(ed.record_candidate(b, a, mid).is_err());
    assert_eq!(ed.move_values(b, a).unwrap().direct, Some(end));
    for allow_imported in [false, true] {
        let b = position(73, 20);
        let mut ed = BookEditor::for_engine(&eng).unwrap();
        for a in actions(b) {
            let mut v = legacy(ed.identity(), b, 0, legacy::Bound::Upper);
            v.bound = legacy::Bound::Exact;
            v.quality.depth = 2;
            v.quality.solved = false;
            ed.record_imported(b, a, v).unwrap();
        }
        let mut cfg = config(2, Selectivity::Full, 0);
        cfg.allow_imported = allow_imported;
        ed.configure_generation(&[b], cfg).unwrap();
        let report = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
        assert_eq!(report.profile.move_searches, u64::from(!allow_imported));
    }
}
#[test]
fn a6_forced_pass_keeps_placement_depth_and_the_zero_square_scope() {
    let pass = (1..512)
        .map(|seed| position(seed, 4))
        .find(|b| {
            b.must_pass()
                && actions(b.play(Action::Pass).unwrap())
                    .iter()
                    .any(|&a| !b.play(Action::Pass).unwrap().play(a).unwrap().is_terminal())
        })
        .unwrap();
    let child = pass.play(Action::Pass).unwrap().canonical();
    let mut eng = engine();
    let raw = search(
        &mut eng,
        pass,
        SearchKind::Midgame {
            depth: 2,
            selectivity: Selectivity::Full,
        },
        0,
    )
    .best_move_value()
    .unwrap();
    let (EvaluationSource::Search(trace) | EvaluationSource::SearchMove(trace)) = raw.source()
    else {
        panic!()
    };
    assert_eq!(trace.moves, 0);
    assert_eq!(trace.best, 64);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.record_candidate(pass, Action::Pass, raw).unwrap();
    ed.register_move(pass, Action::Pass).unwrap();
    ed.configure_generation(&[child], config(2, Selectivity::Full, 0))
        .unwrap();
    ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let backup = ed.move_values(pass, Action::Pass).unwrap().child.unwrap();
    assert_eq!(backup.conditions().midgame_depth(), Some(2));
    assert_eq!(
        ed.move_values(pass, Action::Pass).unwrap().direct,
        Some(raw)
    );
    let dir = Temp::new();
    ed.save(dir.path("pass.work")).unwrap();
    let ed = BookEditor::load(dir.path("pass.work"), ReadLimits::default()).unwrap();
    assert_eq!(
        ed.move_values(pass, Action::Pass).unwrap().child,
        Some(backup)
    );
}
#[test]
fn independent_position_proof_survives_manual_book_values_and_propagates_separately() {
    let eng = engine();
    let parent = position(123, 7);
    let link = actions(parent)[0];
    let b = parent.play(link).unwrap().canonical();
    let aa = actions(b);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.register_move(parent, link).unwrap();
    for &a in &aa {
        ed.record_imported(
            b,
            a,
            legacy(
                ed.identity(),
                b,
                -brute(b.play(a).unwrap()),
                legacy::Bound::Exact,
            ),
        )
        .unwrap();
    }
    let exact = brute(b);
    let mut manual = legacy(ed.identity(), b, 64, legacy::Bound::Upper);
    manual.bound = legacy::Bound::Exact;
    manual.quality.solved = false;
    manual.origin = ValueOrigin::Manual;
    ed.record_imported(b, aa[0], manual).unwrap();
    let book = ed.export().unwrap();
    let entry = book.probe(b).unwrap();
    assert_eq!(entry.value().unwrap().score(), 64);
    assert!(!entry.value().unwrap().is_solved());
    assert_eq!(
        entry.final_score().unwrap().result(),
        Score::Final(ScoreInterval::point(exact).unwrap())
    );
    let ev = ed.move_values(parent, link).unwrap();
    assert_eq!(ev.child.unwrap().score(), -64);
    assert_eq!(ev.child_final_score.unwrap().score(), -exact);
    assert_eq!(ev.final_score.unwrap().score(), -exact);
    assert!(ev.selected.unwrap().is_solved());
    let dir = Temp::new();
    book.save(dir.path("manual.book")).unwrap();
    let loaded = Book::load(dir.path("manual.book"), ReadLimits::default()).unwrap();
    assert_eq!(loaded.probe(b).unwrap().final_score(), entry.final_score());
    assert_eq!(loaded.probe(b).unwrap().value(), entry.value());
    ed.save(dir.path("manual.work")).unwrap();
    let mut loaded = BookEditor::load(dir.path("manual.work"), ReadLimits::default()).unwrap();
    loaded.clear_manual(b, aa[0]).unwrap();
    assert_eq!(
        loaded
            .export()
            .unwrap()
            .probe(b)
            .unwrap()
            .value()
            .unwrap()
            .score(),
        exact
    );
}
#[test]
fn a8_failed_search_publication_rolls_back_raw_and_derived_evidence() {
    for ancestor in [false, true] {
        let mut eng = engine();
        let parent = position(123, 6);
        let link = actions(parent)[0];
        let b = if ancestor {
            parent.play(link).unwrap().canonical()
        } else {
            parent
        };
        let mut ed = BookEditor::for_engine(&eng).unwrap();
        if ancestor {
            ed.register_move(parent, link).unwrap();
            ed.record_imported(
                parent,
                link,
                legacy(ed.identity(), parent, 64, legacy::Bound::Exact),
            )
            .unwrap();
        } else {
            for a in actions(b) {
                ed.record_imported(
                    b,
                    a,
                    legacy(ed.identity(), b, 64, legacy::Bound::ProvenLower),
                )
                .unwrap();
            }
        }
        ed.configure_generation(&[b], config(b.empty_count(), Selectivity::Full, 0))
            .unwrap();
        let revision = ed.revision();
        let before: Vec<_> = actions(b).iter().map(|&a| ed.move_values(b, a)).collect();
        assert!(ed.generate(&mut eng, budget(), StopHandle::new()).is_err());
        assert_eq!(ed.revision(), revision);
        assert_eq!(
            actions(b)
                .iter()
                .map(|&a| ed.move_values(b, a))
                .collect::<Vec<_>>(),
            before
        );
        assert!(ed.generation_config().is_some());
        assert_eq!(
            ed.generation_runs().last().unwrap().status,
            RunStatus::Failed
        );
        let dir = Temp::new();
        ed.save(dir.path("failed.work")).unwrap();
        let loaded = BookEditor::load(dir.path("failed.work"), ReadLimits::default()).unwrap();
        assert_eq!(loaded.revision(), revision);
        assert_eq!(eng.options().threads, 1);
        assert_eq!(eng.options().tt_mib, 1);
    }
}
#[test]
fn a6_unlink_preserves_reachable_jobs_when_the_saved_memory_budget_is_exceeded() {
    let eng = engine();
    let b = position(123, 9);
    let aa = actions(b);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.register_move(b, aa[0]).unwrap();
    let mut cfg = config(2, Selectivity::Full, 1);
    cfg.max_memory_bytes = ed.estimated_bytes() + 16384;
    ed.configure_generation(&[b], cfg.clone()).unwrap();
    for index in 0..128 {
        let a = aa[index % aa.len()];
        let mut value = legacy(
            ed.identity(),
            b,
            (index % 64) as i16 - 32,
            legacy::Bound::Upper,
        );
        value.bound = legacy::Bound::Exact;
        value.quality.solved = false;
        ed.record_imported(b, a, value).unwrap();
    }
    assert!(ed.estimated_bytes() > cfg.max_memory_bytes);
    ed.unregister_move(b, aa[0]).unwrap();
    assert_eq!(ed.generation_config(), Some(&cfg));
    let dir = Temp::new();
    ed.save(dir.path("over-budget.work")).unwrap();
    let loaded = BookEditor::load(dir.path("over-budget.work"), ReadLimits::default()).unwrap();
    assert_eq!(loaded.generation_config(), Some(&cfg));
}
