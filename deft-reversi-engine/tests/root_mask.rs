use deft_reversi_engine::*;
fn engine(threads: usize) -> Engine {
    Engine::new(
        Model::from_pattern(PatternModel::zero()),
        EngineOptions::default().threads(threads).tt_mib(1),
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
fn brute(b: Board) -> i32 {
    if b.is_terminal() {
        b.final_score()
    } else {
        actions(b)
            .into_iter()
            .map(|a| -brute(b.play(a).unwrap()))
            .max()
            .unwrap()
    }
}
#[test]
fn restricted_endgames_match_independent_enumeration_without_poisoning_full_root() {
    let mut serial = engine(1);
    let mut parallel = engine(4);
    let mut checked = 0;
    for seed in 1..=20 {
        let b = position(seed, 6);
        let aa = actions(b);
        if aa.len() < 2 || b.must_pass() {
            continue;
        }
        checked += 1;
        let full = brute(b);
        let mut mask = 0;
        for a in aa.iter().step_by(2) {
            if let Action::Place(s) = a {
                mask |= 1u64 << s.index();
            }
        }
        let expected = aa
            .iter()
            .filter(|a| matches!(a,Action::Place(s) if mask&(1u64<<s.index())!=0))
            .map(|&a| -brute(b.play(a).unwrap()))
            .max()
            .unwrap();
        for e in [&mut serial, &mut parallel] {
            let r = e
                .search(
                    b,
                    &SearchRequest::new(Goal::Endgame)
                        .root_moves(mask)
                        .all_moves(),
                )
                .unwrap();
            assert_eq!(r.score, Some(expected));
            assert_eq!(r.root_moves, mask);
            assert!(r.root_restricted);
            assert!(r.is_subset_solved());
            assert!(!r.is_solved());
            assert!(matches!(r.best_move,Some(Action::Place(s)) if mask&(1u64<<s.index())!=0));
            assert!(
                r.moves
                    .iter()
                    .all(|m| matches!(m.action,Action::Place(s) if mask&(1u64<<s.index())!=0))
            );
            let mut pv = b;
            for a in r.pv {
                pv = pv.play(a).unwrap();
            }
            let r = e.search(b, &SearchRequest::new(Goal::Endgame)).unwrap();
            assert_eq!(r.score, Some(full));
            assert!(r.is_solved());
            assert!(!r.root_restricted);
        }
    }
    assert!(checked >= 10);
}
#[test]
fn forced_pass_and_invalid_masks_have_explicit_semantics() {
    let mut e = engine(1);
    let b = Board::new();
    assert!(
        e.search_plan(b, &SearchRequest::new(Goal::Depth(2)).root_moves(0))
            .is_err()
    );
    assert!(
        e.search(
            b,
            &SearchRequest::new(Goal::Depth(2)).root_moves(b.player_bits())
        )
        .is_err()
    );
    let pass = (1..500)
        .map(|s| position(s, 3))
        .find(|b| b.must_pass())
        .unwrap();
    let r = e
        .search(pass, &SearchRequest::new(Goal::Endgame).root_moves(0))
        .unwrap();
    assert_eq!(r.best_move, Some(Action::Pass));
    assert_eq!(r.score, Some(brute(pass)));
    assert_eq!(r.root_moves, 0);
    assert!(!r.root_restricted);
    assert!(r.is_solved());
}

#[test]
fn fork_and_resize_preserve_model_identity_and_independent_search_state() {
    let mut main = engine(4);
    let mut fork = main
        .fork(EngineOptions::default().threads(2).tt_mib(2))
        .unwrap();
    let b = position(123, 6);
    let exact = brute(b);
    main.set_resources(EngineOptions::default().threads(1).tt_mib(1))
        .unwrap();
    assert_eq!(main.options().threads, 1);
    assert_eq!(fork.options().threads, 2);
    assert_eq!(fork.options().tt_mib, 2);
    for e in [&mut main, &mut fork] {
        let r = e.search(b, &SearchRequest::new(Goal::Endgame)).unwrap();
        assert_eq!(r.score, Some(exact));
        assert!(r.is_solved());
    }
    main.set_resources(EngineOptions::default().threads(4).tt_mib(2))
        .unwrap();
    assert!(
        main.set_resources(EngineOptions::default().threads(0))
            .is_err()
    );
    assert_eq!(main.options().threads, 4);
    assert_eq!(main.options().tt_mib, 2);
    assert_eq!(
        main.search(b, &SearchRequest::new(Goal::Endgame))
            .unwrap()
            .score,
        Some(exact)
    );
}
