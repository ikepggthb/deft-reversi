use deft_reversi_book::ImportedValue;
use deft_reversi_book::*;
use deft_reversi_engine::*;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "deft-review-regression-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn engine(threads: usize) -> Engine {
    Engine::new(
        Model::from_pattern(PatternModel::zero()),
        EngineOptions::default().threads(threads).tt_mib(4),
    )
    .unwrap()
}
fn biased_engine() -> Engine {
    // A real, self-contained evaluator. Its tempo term makes different search
    // horizons disagree without invented evidence or an external model file.
    let phase = PatternPhase::from_flat(vec![0; PATTERN_WEIGHT_COUNT], [0; 128], 128).unwrap();
    let pattern = PatternModel::new(vec![phase; PHASE_COUNT]).unwrap();
    Engine::new(
        Model::from_pattern(pattern),
        EngineOptions::default().tt_mib(4),
    )
    .unwrap()
}
fn actions(board: Board) -> Vec<Action> {
    let mut a: Vec<_> = board.legal_moves().map(Action::Place).collect();
    if board.must_pass() {
        a.push(Action::Pass);
    }
    a
}
fn position(mut seed: u64, empties: u8) -> Board {
    let mut board = Board::new();
    while board.empty_count() > empties && !board.is_terminal() {
        let a = actions(board);
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        board = board.play(a[seed as usize % a.len()]).unwrap();
    }
    board
}
fn brute(board: Board) -> i16 {
    if board.is_terminal() {
        board.final_score() as i16
    } else {
        actions(board)
            .into_iter()
            .map(|a| -brute(board.play(a).unwrap()))
            .max()
            .unwrap()
    }
}
fn config(depth: u8, plies: u8) -> GenerationConfig {
    GenerationConfig {
        analysis: AnalysisSchedule::Stages(vec![Stage {
            depth,
            selectivity: Selectivity::Full,
        }]),
        max_plies: plies,
        expansion_loss: 4,
        cumulative_loss: Some(4),
        leaf_loss: Some(4),
        checkpoint_interval: Duration::from_nanos(1),
        ..Default::default()
    }
}
fn budget() -> GenerationLimits {
    GenerationLimits {
        nodes: Some(2_000_000),
        ..Default::default()
    }
}
fn generate(ed: &mut BookEditor, engine: &mut Engine) {
    assert_eq!(
        ed.generate(engine, budget(), StopHandle::new())
            .unwrap()
            .reason,
        GenerationStop::Exhausted
    );
}
fn proven_value(ed: &BookEditor, board: Board, expected: i16) {
    let value = ed.export().unwrap().probe(board).unwrap().value().unwrap();
    assert!(value.quality().solved, "lost certificate: {value:?}");
    assert_eq!(value.score(), expected);
}

