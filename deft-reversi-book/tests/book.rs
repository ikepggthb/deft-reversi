use deft_reversi_book::legacy::{self, AnalysisQuality, ImportedValue, ValueOrigin};
use deft_reversi_book::*;
use deft_reversi_engine::*;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
static TEMP: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "deft-book-test-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, n: &str) -> PathBuf {
        self.0.join(n)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
/// The zero model with the legacy main rules, so selective analyses run.
fn engine() -> Engine {
    let model = Model::from_pattern(PatternModel::zero())
        .with_calibration(legacy_main_rules(), ProbCutSigmas::NORMAL)
        .unwrap();
    Engine::new(model, EngineOptions::default().tt_mib(1)).unwrap()
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
        return b.final_score() as i16;
    }
    actions(b)
        .into_iter()
        .map(|a| -brute(b.play(a).unwrap()))
        .max()
        .unwrap()
}
fn compat_policy() -> UsePolicy {
    UsePolicy {
        allow_imported: true,
        allow_manual: true,
        ..UsePolicy::default()
    }
}
fn estimate(identity: AnalysisIdentity, score: i16) -> Evaluation {
    Evaluation::import(ImportedValue {
        score,
        quality: AnalysisQuality {
            depth: 12,
            selectivity: Selectivity::Percent95,
            solved: false,
        },
        bound: legacy::Bound::Exact,
        identity,
        origin: ValueOrigin::Search,
    })
    .unwrap()
}
fn config(depth: u8, plies: u8) -> GenerationConfig {
    GenerationConfig {
        allow_imported: true,
        analysis: AnalysisSchedule::Stages(vec![Stage {
            depth,
            selectivity: Selectivity::Full,
        }]),
        max_plies: plies,
        expansion_loss: 128,
        ..GenerationConfig::default()
    }
}
fn budget() -> GenerationLimits {
    GenerationLimits {
        nodes: Some(2_000_000),
        ..GenerationLimits::default()
    }
}
#[test]
fn transforms_restore_board_and_moves_and_share_one_key() {
    let b = position(42, 32);
    let id = [7; 32];
    let mut e = BookEditor::new(id);
    for (i, a) in actions(b).into_iter().enumerate() {
        e.register_move(b, a).unwrap();
        e.record_candidate(b, a, estimate(id, i as i16)).unwrap();
    }
    let book = e.export().unwrap();
    for t in Symmetry::all() {
        assert_eq!(t.inverse().board(t.board(b)), b);
        let view = book.probe(t.board(b)).unwrap();
        assert!(view.all_moves_evaluated());
        for c in book.probe(b).unwrap().candidates() {
            let expected = t.action(c.action);
            assert_eq!(t.inverse().action(expected), c.action);
            assert!(t.board(b).play(expected).is_ok());
            let found = view.candidates().find(|c| c.action == expected).unwrap();
            assert_eq!(found.value, c.value);
        }
    }
    assert_eq!(book.stats().positions, e.len());
}
#[test]
fn sparse_book_defaults_to_registered_candidates_and_strict_falls_back() {
    let eng = engine();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let b = position(17, 40);
    let a = actions(b)[0];
    ed.register_move(b, a).unwrap();
    ed.record_candidate(b, a, estimate(ed.identity(), 4))
        .unwrap();
    let book = Arc::new(ed.export().unwrap());
    assert!(!book.probe(b).unwrap().all_moves_evaluated());
    let mut p = BookPlayer::new(&eng, Some(book), compat_policy(), 0).unwrap();
    let r = p.choose(b);
    assert!(matches!(r,Some(BookChoice{action,..}) if action==a));
    p.set_policy(UsePolicy {
        require_all_evaluated: true,
        ..compat_policy()
    })
    .unwrap();
    assert!(p.choose(b).is_none());
}
#[test]
fn unknown_frontier_bounds_and_model_mismatch_are_not_played() {
    let eng = engine();
    let b = position(31, 42);
    let a = actions(b)[0];
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let id = ed.identity();
    ed.record_candidate(b, a, estimate(id, 3)).unwrap();
    let mut p = BookPlayer::new(
        &eng,
        Some(Arc::new(ed.export().unwrap())),
        compat_policy(),
        1,
    )
    .unwrap();
    assert!(p.choose(b).is_none());
    ed.register_move(b, a).unwrap();
    let mut upper = ImportedValue::from(estimate(id, 5));
    upper.bound = legacy::Bound::Upper;
    let mut bounded = BookEditor::new(id);
    bounded.register_move(b, a).unwrap();
    bounded.record_imported(b, a, upper).unwrap();
    p.set_book(Some(Arc::new(bounded.export().unwrap())));
    assert!(p.choose(b).is_none());
    let mut wrong = BookEditor::new([9; 32]);
    wrong.register_move(b, a).unwrap();
    wrong.record_candidate(b, a, estimate([9; 32], 64)).unwrap();
    p.set_book(Some(Arc::new(wrong.export().unwrap())));
    assert!(p.choose(b).is_none());
}
#[test]
fn terminal_and_unknown_positions_return_no_book_move() {
    let eng = engine();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let b = Board::new();
    for a in actions(b) {
        ed.register_move(b, a).unwrap();
        ed.record_candidate(b, a, estimate(ed.identity(), 0))
            .unwrap();
    }
    let mut p = BookPlayer::new(
        &eng,
        Some(Arc::new(ed.export().unwrap())),
        compat_policy(),
        1,
    )
    .unwrap();
    assert!(p.choose(b).is_some());
    assert!(p.choose(Board::from_bits(u64::MAX, 0).unwrap()).is_none());
    assert!(p.choose(position(5, 40)).is_none());
    p.set_book(None);
    assert!(p.choose(b).is_none());
}
#[test]
fn a_registered_move_known_to_lose_more_than_allowed_is_not_played() {
    let eng = engine();
    let b = position(94, 36);
    let aa = actions(b);
    let mut e = BookEditor::for_engine(&eng).unwrap();
    // The registered move is two discs worse than an evaluated frontier move.
    e.register_move(b, aa[0]).unwrap();
    e.record_candidate(b, aa[0], estimate(e.identity(), 2))
        .unwrap();
    e.record_candidate(b, aa[1], estimate(e.identity(), 4))
        .unwrap();
    let book = Arc::new(e.export().unwrap());
    let mut best = BookPlayer::new(&eng, Some(book.clone()), compat_policy(), 1).unwrap();
    assert!(best.choose(b).is_none());
    let tolerant = UsePolicy {
        moves: MovePolicy::RandomWithinLoss { max_loss: 2 },
        ..compat_policy()
    };
    let mut within = BookPlayer::new(&eng, Some(book), tolerant, 1).unwrap();
    assert_eq!(within.choose(b).unwrap().action, aa[0]);
}
#[test]
fn random_selection_is_seeded_and_stays_within_loss() {
    let b = position(94, 36);
    let mut e = BookEditor::for_engine(&engine()).unwrap();
    let aa = actions(b);
    assert!(aa.len() >= 3);
    for (i, &a) in aa.iter().enumerate() {
        e.register_move(b, a).unwrap();
        e.record_candidate(b, a, estimate(e.identity(), 10 - i as i16))
            .unwrap();
    }
    let book = Arc::new(e.export().unwrap());
    let policy = UsePolicy {
        moves: MovePolicy::RandomWithinLoss { max_loss: 1 },
        ..compat_policy()
    };
    let mut p = BookPlayer::new(&engine(), Some(book.clone()), policy, 123).unwrap();
    let mut q = BookPlayer::new(&engine(), Some(book), policy, 123).unwrap();
    let mut seen = vec![];
    for _ in 0..128 {
        let Some(a) = p.choose(b) else { panic!() };
        let Some(c) = q.choose(b) else { panic!() };
        assert_eq!(a.action, c.action);
        assert!(a.value.score() >= 9);
        seen.push(a.action);
    }
    assert!(seen.contains(&aa[0]) && seen.contains(&aa[1]));
}
#[test]
fn small_solved_book_matches_exhaustive_and_survives_model_change() {
    let mut eng = engine();
    let b = position(2, 5);
    assert!(!b.is_terminal());
    let mut e = BookEditor::for_engine(&eng).unwrap();
    e.configure_generation(&[b], config(8, 2)).unwrap();
    let r = e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    let book = e.export().unwrap();
    let view = book.probe(b).unwrap();
    assert!(view.all_moves_evaluated());
    assert_eq!(view.value().unwrap().score(), brute(b));
    assert!(view.value().unwrap().is_solved());
    for c in view.candidates() {
        assert_eq!(c.value.unwrap().score(), -brute(b.play(c.action).unwrap()));
    }
    // Fixed-depth Bound::Point was not enough; these candidates are terminal proofs.
    let mut other = engine();
    other.set_probcut_rules(vec![]).unwrap();
    let mut p = BookPlayer::new(&other, Some(Arc::new(book)), compat_policy(), 1).unwrap();
    let Some(choice) = p.choose(b) else { panic!() };
    assert_eq!(choice.value.score(), brute(b));
}
#[test]
fn forced_pass_and_terminal_round_trip() {
    let mut pass = None;
    for s in 1..200 {
        let b = position(s, 4);
        if b.must_pass() {
            pass = Some(b);
            break;
        }
    }
    let b = pass.expect("pass fixture");
    let mut eng = engine();
    let mut e = BookEditor::for_engine(&eng).unwrap();
    e.configure_generation(
        &[b],
        GenerationConfig {
            black_loss: Some(128),
            white_loss: Some(0),
            ..config(6, 1)
        },
    )
    .unwrap();
    e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let book = e.export().unwrap();
    let v = book.probe(b).unwrap();
    assert_eq!(v.candidates().next().unwrap().action, Action::Pass);
    assert_eq!(v.value().unwrap().score(), brute(b));
    // A pass changes absolute side without consuming a placement: this child
    // must use White's zero-loss width rather than Black's unrestricted width.
    let child = book.probe(b.play(Action::Pass).unwrap()).unwrap();
    let best = child
        .candidates()
        .map(|c| c.value.unwrap().score())
        .max()
        .unwrap();
    assert!(
        child
            .candidates()
            .all(|c| c.registered == (c.value.unwrap().score() == best))
    );
    let temp = Temp::new();
    let p = temp.file("pass.dbook");
    book.save(&p).unwrap();
    let loaded = Book::load(p, ReadLimits::default()).unwrap();
    assert_eq!(loaded.probe(b).unwrap().value().unwrap().score(), brute(b));
    let t = Board::from_bits(u64::MAX, 0).unwrap();
    e.insert_position(t).unwrap();
    let book = e.export().unwrap();
    assert!(book.probe(t).unwrap().is_terminal());
    book.save(temp.file("terminal")).unwrap();
    Book::load(temp.file("terminal"), ReadLimits::default()).unwrap();
}
#[test]
fn work_and_play_files_round_trip_deterministically() {
    let mut eng = engine();
    let mut e = BookEditor::for_engine(&eng).unwrap();
    let b = position(78, 7);
    e.configure_generation(&[b], config(2, 1)).unwrap();
    e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let temp = Temp::new();
    let p = temp.file("source");
    e.save(&p).unwrap();
    let e2 = BookEditor::load(&p, ReadLimits::default()).unwrap();
    e2.save(temp.file("copy")).unwrap();
    assert_eq!(
        std::fs::read(&p).unwrap(),
        std::fs::read(temp.file("copy")).unwrap()
    );
    let b1 = e.export().unwrap();
    b1.save(temp.file("play")).unwrap();
    let b2 = Book::load(temp.file("play"), ReadLimits::default()).unwrap();
    b2.save(temp.file("play2")).unwrap();
    assert_eq!(
        std::fs::read(temp.file("play")).unwrap(),
        std::fs::read(temp.file("play2")).unwrap()
    );
    assert_eq!(b1.stats().positions, b2.stats().positions);
    assert!(Book::load(&p, ReadLimits::default()).is_err());
    assert!(BookEditor::load(temp.file("play"), ReadLimits::default()).is_err());
}
#[test]
fn position_budget_keeps_unexpanded_frontier_for_resume() {
    let mut eng = engine();
    let b = position(44, 12);
    let mut e = BookEditor::for_engine(&eng).unwrap();
    e.configure_generation(&[b], config(2, 1)).unwrap();
    let r = e
        .generate(
            &mut eng,
            GenerationLimits {
                new_positions: Some(0),
                ..GenerationLimits::default()
            },
            StopHandle::new(),
        )
        .unwrap();
    assert_eq!(r.reason, GenerationStop::PositionLimit);
    assert_eq!(e.len(), 1);
    let temp = Temp::new();
    let p = temp.file("resume");
    e.save(&p).unwrap();
    let mut e = BookEditor::load(&p, ReadLimits::default()).unwrap();
    let r = e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    assert!(e.len() > 1);
    assert!(
        e.export()
            .unwrap()
            .probe(b)
            .unwrap()
            .candidates()
            .all(|c| c.registered)
    );
}
#[test]
fn node_budget_stops_without_publishing_partial_stage_and_resume_finishes() {
    let mut eng = engine();
    let b = position(15, 10);
    let mut e = BookEditor::for_engine(&eng).unwrap();
    e.configure_generation(&[b], config(6, 1)).unwrap();
    let r = e
        .generate(
            &mut eng,
            GenerationLimits {
                nodes: Some(1),
                ..GenerationLimits::default()
            },
            StopHandle::new(),
        )
        .unwrap();
    assert_eq!(r.reason, GenerationStop::NodeLimit);
    assert!(r.nodes <= 1);
    assert!(!e.export().unwrap().probe(b).unwrap().all_moves_evaluated());
    let temp = Temp::new();
    let p = temp.file("pending");
    e.save(&p).unwrap();
    let mut e = BookEditor::load(p, ReadLimits::default()).unwrap();
    let r = e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    assert!(e.export().unwrap().probe(b).unwrap().all_moves_evaluated());
}
#[test]
fn checkpoint_preserves_the_current_popped_job() {
    let mut eng = engine();
    let b = position(57, 12);
    let mut e = BookEditor::for_engine(&eng).unwrap();
    let mut c = config(4, 1);
    c.checkpoint_interval = Duration::from_nanos(1);
    e.configure_generation(&[b], c).unwrap();
    let temp = Temp::new();
    let p = temp.file("during");
    let mut calls = 0;
    let result = e.generate_with_checkpoint(&mut eng, budget(), StopHandle::new(), |e, _| {
        calls += 1;
        e.save(&p)?;
        if calls == 1 {
            return Err(BookError::Invalid("simulated callback failure".into()));
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(calls, 1);
    let mut resumed = BookEditor::load(&p, ReadLimits::default()).unwrap();
    let r = resumed
        .generate(&mut eng, budget(), StopHandle::new())
        .unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    assert!(
        resumed
            .export()
            .unwrap()
            .probe(b)
            .unwrap()
            .all_moves_evaluated()
    );
}
#[test]
fn external_stop_and_explicit_budget_validation() {
    let mut eng = engine();
    let mut e = BookEditor::for_engine(&eng).unwrap();
    e.configure_generation(&[Board::new()], config(2, 1))
        .unwrap();
    assert!(
        e.generate(&mut eng, GenerationLimits::default(), StopHandle::new())
            .is_err()
    );
    let stop = StopHandle::new();
    stop.stop();
    let r = e.generate(&mut eng, budget(), stop).unwrap();
    assert_eq!(r.reason, GenerationStop::Requested);
    assert_eq!(r.nodes, 0);
    assert_eq!(
        e.generate(
            &mut eng,
            GenerationLimits {
                time: Some(Duration::ZERO),
                ..GenerationLimits::default()
            },
            StopHandle::new()
        )
        .unwrap()
        .reason,
        GenerationStop::TimeLimit
    );
}
#[test]
fn removing_a_link_invalidates_derived_parent_and_repair_is_idempotent() {
    let mut eng = engine();
    let b = position(71, 7);
    let mut e = BookEditor::for_engine(&eng).unwrap();
    e.configure_generation(&[b], config(4, 1)).unwrap();
    e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let a = actions(b)[0];
    let revision = e.revision();
    e.unregister_move(b, a).unwrap();
    assert!(e.revision() > revision);
    assert!(
        !e.export()
            .unwrap()
            .probe(b)
            .unwrap()
            .candidates()
            .find(|c| c.action == a)
            .unwrap()
            .registered
    );
    e.repair().unwrap();
    let val = e.export().unwrap().probe(b).unwrap().value();
    e.repair().unwrap();
    assert_eq!(e.export().unwrap().probe(b).unwrap().value(), val);
}
#[test]
fn corruption_truncation_memory_limits_and_failed_save_are_safe() {
    let temp = Temp::new();
    let p = temp.file("book");
    let mut e = BookEditor::new([0; 32]);
    e.insert_position(Board::new()).unwrap();
    e.export().unwrap().save(&p).unwrap();
    let bytes = std::fs::read(&p).unwrap();
    for n in [0, 8, 55, bytes.len() - 1] {
        std::fs::write(temp.file("bad"), &bytes[..n]).unwrap();
        assert!(Book::load(temp.file("bad"), ReadLimits::default()).is_err());
    }
    let mut bad = bytes.clone();
    *bad.last_mut().unwrap() ^= 1;
    std::fs::write(temp.file("bad"), bad).unwrap();
    assert!(Book::load(temp.file("bad"), ReadLimits::default()).is_err());
    assert!(
        Book::load(
            &p,
            ReadLimits {
                max_positions: 0,
                ..ReadLimits::default()
            }
        )
        .is_err()
    );
    assert!(
        Book::load(
            &p,
            ReadLimits {
                max_bytes: 10,
                ..ReadLimits::default()
            }
        )
        .is_err()
    );
    let dir = temp.file("directory");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("marker"), b"keep").unwrap();
    assert!(e.export().unwrap().save(&dir).is_err());
    assert_eq!(std::fs::read(dir.join("marker")).unwrap(), b"keep");
    assert_eq!(std::fs::read(&p).unwrap(), bytes);
    assert!(
        !std::fs::read_dir(&temp.0).unwrap().any(|p| p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
}
#[test]
fn model_change_requires_explicit_retarget_and_preserves_solved_evidence() {
    let mut eng = engine();
    let mut e = BookEditor::for_engine(&eng).unwrap();
    let b = position(1, 6);
    e.configure_generation(&[b], config(2, 1)).unwrap();
    e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let old = e.identity();
    eng.set_probcut_rules(vec![]).unwrap();
    assert!(e.generate(&mut eng, budget(), StopHandle::new()).is_err());
    e.retarget(&eng).unwrap();
    assert_ne!(old, e.identity());
    assert!(e.generation_config().is_none());
    assert!(!e.export().unwrap().probe(b).unwrap().all_moves_evaluated());
}
#[test]
fn snapshot_is_shareable_and_lookup_has_bounded_storage() {
    fn shareable<T: Send + Sync>() {}
    shareable::<Book>();
    let mut e = BookEditor::new([1; 32]);
    e.insert_position(Board::new()).unwrap();
    let b = Arc::new(e.export().unwrap());
    let c = b.clone();
    std::thread::spawn(move || assert!(c.probe(Board::new()).is_some()))
        .join()
        .unwrap();
    assert!(b.stats().resident_bytes < 1024);
}

#[test]
fn manual_evidence_is_separate_and_not_overwritten_by_generation() {
    let mut eng = engine();
    let b = position(310, 10);
    let a = actions(b)[0];
    let mut e = BookEditor::for_engine(&eng).unwrap();
    e.register_move(b, a).unwrap();
    let mut manual = ImportedValue::from(estimate(e.identity(), 42));
    manual.origin = ValueOrigin::Manual;
    e.record_imported(b, a, manual).unwrap();
    e.configure_generation(&[b], config(2, 1)).unwrap();
    e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let book = e.export().unwrap();
    assert_eq!(
        book.probe(b)
            .unwrap()
            .candidates()
            .find(|c| c.action == a)
            .unwrap()
            .value,
        Some(Evaluation::import(manual).unwrap())
    );
    let temp = Temp::new();
    e.save(temp.file("manual")).unwrap();
    let loaded = BookEditor::load(temp.file("manual"), ReadLimits::default()).unwrap();
    assert_eq!(
        loaded
            .export()
            .unwrap()
            .probe(b)
            .unwrap()
            .candidates()
            .find(|c| c.action == a)
            .unwrap()
            .value,
        Some(Evaluation::import(manual).unwrap())
    );
}
#[test]
fn stage_upgrade_and_explicit_reanalysis_keep_completed_quality() {
    let mut eng = engine();
    let b = position(222, 9);
    let mut e = BookEditor::for_engine(&eng).unwrap();
    let mut c = config(2, 1);
    c.analysis.stages_mut().unwrap().push(Stage {
        depth: 4,
        selectivity: Selectivity::Full,
    });
    e.configure_generation(&[b], c).unwrap();
    let r = e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    assert!(
        e.export()
            .unwrap()
            .probe(b)
            .unwrap()
            .candidates()
            .all(|c| ImportedValue::from(c.value.unwrap()).quality.depth >= 4)
    );
    let mut c = config(2, 1);
    c.reuse_completed = false;
    e.configure_generation(&[b], c).unwrap();
    let r = e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert!(r.nodes > 0);
    assert!(
        e.export()
            .unwrap()
            .probe(b)
            .unwrap()
            .candidates()
            .all(|c| ImportedValue::from(c.value.unwrap()).quality.depth >= 4)
    );
}
#[test]
fn editor_and_model_management_do_not_change_search_results_or_node_counts() {
    let mut a = engine();
    let mut b = engine();
    let board = position(567, 9);
    let exact = SearchRequest::new(Goal::Endgame);
    let r1 = a.search(board, &exact).unwrap();
    let _ = BookEditor::for_engine(&b).unwrap();
    let r2 = b.search(board, &exact).unwrap();
    assert_eq!(r1.score, r2.score);
    assert_eq!(r1.stats.nodes, r2.stats.nodes);
}
#[test]
fn thousands_of_small_positions_round_trip_without_large_book_generation() {
    let mut e = BookEditor::new([0; 32]);
    let positions: Vec<_> = (1..=1000).map(|i| position(i, 30)).collect();
    for &b in &positions {
        e.insert_position(b).unwrap();
    }
    let book = e.export().unwrap();
    for &b in &positions {
        assert!(book.probe(b).is_some());
    }
    assert!(book.stats().resident_bytes < 1024 * 1024);
    let temp = Temp::new();
    book.save(temp.file("small")).unwrap();
    let book = Book::load(temp.file("small"), ReadLimits::default()).unwrap();
    for b in positions {
        assert!(book.probe(b).is_some());
    }
}

#[test]
fn managed_memory_limit_keeps_a_resumable_workspace() {
    let mut eng = engine();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let mut c = config(2, 24);
    c.max_memory_bytes = 32768;
    ed.configure_generation(&[Board::new()], c).unwrap();
    let r = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(r.reason, GenerationStop::MemoryLimit);
    assert!(ed.estimated_bytes() < 32768);
    let temp = Temp::new();
    ed.save(temp.file("memory")).unwrap();
    let mut ed = BookEditor::load(temp.file("memory"), ReadLimits::default()).unwrap();
    let mut c = config(2, 1);
    c.max_memory_bytes = 1024 * 1024;
    ed.configure_generation(&[Board::new()], c).unwrap();
    assert_eq!(
        ed.generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .reason,
        GenerationStop::Exhausted
    );
}
#[test]
fn weight_fingerprint_and_player_engine_replacement_are_checked() {
    let first = engine();
    let board = position(108, 30);
    let a = actions(board)[0];
    let mut e = BookEditor::for_engine(&first).unwrap();
    e.register_move(board, a).unwrap();
    e.record_candidate(board, a, estimate(e.identity(), 4))
        .unwrap();
    let book = Arc::new(e.export().unwrap());
    let mut player = BookPlayer::new(&first, Some(book), compat_policy(), 1).unwrap();
    let mut bytes = Vec::new();
    PatternModel::zero().write_to(&mut bytes).unwrap();
    let n = bytes.len();
    bytes[n - 2..].copy_from_slice(&128i16.to_le_bytes());
    let second = Engine::new(
        Model::from_bytes(&bytes).unwrap(),
        EngineOptions::default().tt_mib(1),
    )
    .unwrap();
    assert!(player.choose(board).is_some());
    player.set_engine(&second).unwrap();
    assert!(player.choose(board).is_none());
}

#[test]
fn an_inactive_registered_branch_is_skipped_and_can_reactivate() {
    let mut eng = engine();
    let b = position(94, 30);
    let aa = actions(b);
    assert!(aa.len() > 1);
    let bad = b.play(aa[0]).unwrap();
    let good = b.play(aa[1]).unwrap();
    assert_ne!(bad.canonical(), good.canonical());
    let mut e = BookEditor::for_engine(&eng).unwrap();
    for (i, &a) in aa.iter().enumerate() {
        e.register_move(b, a).unwrap();
        let mut v = ImportedValue::from(estimate(e.identity(), if i == 1 { 10 } else { -10 }));
        v.quality.selectivity = Selectivity::Full;
        e.record_imported(b, a, v).unwrap();
    }
    let mut c = config(2, 2);
    c.expansion_loss = 0;
    e.configure_generation(&[b], c).unwrap();
    e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert!(
        !e.export()
            .unwrap()
            .probe(bad)
            .unwrap()
            .all_moves_evaluated()
    );
    assert!(
        e.export()
            .unwrap()
            .probe(good)
            .unwrap()
            .all_moves_evaluated()
    );
    let mut v = ImportedValue::from(estimate(e.identity(), 20));
    v.quality.selectivity = Selectivity::Full;
    e.record_imported(b, aa[0], v).unwrap();
    e.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert!(
        e.export()
            .unwrap()
            .probe(bad)
            .unwrap()
            .all_moves_evaluated()
    );
}

#[test]
fn generation_tracks_side_widths_and_persists_contexts_without_duplicate_nodes() {
    let mut engine = engine();
    let b = position(317, 42);
    let moves = actions(b);
    assert!(moves.len() > 1);
    let id = BookEditor::for_engine(&engine).unwrap().identity();
    let prepare = || {
        let mut e = BookEditor::new(id);
        for (i, &a) in moves.iter().enumerate() {
            let mut v = ImportedValue::from(estimate(id, -(i as i16)));
            v.quality.selectivity = Selectivity::Full;
            e.record_imported(b, a, v).unwrap();
        }
        e
    };
    let cfg = GenerationConfig {
        black_loss: Some(0),
        white_loss: Some(128),
        new_job_weight: 1,
        deepen_job_weight: 2,
        ..config(2, 1)
    };
    let roots = [
        GenerationRoot {
            board: b,
            side: BookSide::Black,
        },
        GenerationRoot {
            board: b,
            side: BookSide::White,
        },
    ];
    let mut black = prepare();
    black
        .configure_generation_roots(&roots[..1], cfg.clone())
        .unwrap();
    let r = black
        .generate(&mut engine, budget(), StopHandle::new())
        .unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    let book = black.export().unwrap();
    assert_eq!(
        book.probe(b)
            .unwrap()
            .candidates()
            .filter(|c| c.registered)
            .count(),
        1
    );

    let mut both = prepare();
    both.configure_generation_roots(&roots, cfg.clone())
        .unwrap();
    let tmp = Temp::new();
    let path = tmp.file("colors.work");
    both.save(&path).unwrap();
    let mut both = BookEditor::load(&path, ReadLimits::default()).unwrap();
    assert_eq!(both.generation_config(), Some(&cfg));
    let starts = both.generation_starts();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0].side, BookSide::Black);
    assert_eq!(starts[1].side, BookSide::White);
    assert_eq!(starts[0].board, starts[1].board);
    assert_eq!(both.len(), 1);
    let r = both
        .generate(&mut engine, budget(), StopHandle::new())
        .unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    let book = both.export().unwrap();
    assert_eq!(
        book.probe(b)
            .unwrap()
            .candidates()
            .filter(|c| c.registered)
            .count(),
        moves.len()
    );
    both.save(&path).unwrap();
    let loaded = BookEditor::load(&path, ReadLimits::default()).unwrap();
    assert_eq!(loaded.len(), both.len());
    assert_eq!(loaded.generation_starts().len(), 2);
    let mut invalid = cfg;
    invalid.new_job_weight = 0;
    invalid.deepen_job_weight = 0;
    assert!(invalid.validate().is_err());
}

#[test]
fn generation_combines_incomparable_depth_and_selectivity_without_losing_progress() {
    let mut engine = engine();
    let b = position(222, 10);
    let mut ed = BookEditor::for_engine(&engine).unwrap();
    let mut old = ImportedValue::from(estimate(ed.identity(), 0));
    old.quality.depth = 2;
    old.quality.selectivity = Selectivity::Full;
    for a in actions(b) {
        ed.record_imported(b, a, old).unwrap();
    }
    let mut cfg = config(4, 0);
    cfg.analysis.stages_mut().unwrap()[0].selectivity = Selectivity::Percent95;
    ed.configure_generation(&[b], cfg).unwrap();
    let report = ed
        .generate(&mut engine, budget(), StopHandle::new())
        .unwrap();
    assert_eq!(report.reason, GenerationStop::Exhausted);
    let book = ed.export().unwrap();
    assert!(book.probe(b).unwrap().candidates().all(|c| {
        let q = ImportedValue::from(c.value.unwrap()).quality;
        q.depth >= 4 && q.selectivity == Selectivity::Full
    }));
    // The converse change (shallower but less selective) must also preserve
    // the existing deeper analysis and complete the newly requested quality.
    let mut ed = BookEditor::for_engine(&engine).unwrap();
    old.quality.depth = 4;
    old.quality.selectivity = Selectivity::Percent95;
    for a in actions(b) {
        ed.record_imported(b, a, old).unwrap();
    }
    ed.configure_generation(&[b], config(2, 0)).unwrap();
    ed.generate(&mut engine, budget(), StopHandle::new())
        .unwrap();
    let book = ed.export().unwrap();
    assert!(book.probe(b).unwrap().candidates().all(|c| {
        let q = ImportedValue::from(c.value.unwrap()).quality;
        q.depth >= 4 && q.selectivity == Selectivity::Full
    }));
}

#[test]
fn levels_resolve_at_children_and_survive_interruption_and_reconfiguration() {
    let mut engine = engine();
    let b = position(222, 8);
    let mut ed = BookEditor::for_engine(&engine).unwrap();
    let cfg = GenerationConfig {
        analysis: AnalysisSchedule::Levels(vec![1, 4]),
        max_plies: 0,
        ..Default::default()
    };
    ed.configure_generation(&[b], cfg.clone()).unwrap();
    let stopped = ed
        .generate(
            &mut engine,
            GenerationLimits {
                nodes: Some(1),
                ..Default::default()
            },
            StopHandle::new(),
        )
        .unwrap();
    assert_eq!(stopped.reason, GenerationStop::NodeLimit);
    assert!(!ed.export().unwrap().probe(b).unwrap().all_moves_evaluated());
    let temp = Temp::new();
    let path = temp.file("levels.work");
    ed.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 8);
    let mut ed = BookEditor::load(&path, ReadLimits::default()).unwrap();
    assert_eq!(ed.generation_config(), Some(&cfg));
    let r = ed
        .generate(&mut engine, budget(), StopHandle::new())
        .unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    let book = ed.export().unwrap();
    for c in book.probe(b).unwrap().candidates() {
        let v = c.value.unwrap();
        let exact = -brute(b.play(c.action).unwrap());
        if v.bound() == Bound::Point {
            assert!(v.is_solved());
            assert_eq!(v.score(), exact);
        } else {
            assert_eq!(v.bound(), Bound::Upper);
            assert!(v.score() >= exact);
            assert!(!v.is_solved());
        }
    }
    ed.configure_generation(
        &[b],
        GenerationConfig {
            analysis: AnalysisSchedule::Levels(vec![1]),
            max_plies: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let r = ed
        .generate(&mut engine, budget(), StopHandle::new())
        .unwrap();
    assert_eq!(
        r.nodes, 0,
        "proven results must be reused after level changes"
    );
}

#[test]
fn level_quality_records_actual_child_depth_instead_of_level_number() {
    let mut engine = engine();
    let b = Board::new();
    let mut ed = BookEditor::for_engine(&engine).unwrap();
    ed.configure_generation(
        &[b],
        GenerationConfig {
            analysis: AnalysisSchedule::Levels(vec![2]),
            max_plies: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        ed.generate(&mut engine, budget(), StopHandle::new())
            .unwrap()
            .reason,
        GenerationStop::Exhausted
    );
    let book = ed.export().unwrap();
    assert!(book.probe(b).unwrap().candidates().all(|c| {
        ImportedValue::from(c.value.unwrap()).quality
            == AnalysisQuality {
                depth: 3,
                selectivity: Selectivity::Full,
                solved: false,
            }
    }));
    for levels in [vec![], vec![0], vec![61], vec![2, 2], vec![3, 1]] {
        assert!(
            GenerationConfig {
                analysis: AnalysisSchedule::Levels(levels),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}

#[test]
fn engine_plan_distinguishes_selective_endgame_from_proof() {
    let engine = engine();
    for (empties, level, expected) in [
        (
            60,
            16,
            ResultKind::Depth {
                depth: 12,
                selectivity: Selectivity::Percent95,
            },
        ),
        (
            24,
            12,
            ResultKind::Endgame {
                selectivity: Selectivity::Percent98,
            },
        ),
        (
            20,
            12,
            ResultKind::Endgame {
                selectivity: Selectivity::Full,
            },
        ),
    ] {
        let b = position(123, empties);
        assert_eq!(b.empty_count(), empties);
        assert_eq!(
            engine
                .search_plan(b, &SearchRequest::new(Goal::Level(level)))
                .unwrap(),
            expected
        );
    }
    assert!(
        engine
            .search_plan(Board::new(), &SearchRequest::new(Goal::Level(0)))
            .is_err()
    );
    assert!(
        engine
            .search_plan(
                Board::new(),
                &SearchRequest::new(Goal::Level(2)).selectivity(Selectivity::Full)
            )
            .is_err()
    );
}

#[test]
fn cumulative_and_leaf_limits_control_new_links_and_reactivate_existing_paths() {
    let mut eng = engine();
    let b = position(94, 30);
    let root_actions = actions(b);
    let child = b.play(root_actions[0]).unwrap();
    let child_actions = actions(child);
    assert!(child_actions.len() > 1);
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let evidence = |score| {
        Evaluation::import(ImportedValue {
            score,
            quality: AnalysisQuality {
                depth: 25,
                selectivity: Selectivity::Full,
                solved: false,
            },
            bound: legacy::Bound::Exact,
            identity: ed.identity(),
            origin: ValueOrigin::Search,
        })
        .unwrap()
    };
    let root_values: Vec<_> = root_actions
        .iter()
        .enumerate()
        .map(|(i, _)| {
            evidence(if i == 0 {
                -3
            } else if i == 1 {
                0
            } else {
                -64
            })
        })
        .collect();
    let child_values: Vec<_> = child_actions
        .iter()
        .enumerate()
        .map(|(i, _)| {
            evidence(if i == 0 {
                -3
            } else if i == 1 {
                0
            } else {
                -64
            })
        })
        .collect();
    for (&a, v) in root_actions.iter().zip(root_values) {
        let mut v = ImportedValue::from(v);
        v.quality.depth = 28;
        v.origin = ValueOrigin::Manual;
        ed.record_imported(b, a, v).unwrap();
    }
    for (&a, v) in child_actions.iter().zip(child_values) {
        ed.record_candidate(child, a, v).unwrap();
    }
    let cfg = GenerationConfig {
        expansion_loss: 3,
        cumulative_loss: Some(5),
        leaf_loss: Some(3),
        ..config(2, 2)
    };
    ed.configure_generation(&[b], cfg.clone()).unwrap();
    ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    let registered = |ed: &BookEditor, board, a| {
        ed.export()
            .unwrap()
            .probe(board)
            .unwrap()
            .candidates()
            .find(|c| c.action == a)
            .unwrap()
            .registered
    };
    assert!(registered(&ed, b, root_actions[0]));
    assert!(
        !registered(&ed, child, child_actions[0]),
        "3+3 exceeds cumulative 5"
    );
    let mut v = ImportedValue::from(estimate(ed.identity(), -2));
    v.quality = AnalysisQuality {
        depth: 25,
        selectivity: Selectivity::Full,
        solved: false,
    };
    ed.record_imported(child, child_actions[0], v).unwrap();
    ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert!(
        registered(&ed, child, child_actions[0]),
        "3+2 must reactivate without reconfiguration"
    );
    // A strict leaf threshold only prevents new links; it does not discard an
    // existing continuation whose per-move and cumulative losses are allowed.
    let mut strict = cfg;
    strict.leaf_loss = Some(0);
    ed.configure_generation(&[b], strict.clone()).unwrap();
    ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert!(registered(&ed, b, root_actions[0]));
    let mut fresh = BookEditor::for_engine(&eng).unwrap();
    for c in ed.export().unwrap().probe(b).unwrap().candidates() {
        fresh
            .record_imported(b, c.action, ImportedValue::from(c.value.unwrap()))
            .unwrap();
    }
    fresh.configure_generation(&[b], strict.clone()).unwrap();
    fresh
        .generate(&mut eng, budget(), StopHandle::new())
        .unwrap();
    assert!(
        !registered(&fresh, b, root_actions[0]),
        "leaf 0 blocks a new loss-3 link even when per-move and cumulative limits allow it"
    );
    let temp = Temp::new();
    let path = temp.file("loss.work");
    ed.save(&path).unwrap();
    let mut loaded = BookEditor::load(&path, ReadLimits::default()).unwrap();
    assert_eq!(loaded.generation_config(), Some(&strict));
    assert_eq!(
        loaded
            .generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .nodes,
        0
    );
    assert!(
        GenerationConfig {
            cumulative_loss: Some(7681),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        GenerationConfig {
            leaf_loss: Some(129),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn old_search_identity_requires_retarget_and_retains_proofs_links_and_manual_records() {
    use sha2::{Digest, Sha256};
    let mut eng = engine();
    let mut model = Vec::new();
    eng.write_evaluation_model(&mut model).unwrap();
    let mut settings = Vec::new();
    eng.write_analysis_settings(&mut settings).unwrap();
    let prefix = b"deft-search-semantics".len() + 4;
    assert_eq!(&settings[..prefix - 4], b"deft-search-semantics");
    let mut legacy_hash = Sha256::new();
    legacy_hash.update(model);
    legacy_hash.update(b"deft-book-analysis-v1");
    legacy_hash.update(&settings[prefix..]);
    let old_id: AnalysisIdentity = legacy_hash.finalize().into();
    assert_ne!(old_id, analysis_identity(&eng).unwrap());
    let b = position(123, 8);
    let a = actions(b)[0];
    let mut ed = BookEditor::new(old_id);
    ed.register_move(b, a).unwrap();
    ed.record_candidate(b, a, estimate(old_id, 3)).unwrap();
    let mut manual = ImportedValue::from(estimate(old_id, 4));
    manual.origin = ValueOrigin::Manual;
    ed.record_imported(b, a, manual).unwrap();
    let solved = position(99, 3);
    for a in actions(solved) {
        ed.record_candidate(
            solved,
            a,
            Evaluation::import(ImportedValue {
                score: -brute(solved.play(a).unwrap()),
                quality: AnalysisQuality {
                    depth: solved.empty_count(),
                    selectivity: Selectivity::Full,
                    solved: true,
                },
                bound: legacy::Bound::Exact,
                identity: old_id,
                origin: ValueOrigin::Search,
            })
            .unwrap(),
        )
        .unwrap();
    }
    ed.configure_generation(&[b], config(2, 1)).unwrap();
    assert!(ed.generate(&mut eng, budget(), StopHandle::new()).is_err());
    ed.retarget(&eng).unwrap();
    let temp = Temp::new();
    let path = temp.file("retarget.work");
    ed.save(&path).unwrap();
    let ed = BookEditor::load(&path, ReadLimits::default()).unwrap();
    let book = ed.export().unwrap();
    let candidate = book
        .probe(b)
        .unwrap()
        .candidates()
        .find(|c| c.action == a)
        .unwrap();
    assert!(candidate.registered);
    assert_eq!(
        ed.move_values(b, a).unwrap().manual,
        Some(Evaluation::import(manual).unwrap())
    );
    assert!(
        candidate.value.is_none(),
        "old estimates and manual overrides must be inactive"
    );
    assert!(
        book.probe(solved)
            .unwrap()
            .candidates()
            .all(|c| c.value.unwrap().is_solved())
    );
}

#[test]
fn level_selective_endgame_results_do_not_become_proven_through_book_backup() {
    let mut eng = engine();
    // Forty occupied cells, only two opposing discs at the open frontier.
    // The game finishes after a few placements/passes despite 24 empty cells,
    // so a selective-endgame request is exercised without a large search.
    let occupied = u64::MAX << 24;
    let opponent = (1u64 << 27) | (1u64 << 28);
    let b = Board::from_bits(occupied ^ opponent, opponent).unwrap();
    assert_eq!(b.empty_count(), 24);
    assert!(
        actions(b)
            .iter()
            .any(|a| !b.play(*a).unwrap().is_terminal())
    );
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.configure_generation(
        &[b],
        GenerationConfig {
            analysis: AnalysisSchedule::Levels(vec![12]),
            max_plies: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        ed.generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .reason,
        GenerationStop::Exhausted
    );
    let book = ed.export().unwrap();
    for c in book.probe(b).unwrap().candidates() {
        let child = b.play(c.action).unwrap();
        let v = c.value.unwrap();
        assert_eq!(v.score(), -brute(child));
        if !child.is_terminal() {
            assert_eq!(v.depth(), 24);
            assert_eq!(v.selectivity(), Selectivity::Percent98);
            assert!(!v.is_solved());
        }
    }
    assert!(!book.probe(b).unwrap().value().unwrap().is_solved());
}

#[test]
fn terminal_children_need_no_mpc_calibration_even_when_the_level_would_be_selective() {
    let mut eng = Engine::new(
        Model::from_pattern(PatternModel::zero()),
        EngineOptions::default().tt_mib(1),
    )
    .unwrap();
    let occupied = u64::MAX << 24;
    let opponent = 1u64 << 27;
    let b = Board::from_bits(occupied ^ opponent, opponent).unwrap();
    assert!(!actions(b).is_empty());
    assert!(actions(b).iter().all(|a| b.play(*a).unwrap().is_terminal()));
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    ed.configure_generation(
        &[b],
        GenerationConfig {
            analysis: AnalysisSchedule::Levels(vec![12]),
            max_plies: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let report = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert_eq!(report.reason, GenerationStop::Exhausted);
    assert_eq!(report.nodes, 0);
    assert!(
        ImportedValue::from(ed.export().unwrap().probe(b).unwrap().value().unwrap())
            .quality
            .solved
    );
}

#[test]
fn book_backup_policy_keeps_search_evidence_and_never_downgrades_proofs() {
    let eng = engine();
    let b = position(94, 30);
    let a = actions(b)[0];
    let child = b.play(a).unwrap();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let direct = estimate(ed.identity(), 12);
    ed.record_candidate(b, a, direct).unwrap();
    for reply in actions(child) {
        let mut v = ImportedValue::from(estimate(ed.identity(), 4));
        v.quality.depth = 2;
        ed.record_imported(child, reply, v).unwrap();
    }
    ed.register_move(b, a).unwrap();
    let q = ed.move_values(b, a).unwrap();
    assert_eq!(q.direct, Some(direct));
    assert_eq!(q.child.unwrap().score(), -4);
    assert_eq!(q.selected_from, Some(SelectedFrom::Child));
    let backed_up = ed.move_values(b, a).unwrap();
    assert_eq!(backed_up.selected_from, Some(SelectedFrom::Child));
    assert_eq!(backed_up.selected.unwrap().depth(), 3);
    assert!(!backed_up.selected.unwrap().is_solved());
    assert_eq!(backed_up.direct, Some(direct));
    let temp = Temp::new();
    let path = temp.file("backup.work");
    ed.save(&path).unwrap();
    let mut loaded = BookEditor::load(&path, ReadLimits::default()).unwrap();
    assert_eq!(loaded.move_values(b, a), Some(backed_up));
    loaded.repair().unwrap();
    loaded.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    loaded.repair().unwrap();
    loaded.save(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(loaded.move_values(b, a).unwrap().selected, backed_up.child);
    let mut final_score = ImportedValue::from(direct);
    final_score.score = 2;
    final_score.quality = AnalysisQuality {
        depth: b.empty_count(),
        selectivity: Selectivity::Full,
        solved: true,
    };
    loaded.record_imported(b, a, final_score).unwrap();
    assert_eq!(
        loaded.move_values(b, a).unwrap().selected,
        Some(Evaluation::import(final_score).unwrap())
    );
    let mut manual = ImportedValue::from(estimate(loaded.identity(), 7));
    manual.origin = ValueOrigin::Manual;
    loaded.record_imported(b, a, manual).unwrap();
    let e = loaded.move_values(b, a).unwrap();
    assert_eq!(e.selected_from, Some(SelectedFrom::Manual));
    assert_eq!(e.direct, Some(Evaluation::import(final_score).unwrap()));
    assert_eq!(e.selected, Some(Evaluation::import(manual).unwrap()));
    // The public evidence API uses input coordinates under every symmetry.
    for t in Symmetry::all() {
        assert_eq!(loaded.move_values(t.board(b), t.action(a)), Some(e));
    }
    assert!(loaded.move_values(b, Action::Pass).is_none());
}

#[test]
fn workspace_statistics_count_frontier_quality_lines_and_transpositions() {
    let mut ed = BookEditor::new([0; 32]);
    let b = Board::new();
    let aa = actions(b);
    for &a in &aa {
        ed.register_move(b, a).unwrap();
    }
    // Initial moves share one canonical child. Every incoming path counts.
    assert_eq!(ed.len(), 2);
    let child = b.play(aa[0]).unwrap();
    for reply in actions(child) {
        ed.record_candidate(child, reply, estimate([0; 32], 3))
            .unwrap();
    }
    let stats = ed.stats();
    assert_eq!(stats.root_lines, 4);
    assert!(!stats.lines_saturated);
    assert_eq!(stats.leaf_positions, 1);
    assert_eq!(stats.registered_moves, 4);
    assert_eq!(stats.child_values_used, 4);
    assert_eq!(stats.complete_positions, 2);
    assert_eq!(stats.unregistered_candidates, actions(child).len() as u64);
    assert_eq!(stats.unknown_unregistered_candidates, 0);
    assert_eq!(stats.unknown_candidates, 0);
    assert_eq!(stats.qualities.iter().map(|b| b.candidates).sum::<u64>(), 7);
    assert_eq!(
        stats
            .qualities
            .iter()
            .map(|b| b.unregistered_candidates)
            .sum::<u64>(),
        3
    );
    let temp = Temp::new();
    let path = temp.file("stats.work");
    ed.save(&path).unwrap();
    assert_eq!(
        BookEditor::load(&path, ReadLimits::default())
            .unwrap()
            .stats(),
        stats
    );
}

#[test]
fn frontier_recalculation_is_resumable_and_does_not_analyze_or_expand_registered_links() {
    let mut eng = engine();
    let b = position(94, 30);
    let a = actions(b)[0];
    let child = b.play(a).unwrap();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let mut direct = ImportedValue::from(estimate(ed.identity(), -12));
    direct.quality = AnalysisQuality {
        depth: 4,
        selectivity: Selectivity::Full,
        solved: false,
    };
    ed.record_imported(b, a, direct).unwrap();
    ed.register_move(b, a).unwrap();
    let cfg = GenerationConfig {
        unregistered_only: true,
        reuse_completed: false,
        analysis: AnalysisSchedule::Levels(vec![1]),
        max_plies: 1,
        expansion_loss: 0,
        cumulative_loss: Some(0),
        leaf_loss: Some(0),
        ..Default::default()
    };
    ed.configure_generation(&[b], cfg.clone()).unwrap();
    assert!(
        ed.generate(
            &mut eng,
            GenerationLimits {
                new_positions: Some(1),
                ..Default::default()
            },
            StopHandle::new()
        )
        .is_err()
    );
    let interrupted = ed
        .generate(
            &mut eng,
            GenerationLimits {
                nodes: Some(1),
                ..Default::default()
            },
            StopHandle::new(),
        )
        .unwrap();
    assert_eq!(interrupted.reason, GenerationStop::NodeLimit);
    assert_eq!(
        ed.move_values(b, a).unwrap().direct,
        Some(Evaluation::import(direct).unwrap())
    );
    let temp = Temp::new();
    let path = temp.file("frontier.work");
    ed.save(&path).unwrap();
    let mut loaded = BookEditor::load(&path, ReadLimits::default()).unwrap();
    assert_eq!(loaded.generation_config(), Some(&cfg));
    let r = loaded
        .generate(&mut eng, budget(), StopHandle::new())
        .unwrap();
    assert_eq!(r.reason, GenerationStop::Exhausted);
    assert_eq!(r.new_positions, 0);
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded.stats().registered_moves, 1);
    assert_eq!(
        loaded.move_values(b, a).unwrap().direct,
        Some(Evaluation::import(direct).unwrap())
    );
    assert!(
        loaded
            .export()
            .unwrap()
            .probe(child)
            .unwrap()
            .value()
            .is_some(),
        "leaf pass must reach a registered child even when its parent was incomplete or outside current loss limits"
    );
    for c in loaded.export().unwrap().probe(child).unwrap().candidates() {
        assert_eq!(c.value.unwrap().depth(), 2);
        assert!(!c.registered);
    }
    assert_eq!(
        loaded
            .generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .nodes,
        0
    );
}

#[test]
fn child_updates_reschedule_registered_branches_without_a_policy_switch() {
    let mut eng = engine();
    let b = position(94, 30);
    let aa = actions(b);
    let child = b.play(aa[0]).unwrap();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    for (i, &a) in aa.iter().enumerate() {
        let mut v = ImportedValue::from(estimate(ed.identity(), if i == 0 { -10 } else { 0 }));
        v.quality.selectivity = Selectivity::Full;
        ed.record_imported(b, a, v).unwrap();
        ed.register_move(b, a).unwrap();
    }
    for a in actions(child) {
        let mut v = ImportedValue::from(estimate(ed.identity(), -20));
        v.quality = AnalysisQuality {
            depth: 2,
            selectivity: Selectivity::Full,
            solved: false,
        };
        ed.record_imported(child, a, v).unwrap();
    }
    let mut cfg = config(2, 1);
    cfg.expansion_loss = 0;
    ed.configure_generation(&[b], cfg).unwrap();
    assert_eq!(
        ed.move_values(b, aa[0]).unwrap().selected.unwrap().score(),
        20
    );
    let report = ed.generate(&mut eng, budget(), StopHandle::new()).unwrap();
    assert!(
        report.completed_stages >= 1,
        "the inactive child must now publish its retained analysis stage"
    );
    assert_eq!(
        report.nodes, 0,
        "all compatible direct values can be reused"
    );
}

#[test]
fn planning_without_calibration_can_reuse_or_strengthen_existing_full_width_evidence() {
    let mut eng = Engine::new(
        Model::from_pattern(PatternModel::zero()),
        EngineOptions::default().tt_mib(1),
    )
    .unwrap();
    let b = Board::new();
    let selective = SearchRequest::new(Goal::Depth(2)).selectivity(Selectivity::Percent95);
    assert_eq!(
        eng.search_plan(b, &selective).unwrap(),
        ResultKind::Depth {
            depth: 2,
            selectivity: Selectivity::Percent95
        }
    );
    assert!(matches!(
        eng.search(b, &selective),
        Err(EngineError::InvalidCalibration(_))
    ));
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    for a in actions(b) {
        let mut v = ImportedValue::from(estimate(ed.identity(), 0));
        v.quality = AnalysisQuality {
            depth: 2,
            selectivity: Selectivity::Full,
            solved: false,
        };
        ed.record_imported(b, a, v).unwrap();
    }
    let mut cfg = config(2, 0);
    cfg.analysis.stages_mut().unwrap()[0].selectivity = Selectivity::Percent95;
    ed.configure_generation(&[b], cfg.clone()).unwrap();
    assert_eq!(
        ed.generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .nodes,
        0
    );
    cfg.analysis.stages_mut().unwrap()[0].depth = 3;
    ed.configure_generation(&[b], cfg).unwrap();
    assert_eq!(
        ed.generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .reason,
        GenerationStop::Exhausted
    );
    assert!(
        ed.export()
            .unwrap()
            .probe(b)
            .unwrap()
            .candidates()
            .all(|c| c.value.unwrap().selectivity() == Selectivity::Full)
    );
}

#[test]
fn frontier_scheduling_finishes_the_same_small_solved_graph_after_budget_resume() {
    let b = position(123, 6);
    let mut eng = engine();
    let mut expected = BookEditor::for_engine(&eng).unwrap();
    expected.configure_generation(&[b], config(6, 2)).unwrap();
    assert_eq!(
        expected
            .generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .reason,
        GenerationStop::Exhausted
    );
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let mut cfg = config(6, 2);
    cfg.schedule = SchedulePolicy::Promising;
    ed.configure_generation(&[b], cfg).unwrap();
    assert_eq!(
        ed.generate(
            &mut eng,
            GenerationLimits {
                nodes: Some(1),
                ..Default::default()
            },
            StopHandle::new()
        )
        .unwrap()
        .reason,
        GenerationStop::NodeLimit
    );
    let temp = Temp::new();
    let path = temp.file("priority.work");
    ed.save(&path).unwrap();
    let mut ed = BookEditor::load(&path, ReadLimits::default()).unwrap();
    assert_eq!(
        ed.generate(&mut eng, budget(), StopHandle::new())
            .unwrap()
            .reason,
        GenerationStop::Exhausted
    );
    let expected = expected.export().unwrap();
    let actual = ed.export().unwrap();
    assert_eq!(expected.stats().positions, actual.stats().positions);
    let mut todo = vec![b];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(board) = todo.pop() {
        let canonical = board.canonical();
        if !seen.insert((canonical.player_bits(), canonical.opponent_bits())) {
            continue;
        }
        let e = expected.probe(board).unwrap();
        let a = actual.probe(board).unwrap();
        assert_eq!(
            e.value().map(ImportedValue::from),
            a.value().map(ImportedValue::from)
        );
        let entries = |e: BookEntry<'_>| {
            e.candidates()
                .map(|c| (c.action, c.registered, c.value.map(ImportedValue::from)))
                .collect::<Vec<_>>()
        };
        assert_eq!(entries(e), entries(a));
        for candidate in actual.probe(board).unwrap().candidates() {
            let value = candidate.value.unwrap();
            let exact = -brute(board.play(candidate.action).unwrap());
            match value.bound() {
                Bound::Point => assert_eq!(value.score(), exact),
                Bound::Upper => assert!(value.score() >= exact),
                _ => panic!("full endgame must produce exact points or certified upper bounds"),
            }
            if candidate.registered {
                todo.push(board.play(candidate.action).unwrap());
            }
        }
    }
}
