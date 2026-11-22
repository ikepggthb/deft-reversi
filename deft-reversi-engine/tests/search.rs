use deft_reversi_engine::*;
use std::time::Duration;
/// The zero model with the legacy main rules, so selective searches run.
fn model() -> Model {
    Model::from_pattern(PatternModel::zero())
        .with_calibration(legacy_main_rules(), ProbCutSigmas::NORMAL)
        .unwrap()
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
fn skipping_pv_recovery_keeps_the_best_move_and_score() {
    let board = position(9, 14);
    let mut engine = engine(1);
    let expected = engine.search(board, &exact()).unwrap();
    let without_recovery = engine.search(board, &exact().skip_pv_recovery()).unwrap();
    assert_eq!(without_recovery.stop_reason, StopReason::Completed);
    assert!(without_recovery.is_solved());
    assert_eq!(without_recovery.best_move, expected.best_move);
    assert_eq!(without_recovery.score, expected.score);
    assert!(without_recovery.stats.tt_hits > 0);
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
fn timed_levels_return_last_completed_stage_on_timeout() {
    let mut engine = engine(4);
    let board = position(21, 28);
    let budget = Duration::from_millis(500);
    let request = SearchRequest::timed_levels(60, budget).skip_pv_recovery();
    let mut last = None;
    let result = engine
        .search_with(board, &request, |progress| {
            last = Some(progress.clone());
            // Let the actual search clock expire between two stages.
            std::thread::sleep(budget.saturating_sub(progress.elapsed));
        })
        .unwrap();
    let last = last.expect("the first stage should complete");
    assert_eq!(
        last.kind,
        ResultKind::Depth {
            depth: 5,
            selectivity: Selectivity::Percent83
        }
    );
    assert_eq!(result.stop_reason, StopReason::TimeLimit);
    assert_eq!(result.kind, Some(last.kind));
    assert_eq!(result.score, Some(last.score));
    assert_eq!(result.best_move, last.best_move);
    assert!(board.play(result.best_move.unwrap()).is_ok());
}

#[test]
fn timed_levels_searches_selective_stages_and_solves_exactly() {
    let mut engine = engine(4);
    let board = position(9, 14);
    let fixed = engine.search(board, &exact()).unwrap();
    let budget = Duration::from_secs(5);
    let mut reported = Vec::new();
    let timed = engine
        .search_with(
            board,
            &SearchRequest::timed_levels(60, budget),
            |progress| {
                reported.push(progress.kind);
            },
        )
        .unwrap();
    assert_eq!(timed.stop_reason, StopReason::Completed);
    assert!(timed.is_solved());
    assert_eq!(timed.score, fixed.score);
    assert!(timed.stats.tt_hits > 0);
    let expected: Vec<_> = [2, 4]
        .into_iter()
        .map(|depth| ResultKind::Depth {
            depth,
            selectivity: Selectivity::Percent83,
        })
        .chain(
            [Selectivity::Percent85, Selectivity::Full]
                .map(|selectivity| ResultKind::Endgame { selectivity }),
        )
        .collect();
    assert_eq!(reported, expected);
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
        // With 8 or more threads exact nodes split only from 15-17 empties.
        if (2..8).contains(&threads) {
            assert!(splits > 0, "no split with {threads} threads");
            assert!(helper_jobs > 0, "no helper executed with {threads} threads");
        }
    }
    // Too many empties for the brute-force reference: one thread is it.
    let boards: Vec<Board> = (1..=5).map(|seed| position(seed, 20)).collect();
    let expected: Vec<_> = boards
        .iter()
        .map(|&board| engine(1).search(board, &exact()).unwrap().score)
        .collect();
    for threads in [8, 20] {
        let mut engine = engine(threads);
        let mut splits = 0;
        let mut helper_jobs = 0;
        for (&board, &expected) in boards.iter().zip(&expected) {
            for _ in 0..5 {
                let result = engine.search(board, &exact()).unwrap();
                assert_eq!(result.score, expected);
                assert_eq!(result.stop_reason, StopReason::Completed);
                splits += result.stats.splits;
                helper_jobs += result.stats.helper_jobs;
            }
        }
        assert!(splits > 0, "no split with {threads} threads");
        assert!(helper_jobs > 0, "no helper executed with {threads} threads");
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
        // Fourteen empties: ProbCut runs only at null-window nodes, and the
        // thirteen-empty children after the first one are searched so. The
        // expected value is a solve without any ProbCut rule (the plain
        // minimax reference is too slow at fourteen empties).
        for seed in [3, 7, 11] {
            let board = position(seed, 14);
            let mut plain =
                Engine::new(model(), EngineOptions::default().threads(1).tt_mib(1)).unwrap();
            plain.set_probcut_rules(Vec::new()).unwrap();
            let expected = plain.search(board, &exact()).unwrap().score.unwrap();
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
fn selective_endgame_scores_are_even() {
    // A final score is even, and a ProbCut cut at an odd window edge must not
    // leak an odd one. The zero model predicts the intercept everywhere, so
    // these rules cut every null window on one side of it.
    let rules = |intercept: f64| -> Vec<ProbCutRule> {
        (12..=15)
            .map(|empties| ProbCutRule {
                exact: true,
                depth: empties,
                empties,
                shallow_depth: 0,
                slope: 1.0,
                intercept,
                deviation: 0.0,
            })
            .collect()
    };
    for threads in [1, 4] {
        let mut engine = engine(threads);
        let mut cuts = 0;
        for intercept in [-9.0, -3.0, 1.0, 5.0, 11.0] {
            engine.set_probcut_rules(rules(intercept)).unwrap();
            for seed in [3, 7, 11] {
                let board = position(seed, 16);
                let request = exact().selectivity(Selectivity::Percent85).all_moves();
                let result = engine.search(board, &request).unwrap();
                assert_eq!(result.stop_reason, StopReason::Completed);
                cuts += result.stats.probcut_cuts;
                let score = result.score.unwrap();
                assert_eq!(score % 2, 0, "seed {seed}: root score {score}");
                for scored in &result.moves {
                    assert_eq!(scored.score % 2, 0, "seed {seed}: {scored:?}");
                }
            }
        }
        assert!(cuts > 0);
    }
}

#[test]
fn selective_endgame_recovers_a_warm_pv_without_changing_the_result() {
    let board = position(9, 14);
    let request = exact().selectivity(Selectivity::Percent99);
    let run = |request_with_pv| {
        let mut engine = engine(1);
        engine.search(board, &request).unwrap();
        engine.search(board, &request_with_pv).unwrap()
    };
    let warm = run(request.clone());
    assert!(warm.pv_final_score(board).is_none());
    let recovered = run(request.clone().recover_endgame_pv());
    assert_eq!(recovered.best_move, warm.best_move);
    assert_eq!(recovered.score, warm.score);
    assert_eq!(recovered.kind, warm.kind);
    assert_eq!(recovered.pv_final_score(board), warm.score);
}

#[test]
fn selective_midgame_recovers_a_warm_pv_without_changing_the_result() {
    for seed in [3, 9, 21] {
        let board = position(seed, 40);
        let request = depth(8).selectivity(Selectivity::Percent85);
        let run = |request_with_pv: &SearchRequest| {
            let mut engine = engine(1);
            engine.search(board, &request).unwrap();
            engine.search(board, request_with_pv).unwrap()
        };
        let warm = run(&request);
        let recovered = run(&request.clone().recover_pv());
        assert_eq!(recovered.best_move, warm.best_move, "seed {seed}");
        assert_eq!(recovered.score, warm.score, "seed {seed}");
        assert_eq!(recovered.kind, warm.kind, "seed {seed}");
        assert_eq!(recovered.pv.first(), warm.pv.first(), "seed {seed}");
        // The warm table cut the line short; recovery reads it to the depth.
        assert!(warm.pv.len() < 8, "seed {seed}");
        assert_eq!(recovered.pv.len(), 8, "seed {seed}");
        let mut line = board;
        for &action in &recovered.pv {
            line = line.play(action).unwrap();
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
    assert_eq!(stopped.best_move, prepared.best_move);
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
    assert!(depth > 0 && depth < 16);
    assert_eq!(selectivity, Selectivity::Percent83);
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
    let board = position(9, 8);
    let mut reviewer = engine(1);
    let all = reviewer.search(board, &exact().all_moves()).unwrap();
    for played in board.legal_moves() {
        let result = reviewer
            .search(board, &exact().review_move(played))
            .unwrap();
        assert_eq!(result.score, all.score);
        assert_eq!(result.moves[0].score, all.score.unwrap());
        assert_eq!(
            result.moves.len(),
            if result.best_move == Some(Action::Place(played)) {
                1
            } else {
                2
            }
        );
        assert!(result.moves.iter().all(|score| all.moves.contains(score)));
        assert!(
            result
                .moves
                .iter()
                .any(|m| m.action == Action::Place(played))
        );
    }
    // A stopped search leaves the list empty rather than partial.
    let result = engine(1)
        .search(Board::new(), &depth(10).all_moves().node_limit(500))
        .unwrap();
    assert!(result.moves.is_empty());
}

#[test]
fn candidate_lines_are_legal_and_match_full_window_scores() {
    let board = position(9, 8);
    let mut engine = engine(4);
    let result = engine.search(board, &exact().all_moves_with_pv()).unwrap();
    assert!(result.is_solved());
    assert_eq!(result.moves.len(), board.legal_moves().count());
    assert_eq!(result.moves.len(), result.move_pvs.len());
    for (scored, pv) in result.moves.iter().zip(result.move_pvs) {
        assert_eq!(pv.first(), Some(&scored.action));
        let child = board.play(scored.action).unwrap();
        assert_eq!(scored.score, -reference(child));
        let final_board = pv
            .iter()
            .try_fold(board, |board, &action| board.play(action))
            .unwrap();
        if final_board.is_terminal() {
            let score = final_board.final_score();
            assert_eq!(scored.score, if pv.len() % 2 == 0 { score } else { -score });
        }
    }

    let stopped = engine
        .search(Board::new(), &depth(14).all_moves_with_pv().node_limit(500))
        .unwrap();
    assert!(stopped.moves.is_empty());
    assert!(stopped.move_pvs.is_empty());
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

#[test]
fn probcut_sigmas_set_the_cut_distance() {
    // The zero model scores every position 0; the intercept predicts +2, so
    // a near cut distance cuts at low windows and a far one never does.
    let board = Board::new();
    let rules: Vec<ProbCutRule> = (2..=8)
        .flat_map(|depth| {
            (depth..=60).map(move |empties| ProbCutRule {
                exact: false,
                depth,
                empties,
                shallow_depth: 1,
                slope: 1.0,
                intercept: 2.0,
                deviation: 1.0,
            })
        })
        .collect();
    let request = depth(5).selectivity(Selectivity::Percent95);
    let cuts = |sigmas: ProbCutSigmas| {
        let mut engine = engine(1);
        engine
            .set_probcut_calibration(rules.clone(), sigmas)
            .unwrap();
        let mut settings = Vec::new();
        engine.write_analysis_settings(&mut settings).unwrap();
        (engine.search(board, &request).unwrap().stats, settings)
    };
    let near = ProbCutSigmas {
        midgame: [0.001; 5],
        exact: [100.0; 5],
    };
    let far = ProbCutSigmas {
        midgame: [100.0; 5],
        exact: [0.001; 5],
    };
    let (near_stats, near_settings) = cuts(near);
    let (far_stats, _) = cuts(far);
    assert!(near_stats.probcut_cuts > 0);
    assert!(far_stats.probcut_tries > 0);
    assert_eq!(far_stats.probcut_cuts, 0);

    // The normal multiples are the default and leave the settings unchanged.
    let (_, normal_settings) = cuts(ProbCutSigmas::NORMAL);
    let mut engine = engine(1);
    engine.set_probcut_rules(rules.clone()).unwrap();
    let mut settings = Vec::new();
    engine.write_analysis_settings(&mut settings).unwrap();
    assert_eq!(settings, normal_settings);
    assert_ne!(settings, near_settings);
    assert_eq!(ProbCutSigmas::default(), ProbCutSigmas::NORMAL);

    let mut decreasing = ProbCutSigmas::NORMAL;
    decreasing.exact[4] = 1.0;
    let mut nan = ProbCutSigmas::NORMAL;
    nan.midgame[0] = f64::NAN;
    for bad in [decreasing, nan] {
        assert!(matches!(
            engine.set_probcut_calibration(rules.clone(), bad),
            Err(EngineError::InvalidCalibration(_))
        ));
    }
}

#[test]
fn plan_changes_a_levels_midgame_and_endgame() {
    let board = position(41, 12);
    let mut engine = engine(1);
    let mut search = |plan| {
        engine
            .search(board, &SearchRequest::new(Goal::Plan(plan)))
            .unwrap()
    };
    // Level 10 solves 12 empties exactly, as its plan does.
    let level = SearchPlan::level(10).unwrap();
    assert_eq!(level.endgame_limit(Selectivity::Full), 16);
    assert!(search(level).is_solved());
    let midgame = SearchPlan::midgame(8, Selectivity::Full);
    assert_eq!(
        search(midgame).kind,
        Some(ResultKind::Depth {
            depth: 8,
            selectivity: Selectivity::Full
        })
    );
    assert_eq!(
        search(midgame.endgame(Selectivity::Percent95, 12)).kind,
        Some(ResultKind::Endgame {
            selectivity: Selectivity::Percent95
        })
    );
    // The most accurate stage that reaches the position is used.
    let exact = search(level.endgame(Selectivity::Percent95, 20));
    assert!(exact.is_solved());
    assert_eq!(exact.score, Some(reference(board)));
    assert!(SearchPlan::level(0).is_err() && SearchPlan::level(61).is_err());
    let too_deep = SearchPlan::midgame(61, Selectivity::Full);
    assert!(matches!(
        engine.search(board, &SearchRequest::new(Goal::Plan(too_deep))),
        Err(EngineError::InvalidRequest(_))
    ));
}

#[test]
fn tables_kept_across_stages_still_solve_exactly() {
    let mut engine = engine(1);
    for seed in 51..=58 {
        let board = position(seed, 12);
        // Each stage leaves entries behind for the next one.
        for request in [
            depth(6).selectivity(Selectivity::Percent83),
            exact().selectivity(Selectivity::Percent83),
            exact().selectivity(Selectivity::Percent85),
            exact().selectivity(Selectivity::Percent98),
            depth(8).selectivity(Selectivity::Percent95),
        ] {
            engine.search(board, &request).unwrap();
        }
        let result = engine.search(board, &exact()).unwrap();
        assert!(result.is_solved());
        assert_eq!(result.score, Some(reference(board)));
        let best = board.play(result.best_move.unwrap()).unwrap();
        assert_eq!(-reference(best), reference(board));
    }
}

#[test]
fn depth_searches_reaching_the_end_score_finished_games_exactly() {
    // The zero model values every position 0, so a depth search that reaches
    // the end must take the final score from the finished games, not the
    // evaluation. Two empties: F1 scores 0 and G5 +4 for the side to move.
    let board = Board::from_bits(0x45c98a3f48661ccf, 0xba367580b799e310).unwrap();
    let mut engine = engine(1);
    let solved = engine.search(board, &exact()).unwrap();
    assert_eq!(solved.score, Some(4));
    engine.clear_tt().unwrap();
    let deep = engine.search(board, &depth(2)).unwrap();
    assert_eq!(
        (deep.score, deep.best_move),
        (solved.score, solved.best_move)
    );
    // Small depth searches to the end agree with the exact score.
    for seed in 61..=76 {
        let board = position(seed, 6);
        engine.clear_tt().unwrap();
        let result = engine.search(board, &depth(board.empty_count())).unwrap();
        assert_eq!(result.score, Some(reference(board)), "seed {seed}");
    }
}

#[test]
fn root_threshold_keeps_equal_moves_and_reports_only_an_upper_bound_on_failure() {
    let mut engine = engine(1);
    let board = Board::new();
    let square = board.legal_moves().next().unwrap();
    let request = depth(6).root_moves(1 << square.index());

    let rejected = engine
        .search(board, &request.clone().minimum_score(1))
        .unwrap();
    assert_eq!(rejected.stop_reason, StopReason::Completed);
    assert_eq!(rejected.score, Some(0));
    assert!(rejected.score_is_upper_bound);
    assert!(rejected.pv.is_empty());
    assert_eq!(rejected.root_moves, 1 << square.index());

    let accepted = engine.search(board, &request.minimum_score(0)).unwrap();
    assert_eq!(accepted.score, Some(0));
    assert!(!accepted.score_is_upper_bound);
    assert_eq!(accepted.pv.first(), Some(&Action::Place(square)));
}
