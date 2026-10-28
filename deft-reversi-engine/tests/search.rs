use deft_reversi_engine::*;
use std::time::Duration;
fn engine(threads: usize) -> Engine {
    Engine::new(PatternModel::zero(), EngineOptions { threads, tt_mib: 1 }).unwrap()
}
fn actions(board: Board) -> Vec<Action> {
    let mut moves = board.get_moves();
    let mut actions = Vec::new();
    while moves != 0 {
        let i = moves.trailing_zeros() as u8;
        moves &= moves - 1;
        actions.push(Action::Place(Square::new(i).unwrap()));
    }
    if actions.is_empty() && !board.is_terminal() {
        actions.push(Action::Pass)
    }
    actions
}
fn reference(board: Board) -> i32 {
    if board.is_terminal() {
        return board.final_score();
    }
    actions(board)
        .into_iter()
        .map(|a| -reference(board.play(a).unwrap()))
        .max()
        .unwrap()
}
fn position(mut random: u64, empties: u8) -> Board {
    let mut board = Board::new();
    while board.empty_count() > empties && !board.is_terminal() {
        let actions = actions(board);
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        board = board
            .play(actions[random as usize % actions.len()])
            .unwrap();
    }
    board
}
#[test]
fn exact_matches_exhaustive_and_pv_is_legal() {
    let mut engine = engine(1);
    for seed in 1..=24 {
        let board = position(seed, 6);
        let expected = reference(board);
        let result = engine
            .search(SearchRequest::new(board, SearchMode::Exact))
            .unwrap();
        assert_eq!(result.stop_reason, StopReason::Completed);
        assert_eq!(result.mode, SearchMode::Exact);
        assert_eq!(result.score, Some(expected));
        let mut end = board;
        for action in result.pv {
            end = end.play(action).unwrap();
        }
        assert!(end.is_terminal());
        let mut request = SearchRequest::new(board, SearchMode::Exact);
        request.node_limit = Some(100_000);
        assert_eq!(engine.search(request).unwrap().score, Some(expected));
    }
}
#[test]
fn interrupted_search_never_claims_an_exact_solution() {
    for kind in 0..3 {
        let mut engine = engine(2);
        let mut request = SearchRequest::new(Board::new(), SearchMode::Exact);
        match kind {
            0 => request.node_limit = Some(0),
            1 => request.time_limit = Some(Duration::ZERO),
            _ => request.stop.stop(),
        }
        let result = engine.search(request).unwrap();
        assert!(matches!(result.action, Some(Action::Place(_))));
        assert_eq!(result.score, None);
        assert_eq!(result.completed_depth, 0);
        assert_eq!(result.mode, SearchMode::Depth(0));
        assert_ne!(result.stop_reason, StopReason::Completed);
        let recovered = engine
            .search(SearchRequest::new(Board::new(), SearchMode::Depth(2)))
            .unwrap();
        assert_eq!(recovered.stop_reason, StopReason::Completed);
    }
}
#[test]
fn budget_is_shared_across_workers_and_iterations() {
    let mut engine = engine(4);
    let mut request = SearchRequest::new(Board::new(), SearchMode::Depth(12));
    request.node_limit = Some(1500);
    let result = engine.search(request).unwrap();
    assert!(result.stats.nodes <= 1500);
    assert_eq!(result.stop_reason, StopReason::NodeLimit);
    if result.score.is_some() {
        assert!(result.completed_depth > 0);
        assert_eq!(result.mode, SearchMode::Depth(result.completed_depth));
    }
}
#[test]
fn pass_and_terminal_are_distinct() {
    let board = Board::from_bits(1, 2).unwrap().board_after_pass();
    assert_eq!(board.get_moves(), 0);
    assert_ne!(board.get_opponent_moves(), 0);
    let result = engine(1)
        .search(SearchRequest::new(board, SearchMode::Depth(2)))
        .unwrap();
    assert_eq!(result.action, Some(Action::Pass));
    assert_eq!(result.pv.first(), Some(&Action::Pass));
    let terminal = Board::from_bits(1, 0).unwrap();
    let result = engine(1)
        .search(SearchRequest::new(terminal, SearchMode::Exact))
        .unwrap();
    assert_eq!(result.action, None);
    assert_eq!(result.score, Some(64));
}
#[test]
fn configuration_is_validated() {
    assert!(
        Engine::new(
            PatternModel::zero(),
            EngineOptions {
                threads: 0,
                tt_mib: 1
            }
        )
        .is_err()
    );
    assert!(Square::new(64).is_none());
    assert!("A9".parse::<Square>().is_err());
    assert!("あ".parse::<Square>().is_err());
    let mut engine = engine(1);
    assert!(
        engine
            .search(SearchRequest::new(Board::new(), SearchMode::Depth(61)))
            .is_err()
    );
    let mut request = SearchRequest::new(Board::new(), SearchMode::Depth(3));
    request.probcut_sigma = Some(f64::NAN);
    assert!(engine.search(request).is_err());
}
#[test]
fn parallel_matches_serial_at_fixed_depth() {
    let board = Board::new();
    let serial = engine(1)
        .search(SearchRequest::new(board, SearchMode::Depth(8)))
        .unwrap();
    let parallel = engine(4)
        .search(SearchRequest::new(board, SearchMode::Depth(8)))
        .unwrap();
    assert_eq!(serial.score, parallel.score);
    assert_eq!(parallel.stop_reason, StopReason::Completed);
    // Assignment can fail if a helper is still releasing its previous mailbox;
    // on a fresh pool this position offers enough work for at least one split.
    assert!(parallel.stats.splits > 0);
    assert!(parallel.stats.helper_jobs > 0);
}
#[test]
#[ignore = "stress suite: run after the resource hold ends"]
fn ybwc_exact_repetition_and_thread_counts() {
    for threads in [1, 2, 4, 8, 20] {
        let mut engine = engine(threads);
        let mut splits = 0;
        let mut helper_jobs = 0;
        for seed in 1..=5 {
            let board = position(seed, 10);
            let expected = reference(board);
            for _ in 0..20 {
                let result = engine
                    .search(SearchRequest::new(board, SearchMode::Exact))
                    .unwrap();
                assert_eq!(result.score, Some(expected));
                assert_eq!(result.stop_reason, StopReason::Completed);
                splits += result.stats.splits;
                helper_jobs += result.stats.helper_jobs;
            }
        }
        // TT reuse may finish a search without splitting. An offer can also
        // miss a worker still releasing its mailbox from the preceding stage.
        // Require real parallel execution across the repeated workload, while
        // checking the score and completion of every individual search above.
        if threads > 1 {
            assert!(splits > 0, "no split with {threads} threads");
            assert!(helper_jobs > 0, "no helper executed with {threads} threads");
        }
    }
}

