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
fn engine() -> Engine {
    Engine::new(
        PatternModel::zero(),
        EngineOptions {
            threads: 1,
            tt_mib: 1,
        },
    )
    .unwrap()
}
fn actions(b: Board) -> Vec<Action> {
    let mut bits = b.get_moves();
    let mut a = vec![];
    while bits != 0 {
        let bit = bits.isolate_lowest_one();
        bits ^= bit;
        a.push(Action::Place(
            Square::new(bit.trailing_zeros() as u8).unwrap(),
        ));
    }
    if a.is_empty() && b.get_opponent_moves() != 0 {
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
fn estimate(identity: AnalysisIdentity, score: i16) -> Assessment {
    Assessment {
        score,
        quality: AnalysisQuality {
            depth: 12,
            selectivity: Selectivity::Percent95,
            solved: false,
        },
        bound: Bound::Exact,
        identity,
        origin: ValueOrigin::Search,
    }
}
fn config(depth: u8, plies: u8) -> GenerationConfig {
    GenerationConfig {
        stages: vec![Stage {
            depth,
            selectivity: Selectivity::Full,
        }],
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
        e.record(b, a, estimate(id, i as i16)).unwrap();
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
    ed.record(b, a, estimate(ed.identity(), 4)).unwrap();
    let book = Arc::new(ed.export().unwrap());
    assert!(!book.probe(b).unwrap().all_moves_evaluated());
    let mut p = BookPlayer::new(eng, Some(book), UsePolicy::default(), 0).unwrap();
    let r = p
        .choose_move(SearchRequest::new(b, SearchMode::Depth(1)))
        .unwrap();
    assert!(matches!(r.source,PlaySource::Book(BookChoice{action,..}) if action==a));
    p.set_policy(UsePolicy {
        require_all_evaluated: true,
        ..UsePolicy::default()
    })
    .unwrap();
    assert!(matches!(
        p.choose_move(SearchRequest::new(b, SearchMode::Depth(1)))
            .unwrap()
            .source,
        PlaySource::Search(_)
    ));
}
#[test]
fn unknown_frontier_bounds_and_model_mismatch_are_not_played() {
    let eng = engine();
    let b = position(31, 42);
    let a = actions(b)[0];
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let id = ed.identity();
    ed.record(b, a, estimate(id, 3)).unwrap();
    let mut p = BookPlayer::new(
        eng,
        Some(Arc::new(ed.export().unwrap())),
        UsePolicy::default(),
        1,
    )
    .unwrap();
    assert!(matches!(
        p.choose_move(SearchRequest::new(b, SearchMode::Depth(1)))
            .unwrap()
            .source,
        PlaySource::Search(_)
    ));
    ed.register_move(b, a).unwrap();
    let mut upper = estimate(id, 5);
    upper.bound = Bound::Upper;
    let mut bounded = BookEditor::new(id);
    bounded.register_move(b, a).unwrap();
    bounded.record(b, a, upper).unwrap();
    p.set_book(Some(Arc::new(bounded.export().unwrap())));
    assert!(matches!(
        p.choose_move(SearchRequest::new(b, SearchMode::Depth(1)))
            .unwrap()
            .source,
        PlaySource::Search(_)
    ));
    let mut wrong = BookEditor::new([9; 32]);
    wrong.register_move(b, a).unwrap();
    wrong.record(b, a, estimate([9; 32], 64)).unwrap();
    p.set_book(Some(Arc::new(wrong.export().unwrap())));
    assert!(matches!(
        p.choose_move(SearchRequest::new(b, SearchMode::Depth(1)))
            .unwrap()
            .source,
        PlaySource::Search(_)
    ));
}
#[test]
fn stopping_and_terminal_requests_never_return_a_book_move() {
    let eng = engine();
    let mut ed = BookEditor::for_engine(&eng).unwrap();
    let b = Board::new();
    for a in actions(b) {
        ed.register_move(b, a).unwrap();
        ed.record(b, a, estimate(ed.identity(), 0)).unwrap();
    }
    let mut p = BookPlayer::new(
        eng,
        Some(Arc::new(ed.export().unwrap())),
        UsePolicy::default(),
        1,
    )
    .unwrap();
    let mut req = SearchRequest::new(b, SearchMode::Depth(1));
    req.stop.stop();
    assert!(matches!(
        p.choose_move(req.clone()).unwrap().source,
        PlaySource::Search(_)
    ));
    req.stop = StopHandle::new();
    req.time_limit = Some(Duration::ZERO);
    assert!(matches!(
        p.choose_move(req).unwrap().source,
        PlaySource::Search(_)
    ));
    let terminal = Board::from_bits(u64::MAX, 0).unwrap();
    let r = p
        .choose_move(SearchRequest::new(terminal, SearchMode::Exact))
        .unwrap();
    assert!(matches!(
        r.source,
        PlaySource::Search(SearchResult { action: None, .. })
    ));
}
#[test]
fn random_selection_is_seeded_and_stays_within_loss() {
    let b = position(94, 36);
    let mut e = BookEditor::for_engine(&engine()).unwrap();
    let aa = actions(b);
    assert!(aa.len() >= 3);
    for (i, &a) in aa.iter().enumerate() {
        e.register_move(b, a).unwrap();
        e.record(b, a, estimate(e.identity(), 10 - i as i16))
            .unwrap();
    }
    let book = Arc::new(e.export().unwrap());
    let policy = UsePolicy {
        moves: MovePolicy::RandomWithinLoss { max_loss: 1 },
        ..UsePolicy::default()
    };
    let mut p = BookPlayer::new(engine(), Some(book.clone()), policy, 123).unwrap();
    let mut q = BookPlayer::new(engine(), Some(book), policy, 123).unwrap();
    let mut seen = vec![];
    for _ in 0..128 {
        let PlaySource::Book(a) = p
            .choose_move(SearchRequest::new(b, SearchMode::Depth(1)))
            .unwrap()
            .source
        else {
            panic!()
        };
        let PlaySource::Book(c) = q
            .choose_move(SearchRequest::new(b, SearchMode::Depth(1)))
            .unwrap()
            .source
        else {
            panic!()
        };
        assert_eq!(a.action, c.action);
        assert!(a.value.score >= 9);
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
    assert_eq!(view.value().unwrap().score, brute(b));
    assert!(view.value().unwrap().quality.solved);
    for c in view.candidates() {
        assert_eq!(c.value.unwrap().score, -brute(b.play(c.action).unwrap()));
    }
    // Fixed-depth Bound::Exact was not enough; these candidates are terminal proofs.
    let mut other = engine();
    other.set_probcut_rules(vec![]).unwrap();
    let mut p = BookPlayer::new(other, Some(Arc::new(book)), UsePolicy::default(), 1).unwrap();
    let PlaySource::Book(choice) = p
        .choose_move(SearchRequest::new(b, SearchMode::Depth(1)))
        .unwrap()
        .source
    else {
        panic!()
    };
    assert_eq!(choice.value.score, brute(b));
}
#[test]
fn forced_pass_and_terminal_round_trip() {
    let mut pass = None;
    for s in 1..200 {
        let b = position(s, 4);
        if b.get_moves() == 0 && !b.is_terminal() {
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
    assert_eq!(v.value().unwrap().score, brute(b));
    // A pass changes absolute side without consuming a placement: this child
    // must use White's zero-loss width rather than Black's unrestricted width.
    let child = book.probe(b.board_after_pass()).unwrap();
    let best = child
        .candidates()
        .map(|c| c.value.unwrap().score)
        .max()
        .unwrap();
    assert!(
        child
            .candidates()
            .all(|c| c.registered == (c.value.unwrap().score == best))
    );
    let temp = Temp::new();
    let p = temp.file("pass.dbook");
    book.save(&p).unwrap();
    let loaded = Book::load(p, ReadLimits::default()).unwrap();
    assert_eq!(loaded.probe(b).unwrap().value().unwrap().score, brute(b));
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
    let mut manual = estimate(e.identity(), 42);
    manual.origin = ValueOrigin::Manual;
    e.record(b, a, manual).unwrap();
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
        Some(manual)
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
        Some(manual)
    );
}
#[test]
fn stage_upgrade_and_explicit_reanalysis_keep_completed_quality() {
    let mut eng = engine();
    let b = position(222, 9);
    let mut e = BookEditor::for_engine(&eng).unwrap();
    let mut c = config(2, 1);
    c.stages.push(Stage {
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
            .all(|c| c.value.unwrap().quality.depth >= 4)
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
            .all(|c| c.value.unwrap().quality.depth >= 4)
    );
}
#[test]
fn editor_and_model_management_do_not_change_search_results_or_node_counts() {
    let mut a = engine();
    let mut b = engine();
    let board = position(567, 9);
    let r1 = a
        .search(SearchRequest::new(board, SearchMode::Exact))
        .unwrap();
    let _ = BookEditor::for_engine(&b).unwrap();
    let r2 = b
        .search(SearchRequest::new(board, SearchMode::Exact))
        .unwrap();
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
    e.record(board, a, estimate(e.identity(), 4)).unwrap();
    let book = Arc::new(e.export().unwrap());
    let mut player = BookPlayer::new(first, Some(book), UsePolicy::default(), 1).unwrap();
    let mut bytes = Vec::new();
    PatternModel::zero().write_to(&mut bytes).unwrap();
    let n = bytes.len();
    bytes[n - 2..].copy_from_slice(&128i16.to_le_bytes());
    let second = Engine::from_bytes(
        &bytes,
        EngineOptions {
            threads: 1,
            tt_mib: 1,
        },
    )
    .unwrap();
    let _old = player.replace_engine(second).unwrap();
    assert!(matches!(
        player
            .choose_move(SearchRequest::new(board, SearchMode::Depth(1)))
            .unwrap()
            .source,
        PlaySource::Search(_)
    ));
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
        let mut v = estimate(e.identity(), if i == 1 { 10 } else { -10 });
        v.quality.selectivity = Selectivity::Full;
        e.record(b, a, v).unwrap();
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
    let mut v = estimate(e.identity(), 20);
    v.quality.selectivity = Selectivity::Full;
    e.record(b, aa[0], v).unwrap();
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
            let mut v = estimate(id, -(i as i16));
            v.quality.selectivity = Selectivity::Full;
            e.record(b, a, v).unwrap();
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
    let mut old = estimate(ed.identity(), 0);
    old.quality.depth = 2;
    old.quality.selectivity = Selectivity::Full;
    for a in actions(b) {
        ed.record(b, a, old).unwrap();
    }
    let mut cfg = config(4, 0);
    cfg.stages[0].selectivity = Selectivity::Percent95;
    ed.configure_generation(&[b], cfg).unwrap();
    let report = ed
        .generate(&mut engine, budget(), StopHandle::new())
        .unwrap();
    assert_eq!(report.reason, GenerationStop::Exhausted);
    let book = ed.export().unwrap();
    assert!(book.probe(b).unwrap().candidates().all(|c| {
        let q = c.value.unwrap().quality;
        q.depth >= 4 && q.selectivity == Selectivity::Full
    }));
    // The converse change (shallower but less selective) must also preserve
    // the existing deeper analysis and complete the newly requested quality.
    let mut ed = BookEditor::for_engine(&engine).unwrap();
    old.quality.depth = 4;
    old.quality.selectivity = Selectivity::Percent95;
    for a in actions(b) {
        ed.record(b, a, old).unwrap();
    }
    ed.configure_generation(&[b], config(2, 0)).unwrap();
    ed.generate(&mut engine, budget(), StopHandle::new())
        .unwrap();
    let book = ed.export().unwrap();
    assert!(book.probe(b).unwrap().candidates().all(|c| {
        let q = c.value.unwrap().quality;
        q.depth >= 4 && q.selectivity == Selectivity::Full
    }));
}
