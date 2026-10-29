use deft_reversi_engine::*;
use std::time::Duration;
fn model() -> Model {
    Model::from_pattern(PatternModel::zero())
}
fn engine(threads: usize) -> Engine {
    Engine::new(model(), EngineOptions::default().threads(threads).tt_mib(1)).unwrap()
}
fn exact() -> SearchRequest {
    SearchRequest::new(Goal::Endgame)
}
fn depth(depth: u8) -> SearchRequest {
    SearchRequest::new(Goal::Depth(depth))
}
fn actions(board: Board) -> Vec<Action> {
    let mut actions: Vec<Action> = board.legal_moves().map(Action::Place).collect();
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
        let result = engine.search(board, &exact()).unwrap();
        assert_eq!(result.stop_reason, StopReason::Completed);
        assert!(result.is_solved());
        assert_eq!(result.score, Some(expected));
        let mut end = board;
        for action in result.pv {
            end = end.play(action).unwrap();
        }
        assert!(end.is_terminal());
        let request = exact().node_limit(100_000);
        assert_eq!(
            engine.search(board, &request).unwrap().score,
            Some(expected)
        );
    }
}
#[test]
fn interrupted_search_never_claims_an_exact_solution() {
    for kind in 0..3 {
        let mut engine = engine(2);
        let request = match kind {
            0 => exact().node_limit(0),
            1 => exact().time_limit(Duration::ZERO),
            _ => exact(),
        };
        if kind == 2 {
            request.stop_handle().stop();
        }
        let result = engine.search(Board::new(), &request).unwrap();
        assert!(matches!(result.best_move, Some(Action::Place(_))));
        assert_eq!(result.score, None);
        assert_eq!(result.kind, None);
        assert!(!result.is_solved());
        assert_ne!(result.stop_reason, StopReason::Completed);
        let recovered = engine.search(Board::new(), &depth(2)).unwrap();
        assert_eq!(recovered.stop_reason, StopReason::Completed);
    }
}
#[test]
fn budget_is_shared_across_workers_and_iterations() {
    let mut engine = engine(4);
    let result = engine
        .search(Board::new(), &depth(12).node_limit(1500))
        .unwrap();
    assert!(result.stats.nodes <= 1500);
    assert_eq!(result.stop_reason, StopReason::NodeLimit);
    if result.score.is_some() {
        assert!(matches!(result.kind, Some(ResultKind::Depth { depth, .. }) if depth > 0));
    }
}
#[test]
fn pass_and_terminal_are_distinct() {
    let board = Board::from_bits(2, 1).unwrap();
    assert!(board.legal_moves().is_empty());
    assert!(board.must_pass());
    let result = engine(1).search(board, &depth(2).all_moves()).unwrap();
    assert_eq!(result.best_move, Some(Action::Pass));
    assert_eq!(result.pv.first(), Some(&Action::Pass));
    assert_eq!(
        result.moves,
        [MoveScore {
            action: Action::Pass,
            score: result.score.unwrap()
        }]
    );
    let terminal = Board::from_bits(1, 0).unwrap();
    let result = engine(1).search(terminal, &exact()).unwrap();
    assert_eq!(result.best_move, None);
    assert_eq!(result.score, Some(64));
    assert!(result.is_solved());
}
#[test]
fn configuration_is_validated() {
    assert!(Engine::new(model(), EngineOptions::default().threads(0)).is_err());
    assert!(Square::new(64).is_none());
    assert!("A9".parse::<Square>().is_err());
    assert!("あ".parse::<Square>().is_err());
    assert_eq!("pass".parse::<Action>(), Ok(Action::Pass));
    let mut engine = engine(1);
    for request in [
        depth(61),
        SearchRequest::new(Goal::Level(0)),
        SearchRequest::new(Goal::Level(61)),
        SearchRequest::new(Goal::Level(10)).selectivity(Selectivity::Percent95),
    ] {
        assert!(matches!(
            engine.search(Board::new(), &request),
            Err(EngineError::InvalidRequest(_))
        ));
    }
}
#[test]
fn parallel_matches_serial_at_fixed_depth() {
    let board = Board::new();
    let serial = engine(1).search(board, &depth(8)).unwrap();
    let parallel = engine(4).search(board, &depth(8)).unwrap();
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
                let result = engine.search(board, &exact()).unwrap();
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
    let selective = depth(5).selectivity(Selectivity::Percent95);
    let result = engine.search(board, &selective).unwrap();
    assert_eq!(
        result.kind,
        Some(ResultKind::Depth {
            depth: 5,
            selectivity: Selectivity::Percent95
        })
    );
    assert!(result.stats.probcut_cuts > 0);
    let result = engine.search(board, &depth(5)).unwrap();
    assert_eq!(result.kind.unwrap().selectivity(), Selectivity::Full);
    assert_eq!(result.score, Some(0));
    assert_eq!(result.stats.probcut_tries, 0);
    engine.set_probcut_rules(Vec::new()).unwrap();
    assert!(matches!(
        engine.search(board, &selective),
        Err(EngineError::InvalidCalibration(_))
    ));
}