#[test]
fn full_root_certificate_survives_lower_depth_extension_and_restart() {
    let board = position(123, 6);
    let expected = brute(board);
    for (retained_link, threads) in [(false, 1), (true, 4), (false, 4), (true, 1)] {
        let mut engine = engine(threads);
        let mut ed = BookEditor::for_engine(&engine).unwrap();
        if retained_link {
            let other = actions(board)
                .into_iter()
                .find(|&a| {
                    let child = board.play(a).unwrap();
                    !child.is_terminal() && -brute(child) < expected
                })
                .unwrap();
            ed.register_move(board, other).unwrap();
        }
        ed.configure_generation(&[board], config(6, 0)).unwrap();
        generate(&mut ed, &mut engine);
        proven_value(&ed, board, expected);
        ed.configure_generation(&[board], config(2, 1)).unwrap();
        let stopped = ed
            .generate_with_checkpoint(
                &mut engine,
                GenerationLimits {
                    nodes: Some(1),
                    ..Default::default()
                },
                StopHandle::new(),
                |ed, _| {
                    proven_value(ed, board, expected);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(stopped.reason, GenerationStop::NodeLimit);
        let temp = Temp::new();
        ed.save(temp.file("work")).unwrap();
        let mut ed = BookEditor::load(temp.file("work"), ReadLimits::default()).unwrap();
        proven_value(&ed, board, expected);
        let mut checkpoints = 0;
        let done = ed
            .generate_with_checkpoint(&mut engine, budget(), StopHandle::new(), |ed, _| {
                checkpoints += 1;
                proven_value(ed, board, expected);
                Ok(())
            })
            .unwrap();
        assert_eq!(done.reason, GenerationStop::Exhausted);
        assert!(checkpoints > 1);
        let book = ed.export().unwrap();
        book.save(temp.file("play")).unwrap();
        let book = Book::load(temp.file("play"), ReadLimits::default()).unwrap();
        assert!(book.probe(board).unwrap().value().unwrap().quality().solved);
        let mut player = BookPlayer::new(
            &engine,
            Some(Arc::new(book)),
            UsePolicy {
                min_depth: 2,
                min_selectivity: Selectivity::Full,
                ..Default::default()
            },
            1,
        )
        .unwrap();
        let choice = player.choose(board).unwrap();
        assert!(choice.value.quality().solved);
        assert_eq!(-brute(board.play(choice.action).unwrap()), expected);
    }
}

#[test]
fn retiring_frontier_during_record_or_unregister_preserves_all_certificates() {
    let board = position(123, 6);
    let expected = brute(board);
    for unregister in [false, true] {
        let mut engine = engine(1);
        let mut ed = BookEditor::for_engine(&engine).unwrap();
        ed.configure_generation(&[board], config(6, 0)).unwrap();
        generate(&mut ed, &mut engine);
        let book = ed.export().unwrap();
        let best = book
            .probe(board)
            .unwrap()
            .candidates()
            .find(|c| c.value.unwrap().bound() == Bound::Exact)
            .unwrap()
            .action;
        let other = book
            .probe(board)
            .unwrap()
            .candidates()
            .find(|c| c.value.unwrap().bound() == Bound::ProvenUpper)
            .unwrap()
            .action;
        if unregister {
            ed.register_move(board, best).unwrap();
            ed.unregister_move(board, best).unwrap();
        } else {
            let mut value = book
                .probe(board)
                .unwrap()
                .candidates()
                .find(|c| c.action == other)
                .unwrap()
                .value
                .unwrap()
                .as_imported();
            value.bound = Bound::Exact;
            value.score = 0;
            value.quality = AnalysisQuality {
                depth: 2,
                selectivity: Selectivity::Full,
                solved: false,
            };
            let revision = ed.revision();
            assert!(ed.record_imported(board, other, value).is_err());
            assert_eq!(ed.revision(), revision);
            proven_value(&ed, board, expected);
            value.score = -brute(board.play(other).unwrap());
            value.quality.depth = board.empty_count();
            value.quality.solved = true;
            ed.record_imported(board, other, value).unwrap();
        }
        proven_value(&ed, board, expected);
        let other_engine = biased_engine();
        ed.retarget(&other_engine).unwrap();
        proven_value(&ed, board, expected);
        let temp = Temp::new();
        ed.save(temp.file("work")).unwrap();
        proven_value(
            &BookEditor::load(temp.file("work"), ReadLimits::default()).unwrap(),
            board,
            expected,
        );
    }
}

#[test]
fn book_policy_prefers_actual_child_backup_over_estimated_frontier() {
    let mut engine = biased_engine();
    let board = position(1, 30);
    let mut ed = BookEditor::for_engine(&engine).unwrap();
    ed.configure_generation(&[board], config(4, 0)).unwrap();
    generate(&mut ed, &mut engine);
    let book = ed.export().unwrap();
    let action = book
        .probe(board)
        .unwrap()
        .candidates()
        .find(|c| c.value.is_some_and(|v| v.bound() == Bound::Exact))
        .unwrap()
        .action;
    let child = board.play(action).unwrap();
    ed.register_move(board, action).unwrap();
    for a in actions(child) {
        let result = engine
            .search(child.play(a).unwrap(), &SearchRequest::new(Goal::Depth(1)))
            .unwrap();
        assert_eq!(result.stop_reason, StopReason::Completed);
        ed.record_candidate(
            child,
            a,
            Assessment::import(ImportedValue {
                score: -result.score.unwrap() as i16,
                quality: AnalysisQuality {
                    depth: 2,
                    selectivity: Selectivity::Full,
                    solved: false,
                },
                bound: Bound::Exact,
                identity: ed.identity(),
                origin: ValueOrigin::Search,
            })
            .unwrap(),
        )
        .unwrap();
    }
    let before = ed.candidate_evidence(board, action).unwrap();
    assert_ne!(
        before.direct.unwrap().score(),
        before.propagated.unwrap().score()
    );
    assert_eq!(before.effective, before.propagated);
    let ev = ed.candidate_evidence(board, action).unwrap();
    assert_eq!(ev.effective, ev.propagated);
    assert_eq!(ev.source, Some(ValueSource::Propagated));
    assert_eq!(ev.direct, before.direct);
    assert_eq!(ev.frontier, before.frontier);
    let temp = Temp::new();
    ed.save(temp.file("work")).unwrap();
    let ed = BookEditor::load(temp.file("work"), ReadLimits::default()).unwrap();
    assert_eq!(ed.candidate_evidence(board, action).unwrap(), ev);
    assert_eq!(
        ed.candidate_evidence(board, action).unwrap().effective,
        before.propagated
    );
}

#[test]
fn registration_after_configuration_obeys_boundary_and_survives_restart() {
    let board = position(1, 6);
    let action = Action::Place(Square::new(48).unwrap());
    let child = board.play(action).unwrap();
    assert!(!child.is_terminal());
    assert_eq!(brute(board) + brute(child), 4);
    for plies in [0, 1] {
        let mut engine = engine(1);
        let mut ed = BookEditor::for_engine(&engine).unwrap();
        ed.configure_generation(&[board], config(6, plies)).unwrap();
        ed.register_move(board, action).unwrap();
        let stop = ed
            .generate(
                &mut engine,
                GenerationLimits {
                    nodes: Some(0),
                    ..Default::default()
                },
                StopHandle::new(),
            )
            .unwrap();
        assert_eq!(stop.reason, GenerationStop::NodeLimit);
        let temp = Temp::new();
        ed.save(temp.file("work")).unwrap();
        let mut ed = BookEditor::load(temp.file("work"), ReadLimits::default()).unwrap();
        generate(&mut ed, &mut engine);
        let book = ed.export().unwrap();
        let value = book.probe(child).unwrap().value();
        if plies == 0 {
            assert!(value.is_none());
        } else {
            let value = value.unwrap();
            assert!(value.quality().solved);
            assert_eq!(value.score(), brute(child));
        }
    }
}

#[test]
fn newly_connected_retained_subgraph_is_scheduled_to_the_placement_boundary() {
    let board = position(1, 6);
    let action = Action::Place(Square::new(48).unwrap());
    let child = board.play(action).unwrap();
    let reply = actions(child)
        .into_iter()
        .max_by_key(|&a| -brute(child.play(a).unwrap()))
        .unwrap();
    let descendant = child.play(reply).unwrap();
    assert!(!descendant.is_terminal());
    for plies in [1, 2] {
        let mut engine = engine(1);
        let mut ed = BookEditor::for_engine(&engine).unwrap();
        ed.register_move(child, reply).unwrap();
        ed.configure_generation(&[board], config(6, plies)).unwrap();
        ed.register_move(board, action).unwrap();
        generate(&mut ed, &mut engine);
        let book = ed.export().unwrap();
        assert!(book.probe(child).unwrap().value().unwrap().quality().solved);
        let value = book.probe(descendant).unwrap().value();
        assert_eq!(value.is_some(), plies == 2);
        if let Some(value) = value {
            assert!(value.quality().solved);
            assert_eq!(value.score(), brute(descendant));
        }
    }
}

#[test]
fn registered_lower_certificate_keeps_its_upper_coverage_when_subset_changes() {
    let board = position(123, 6);
    let expected = brute(board);
    let retained = actions(board)
        .into_iter()
        .find(|&a| {
            let child = board.play(a).unwrap();
            !child.is_terminal() && -brute(child) < expected
        })
        .unwrap();
    for _ in 0..1 {
        let mut engine = engine(1);
        let mut ed = BookEditor::for_engine(&engine).unwrap();
        ed.register_move(board, retained).unwrap();
        ed.record_candidate(
            board,
            retained,
            Assessment::import(ImportedValue {
                score: -64,
                quality: AnalysisQuality {
                    depth: 6,
                    selectivity: Selectivity::Full,
                    solved: false,
                },
                bound: Bound::ProvenLower,
                identity: ed.identity(),
                origin: ValueOrigin::Imported,
            })
            .unwrap(),
        )
        .unwrap();
        ed.configure_generation(&[board], config(6, 0)).unwrap();
        generate(&mut ed, &mut engine);
        proven_value(&ed, board, expected);
        assert_eq!(
            ed.candidate_evidence(board, retained)
                .unwrap()
                .direct
                .unwrap()
                .bound(),
            Bound::ProvenLower
        );
        ed.configure_generation(&[board], config(2, 1)).unwrap();
        let result = ed
            .generate_with_checkpoint(&mut engine, budget(), StopHandle::new(), |ed, _| {
                proven_value(ed, board, expected);
                Ok(())
            })
            .unwrap();
        assert_eq!(result.reason, GenerationStop::Exhausted);
        proven_value(&ed, board, expected);
        let book = ed.export().unwrap();
        let best = book
            .probe(board)
            .unwrap()
            .candidates()
            .find(|c| {
                c.value
                    .is_some_and(|v| v.quality().solved && v.score() == expected)
            })
            .unwrap();
        // A no-op replacement and a link removal must keep the aggregate's
        // complementary upper even if a direct lower occupies that edge slot.
        ed.record_candidate(board, best.action, best.value.unwrap())
            .unwrap();
        proven_value(&ed, board, expected);
        ed.unregister_move(board, best.action).unwrap();
        proven_value(&ed, board, expected);
        let temp = Temp::new();
        ed.save(temp.file("work")).unwrap();
        proven_value(
            &BookEditor::load(temp.file("work"), ReadLimits::default()).unwrap(),
            board,
            expected,
        );
    }
}