#[test]
fn selective_cache_does_not_leak_into_full_search() {
    let board = Board::new();
    let mut engine = engine(2);
    let mut rules = Vec::new();
    for depth in 2..=8 {
        for empties in depth..=60 {
            rules.push(ProbCutRule {
                exact: false,
                depth,
                empties,
                shallow_depth: 1,
                slope: 1.0,
                intercept: 0.0,
                deviation: 0.0,
            });
        }
    }
    engine.set_probcut_rules(rules).unwrap();
    let mut request = SearchRequest::new(board, SearchMode::Depth(5));
    request.probcut_sigma = Some(2.0);
    let result = engine.search(request).unwrap();
    assert!(result.selective);
    assert!(result.stats.probcut_cuts > 0);
    let result = engine
        .search(SearchRequest::new(board, SearchMode::Depth(5)))
        .unwrap();
    assert!(!result.selective);
    assert_eq!(result.score, Some(0));
    assert_eq!(result.stats.probcut_tries, 0);
    engine.set_model(PatternModel::zero()).unwrap();
    let mut request = SearchRequest::new(board, SearchMode::Depth(4));
    request.probcut_sigma = Some(f64::NAN);
    assert!(engine.search(request).is_err());
}

#[test]
fn external_stop_joins_workers_and_engine_can_be_reused() {
    let mut engine = engine(4);
    for _ in 0..3 {
        let request = SearchRequest::new(Board::new(), SearchMode::Depth(60));
        let stop = request.stop.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            stop.stop();
        });
        let result = engine.search(request).unwrap();
        stopper.join().unwrap();
        assert_eq!(result.stop_reason, StopReason::Requested);
        assert!(result.stats.nodes > 0);
        assert!(Board::new().play(result.action.unwrap()).is_ok());
        let result = engine
            .search(SearchRequest::new(Board::new(), SearchMode::Depth(2)))
            .unwrap();
        assert_eq!(result.stop_reason, StopReason::Completed);
    }
}