#[test]
fn external_stop_joins_workers_and_engine_can_be_reused() {
    let mut engine = engine(4);
    for _ in 0..3 {
        let request = depth(60);
        let stop = request.stop_handle();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            stop.stop();
        });
        let result = engine.search(Board::new(), &request).unwrap();
        stopper.join().unwrap();
        assert_eq!(result.stop_reason, StopReason::Requested);
        assert!(result.stats.nodes > 0);
        assert!(Board::new().play(result.best_move.unwrap()).is_ok());
        let result = engine.search(Board::new(), &depth(2)).unwrap();
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
                engine.set_model(model()).unwrap();
            }
            let result = engine.search(board, &exact()).unwrap();
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
                let result = engine.search(board, &exact()).unwrap();
                assert_eq!(result.stop_reason, StopReason::Completed);
                assert!(result.is_solved());
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
    let preparation = exact().selectivity(Selectivity::Percent85);
    let prepared = engine.search(board, &preparation).unwrap();
    assert_eq!(prepared.stop_reason, StopReason::Completed);
    engine.clear_tt().unwrap();
    let stopped = engine
        .search(board, &exact().node_limit(prepared.stats.nodes))
        .unwrap();
    assert_eq!(stopped.stop_reason, StopReason::NodeLimit);
    assert!(!stopped.is_solved());
    assert_eq!(
        stopped.kind,
        Some(ResultKind::Endgame {
            selectivity: Selectivity::Percent85
        })
    );
    assert_eq!(stopped.score, prepared.score);
}

#[test]
fn level_midgame_iterations_keep_the_requested_selectivity() {
    let mut engine = engine(1);
    let request = SearchRequest::new(Goal::Level(16)).node_limit(2000);
    let result = engine.search(Board::new(), &request).unwrap();
    assert_eq!(result.stop_reason, StopReason::NodeLimit);
    let Some(ResultKind::Depth { depth, selectivity }) = result.kind else {
        panic!("expected a depth result, got {:?}", result.kind);
    };
    assert!(depth > 0 && depth < 12);
    assert_eq!(selectivity, Selectivity::Percent95);
}

#[test]
fn all_moves_scores_every_move_exactly() {
    for threads in [1, 4] {
        let mut engine = engine(threads);
        for seed in [5, 9] {
            let board = position(seed, 8);
            let result = engine.search(board, &exact().all_moves()).unwrap();
            assert!(result.is_solved());
            let mut expected: Vec<MoveScore> = actions(board)
                .into_iter()
                .map(|action| MoveScore {
                    action,
                    score: -reference(board.play(action).unwrap()),
                })
                .collect();
            expected.sort_by_key(|entry| std::cmp::Reverse(entry.score));
            let scores = |moves: &[MoveScore]| moves.iter().map(|m| m.score).collect::<Vec<_>>();
            assert_eq!(scores(&result.moves), scores(&expected));
            for entry in &result.moves {
                assert!(expected.contains(entry), "{entry:?}");
            }
            assert_eq!(result.moves[0].score, result.score.unwrap());
        }
    }
    // A stopped search leaves the list empty rather than partial.
    let result = engine(1)
        .search(Board::new(), &depth(10).all_moves().node_limit(500))
        .unwrap();
    assert!(result.moves.is_empty());
}

#[test]
fn progress_reports_each_completed_iteration() {
    let mut engine = engine(2);
    let board = position(3, 12);
    let mut reports = Vec::new();
    let result = engine
        .search_with(board, &exact(), |progress| reports.push(progress.clone()))
        .unwrap();
    assert!(result.is_solved());
    let last = reports.last().unwrap();
    assert_eq!(Some(last.kind), result.kind);
    assert_eq!(Some(last.score), result.score);
    assert!(reports.windows(2).all(|w| w[0].nodes <= w[1].nodes));
    assert!(last.nodes <= result.stats.nodes);
    // After a forced pass the reports are for the side that passes.
    let passing = Board::from_bits(2, 1).unwrap();
    let mut seen = Vec::new();
    let result = engine
        .search_with(passing, &exact(), |progress| seen.push(progress.clone()))
        .unwrap();
    let last = seen.last().unwrap();
    assert_eq!(Some(last.score), result.score);
    assert_eq!(last.best_move, Some(Action::Pass));
    assert_eq!(last.pv.first(), Some(&Action::Pass));
}

#[test]
fn positions_are_searched_for_the_side_to_move() {
    let mut engine = engine(1);
    let black = Position::initial();
    let white = black.play("F5".parse::<Action>().unwrap()).unwrap();
    let from_position = engine.search(white, &depth(4)).unwrap();
    let from_board = engine.search(white.board(), &depth(4)).unwrap();
    assert_eq!(from_position.score, from_board.score);
    assert_eq!(from_position.best_move, from_board.best_move);
    assert!(white.play(from_position.best_move.unwrap()).is_ok());
}

#[test]
fn worker_runs_searches_in_the_background() {
    let worker = EngineWorker::new(engine(2));
    let board = position(7, 11);
    let expected = reference(board);
    let search = worker.start(board, exact()).unwrap();
    assert!(matches!(
        worker.start(board, exact()),
        Err(EngineError::Busy)
    ));
    let reports: Vec<Progress> = search.events().iter().collect();
    let result = search.wait().unwrap();
    assert_eq!(result.score, Some(expected));
    assert_eq!(reports.last().map(|p| p.score), Some(expected));
    // A long search stops on request and leaves the worker reusable.
    let search = worker.start(Board::new(), depth(60)).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    search.stop();
    let result = search.wait().unwrap();
    assert_eq!(result.stop_reason, StopReason::Requested);
    worker.with(|engine| engine.clear_tt()).unwrap();
    // Dropping a handle stops its search, which then ends by itself.
    drop(worker.start(Board::new(), depth(60)).unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while worker.is_busy() {
        assert!(
            std::time::Instant::now() < deadline,
            "dropped search kept running"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let result = worker
        .with(|engine| engine.search(board, &exact()))
        .unwrap();
    assert_eq!(result.score, Some(expected));
}
