use deft_reversi_book::legacy::{self, AnalysisQuality, ImportedValue, ValueOrigin};
use deft_reversi_book::*;
use deft_reversi_engine::*;
fn engine() -> Engine {
    Engine::new(
        Model::from_pattern(PatternModel::zero()),
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
fn position(mut seed: u64, empties: u8) -> Board {
    let mut b = Board::new();
    while b.empty_count() > empties && !b.is_terminal() {
        let a = actions(b);
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        b = b.play(a[seed as usize % a.len()]).unwrap();
    }
    b
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
fn cfg(depth: u8, plies: u8) -> GenerationConfig {
    GenerationConfig {
        allow_imported: true,
        analysis: AnalysisSchedule::Stages(vec![Stage {
            depth,
            selectivity: Selectivity::Full,
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
#[test]
fn one_root_search_covers_unexpanded_moves_without_fabricating_point_values() {
    let mut eng = engine();
    let b = Board::new();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.configure_generation(&[b], cfg(4, 0)).unwrap();
    let r = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    let book = ed.export().unwrap();
    let view = book.probe(b).unwrap();
    assert_eq!(view.value().unwrap().score(), 0);
    assert!(!view.all_moves_evaluated());
    assert_eq!(
        view.candidates()
            .filter(|c| c.value.unwrap().bound() == Bound::Point)
            .count(),
        1
    );
    assert_eq!(
        view.candidates()
            .filter(|c| c.value.unwrap().bound() == Bound::Upper)
            .count(),
        3
    );
    assert_eq!(r.completed_stages, 1);
}
#[test]
fn solved_subset_bounds_combine_to_a_proven_position_and_survive_files() {
    let mut eng = engine();
    let b = position(123, 6);
    assert!(actions(b).len() > 1);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.configure_generation(&[b], cfg(6, 0)).unwrap();
    ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let book = ed.export().unwrap();
    let view = book.probe(b).unwrap();
    let value = view.value().unwrap();
    assert_eq!(value.score(), brute(b));
    assert!(value.is_solved());
    for c in view.candidates() {
        let v = c.value.unwrap();
        let real = -brute(b.play(c.action).unwrap());
        match v.bound() {
            Bound::Point => {
                assert_eq!(v.score(), real);
                assert!(v.is_solved())
            }
            Bound::Upper => {
                assert!(v.score() >= real);
                assert!(!v.is_solved())
            }
            _ => panic!("wrong bound"),
        }
    }
    let folder =
        std::env::temp_dir().join(format!("deft-frontier-final_score-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let work = folder.join("work");
    let play = folder.join("book");
    ed.save(&work).unwrap();
    book.save(&play).unwrap();
    assert_eq!(
        BookEditor::load(&work, ReadLimits::default())
            .unwrap()
            .export()
            .unwrap()
            .probe(b)
            .unwrap()
            .value(),
        Some(value)
    );
    assert_eq!(
        Book::load(&play, ReadLimits::default())
            .unwrap()
            .probe(b)
            .unwrap()
            .value(),
        Some(value)
    );
    std::fs::remove_dir_all(folder).unwrap();
}
#[test]
fn best_remaining_moves_are_expanded_until_loss_limits_are_exhausted() {
    let mut eng = engine();
    let b = position(94, 5);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.configure_generation(&[b], cfg(5, 1)).unwrap();
    let r = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    let book = ed.export().unwrap();
    let best = brute(b);
    for c in book.probe(b).unwrap().candidates() {
        let v = -brute(b.play(c.action).unwrap());
        assert_eq!(c.registered, v >= best - 4);
    }
}

#[test]
fn independent_roots_run_in_bounded_batches_and_restore_resources_after_stop() {
    let mut eng = Engine::new(
        Model::from_pattern(PatternModel::zero()),
        EngineOptions::default().threads(4).tt_mib(4),
    )
    .unwrap();
    let roots: Vec<_> = (121..=128).map(|seed| position(seed, 6)).collect();
    let mut config = cfg(6, 0);
    config.max_parallel_jobs = 2;
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.configure_generation(&roots, config).unwrap();
    let stopped = ed
        .generate(
            &mut eng,
            GenerationLimits {
                nodes: Some(17),
                ..Default::default()
            },
            StopHandle::new(),
        )
        .unwrap();
    assert_eq!(stopped.reason, GenerationStop::NodeLimit);
    assert!(stopped.nodes <= 17);
    assert_eq!(stopped.profile.max_parallel_jobs, 2);
    assert_eq!(eng.options().threads, 4);
    assert_eq!(eng.options().tt_mib, 4);
    let folder = std::env::temp_dir().join(format!("deft-frontier-batch-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("work");
    ed.save(&path).unwrap();
    let mut ed = BookEditor::load(&path, ReadLimits::default()).unwrap();
    let done = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(done.reason, GenerationStop::Exhausted);
    assert_eq!(done.profile.max_parallel_jobs, 2);
    for b in roots {
        let value = ed.export().unwrap().probe(b).unwrap().value().unwrap();
        assert!(value.is_solved());
        assert_eq!(value.score(), brute(b));
    }
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn pv_growth_stop_and_checkpoint_error_leave_all_work_resumable() {
    let mut eng = engine();
    let b = position(123, 6);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let mut config = cfg(6, 3);
    config.line_plies = 2;
    ed.configure_generation(&[b], config).unwrap();
    let first = ed
        .generate(
            &mut eng,
            GenerationLimits {
                new_positions: Some(2),
                ..Default::default()
            },
            StopHandle::new(),
        )
        .unwrap();
    assert_eq!(first.reason, GenerationStop::PositionLimit);
    assert_eq!(first.new_positions, 2);
    let folder = std::env::temp_dir().join(format!("deft-frontier-pv-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("work");
    ed.save(&path).unwrap();
    let mut ed = BookEditor::load(&path, ReadLimits::default()).unwrap();
    assert!(
        ed.generate_with_checkpoint(&mut eng, budget(), StopHandle::new(), |_, _| Err(
            BookError::Invalid("checkpoint failure".into())
        ))
        .is_err()
    );
    assert!(ed.generation_config().is_some());
    ed.validate().unwrap();
    let done = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(done.reason, GenerationStop::Exhausted);
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
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn pv_burst_lengths_preserve_the_same_proven_loss_bounded_graph() {
    use std::collections::{BTreeMap, BTreeSet};
    let root = position(123, 6);
    let mut reference = None;
    for length in [0, 1, 8] {
        let mut eng = engine();
        let mut ed = BookEditor::for_engine(&eng).unwrap();
        let mut config = cfg(6, 3);
        config.line_plies = length;
        config.cumulative_loss = Some(4);
        config.leaf_loss = Some(4);
        ed.configure_generation(&[root], config).unwrap();
        assert_eq!(
            ed.generate(&mut eng, budget(), StopHandle::new())
                .unwrap()
                .reason,
            GenerationStop::Exhausted
        );
        let book = ed.export().unwrap();
        let mut todo = vec![(root, 0u8, 0i16)];
        let mut paths = BTreeSet::new();
        let mut links = BTreeMap::new();
        while let Some((board, distance, loss)) = todo.pop() {
            let board = board.canonical();
            let key = (board.player_bits(), board.opponent_bits());
            if !paths.insert((key, distance, loss)) {
                continue;
            }
            assert!(distance <= 3 && loss <= 4);
            let entry = book.probe(board).unwrap();
            let value = entry.value().unwrap();
            assert!(value.is_solved());
            assert_eq!(value.score(), brute(board));
            let registered: Vec<_> = entry
                .candidates()
                .filter(|c| c.registered)
                .map(|c| c.action)
                .collect();
            links.insert(key, registered.clone());
            for action in registered {
                let child = board.play(action).unwrap();
                let local = value.score() + brute(child);
                assert!((0..=4).contains(&local));
                todo.push((
                    child,
                    distance + u8::from(matches!(action, Action::Place(_))),
                    loss + local,
                ));
            }
        }
        assert!(links.len() > 3);
        if let Some(expected) = &reference {
            assert_eq!(&links, expected);
        } else {
            reference = Some(links);
        }
    }
}

#[test]
fn retained_links_outside_a_reduced_boundary_receive_missing_root_coverage() {
    for old_points in [false, true] {
        let mut eng = engine();
        let board = Board::new();
        let mut ed = BookEditor::for_engine(&eng).unwrap();
        for action in actions(board) {
            ed.register_move(board, action).unwrap();
            if old_points {
                ed.record_candidate(
                    board,
                    action,
                    Evaluation::import(ImportedValue {
                        score: 7,
                        quality: AnalysisQuality {
                            depth: 2,
                            selectivity: Selectivity::Full,
                            solved: false,
                        },
                        bound: legacy::Bound::Exact,
                        identity: ed.identity(),
                        origin: ValueOrigin::Search,
                    })
                    .unwrap(),
                )
                .unwrap();
            }
        }
        ed.configure_generation(&[board], cfg(4, 0)).unwrap();
        let report = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
        assert_eq!(report.reason, GenerationStop::Exhausted);
        assert_eq!(report.new_positions, 0);
        assert_eq!(ed.len(), 2);
        assert_eq!(ed.stats().registered_moves, 4);
        let book = ed.export().unwrap();
        let value = book.probe(board).unwrap().value().unwrap();
        assert_eq!(value.score(), 0);
        assert_eq!(value.depth(), 4);
        assert!(!book.probe(board).unwrap().all_moves_evaluated());
        let mut player = BookPlayer::new(
            &eng,
            Some(std::sync::Arc::new(book)),
            UsePolicy {
                min_depth: 4,
                min_selectivity: Selectivity::Full,
                ..Default::default()
            },
            0,
        )
        .unwrap();
        assert!(player.choose(board).is_some());
    }
}

#[test]
fn certified_upper_only_coverage_is_refined_without_losing_the_old_evidence() {
    for registered in [false, true] {
        let mut eng = engine();
        let board = position(123, 6);
        let mut ed = BookEditor::for_engine(&eng).unwrap();
        for action in actions(board) {
            ed.record_candidate(
                board,
                action,
                Evaluation::import(ImportedValue {
                    score: 64,
                    quality: AnalysisQuality {
                        depth: 6,
                        selectivity: Selectivity::Full,
                        solved: false,
                    },
                    bound: legacy::Bound::ProvenUpper,
                    identity: ed.identity(),
                    origin: ValueOrigin::Imported,
                })
                .unwrap(),
            )
            .unwrap();
            if registered {
                ed.register_move(board, action).unwrap();
            }
        }
        let upper = ed.export().unwrap().probe(board).unwrap().value().unwrap();
        assert_eq!(
            upper.result(),
            Score::Final(ScoreInterval::new(None, Some(64)).unwrap())
        );
        assert!(!upper.is_solved());
        ed.configure_generation(&[board], cfg(2, 0)).unwrap();
        let report = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
        assert_eq!(report.reason, GenerationStop::Exhausted);
        let book = ed.export().unwrap();
        let entry = book.probe(board).unwrap();
        let value = entry.value().unwrap();
        assert!(value.is_solved());
        assert_eq!(value.score(), brute(board));
        assert_eq!(report.profile.move_searches, 1);
        for c in entry
            .candidates()
            .filter(|c| c.value.unwrap().bound() == Bound::Upper)
        {
            let evidence = ed.move_values(board, c.action).unwrap();
            assert_eq!(evidence.direct.unwrap().score(), 64);
            assert_eq!(evidence.selected.unwrap().score(), brute(board));
        }
    }
}