#[test]
fn exact_endgame_cache_is_consistent_when_warm_cleared_or_model_changes() {
    let mut engine = engine(1);
    for seed in [3, 7, 11] {
        let board = position(seed, 9);
        let expected = reference(board);
        for run in 0..4 {
            if run == 2 {
                engine.clear_tt().unwrap();
            }
            if run == 3 {
                engine.set_model(PatternModel::zero()).unwrap();
            }
            let result = engine
                .search(SearchRequest::new(board, SearchMode::Exact))
                .unwrap();
            assert_eq!(result.score, Some(expected));
            assert_eq!(result.stop_reason, StopReason::Completed);
            let mut terminal = board;
            for action in result.pv {
                terminal = terminal.play(action).unwrap();
            }
            assert!(terminal.is_terminal());
        }
    }
}

#[test]
fn selective_preparation_cannot_supply_the_exact_final_score() {
    // Deliberately unreliable calibration makes preparation bounds unsuitable
    // for the final pass. They may order moves but must never cut that pass.
    for threads in [1, 4] {
        let mut engine = engine(threads);
        engine
            .set_probcut_rules(
                (12..=13)
                    .map(|empties| ProbCutRule {
                        exact: true,
                        depth: empties,
                        empties,
                        shallow_depth: 0,
                        slope: 1.0,
                        intercept: 0.0,
                        deviation: 0.0,
                    })
                    .collect(),
            )
            .unwrap();
        for seed in [3, 7, 11] {
            let board = position(seed, 13);
            let expected = reference(board);
            for run in 0..2 {
                let result = engine
                    .search(SearchRequest::new(board, SearchMode::Exact))
                    .unwrap();
                assert_eq!(result.stop_reason, StopReason::Completed);
                assert_eq!(result.mode, SearchMode::Exact);
                assert!(!result.selective);
                assert_eq!(result.score, Some(expected));
                if run == 0 {
                    assert!(result.stats.probcut_tries > 0);
                }
                let mut end = board;
                for action in result.pv {
                    end = end.play(action).unwrap();
                }
                assert!(end.is_terminal());
            }
        }
    }
}

#[test]
fn stopping_after_selective_preparation_does_not_report_exact() {
    // Exact preparation stops endgame ProbCut below thirteen empties; a rule at
    // thirteen makes it search the same tree as the selective request.
    let board = position(3, 14);
    let mut engine = engine(1);
    engine
        .set_probcut_rules(vec![ProbCutRule {
            exact: true,
            depth: 13,
            empties: 13,
            shallow_depth: 0,
            slope: 1.0,
            intercept: 0.0,
            deviation: 0.0,
        }])
        .unwrap();
    let mut preparation = SearchRequest::new(board, SearchMode::SelectiveEndgame);
    preparation.probcut_sigma = Some(1.43953);
    let prepared = engine.search(preparation).unwrap();
    assert_eq!(prepared.stop_reason, StopReason::Completed);
    engine.clear_tt().unwrap();
    let mut exact = SearchRequest::new(board, SearchMode::Exact);
    exact.node_limit = Some(prepared.stats.nodes);
    let stopped = engine.search(exact).unwrap();
    assert_eq!(stopped.stop_reason, StopReason::NodeLimit);
    assert_eq!(stopped.mode, SearchMode::SelectiveEndgame);
    assert!(stopped.selective);
    assert_eq!(stopped.selectivity, Some(Selectivity::Percent85));
    assert_eq!(stopped.probcut_sigma, Some(1.43953));
    assert_eq!(stopped.completed_depth, board.empty_count());
    assert_eq!(stopped.score, prepared.score);
}

#[test]
fn level_midgame_iterations_keep_the_requested_selectivity() {
    let mut engine = engine(1);
    let mut request = SearchRequest::for_level(Board::new(), 16);
    assert_eq!(request.selectivity, Selectivity::Percent95);
    request.node_limit = Some(2000);
    let result = engine.search(request).unwrap();
    assert_eq!(result.stop_reason, StopReason::NodeLimit);
    assert!(result.completed_depth > 0 && result.completed_depth < 12);
    assert_eq!(result.selectivity, Some(Selectivity::Percent95));
    assert_eq!(result.probcut_sigma, Selectivity::Percent95.sigma());
}
