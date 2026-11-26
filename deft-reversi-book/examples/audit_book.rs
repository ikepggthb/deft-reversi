//! Read-only input audit. Outputs are written only to the supplied directory.
//! audit_book roundtrip OLD_WORK OUTPUT_DIR
//! audit_book fixed OLD_PLAY NEW_PLAY
//! audit_book workflow MODEL OUTPUT_DIR [MIGRATED_WORK]
use deft_reversi_book::*;
use deft_reversi_engine::*;
use std::{path::Path, sync::Arc, time::Duration};
fn scalar(v: Option<Evaluation>) -> Option<(Score, u8, Selectivity, bool)> {
    v.map(|v| (v.result(), v.depth(), v.selectivity(), v.is_solved()))
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let limits = ReadLimits {
        max_bytes: 2 * 1024 * 1024 * 1024,
        ..ReadLimits::default()
    };
    match args[1].as_str() {
        "roundtrip" => {
            let input = BookEditor::load(&args[2], limits).unwrap();
            let out = Path::new(&args[3]);
            let work = out.join("audited.work");
            let play = out.join("audited.book");
            input.save(&work).unwrap();
            input.export().unwrap().save(&play).unwrap();
            let loaded = BookEditor::load(&work, limits).unwrap();
            let before = input.export().unwrap();
            let after = Book::load(&play, limits).unwrap();
            assert_eq!(input.len(), loaded.len());
            assert_eq!(input.generation_config(), loaded.generation_config());
            let mut candidates = 0;
            let mut registered = 0;
            for b in input.positions() {
                let a = before.probe(b).unwrap();
                let c = after.probe(b).unwrap();
                assert_eq!(a.value(), c.value());
                assert_eq!(a.final_score(), c.final_score());
                assert_eq!(a.all_moves_evaluated(), c.all_moves_evaluated());
                assert_eq!(
                    a.candidates()
                        .map(|c| (c.action, c.registered, c.value, c.final_score))
                        .collect::<Vec<_>>(),
                    c.candidates()
                        .map(|c| (c.action, c.registered, c.value, c.final_score))
                        .collect::<Vec<_>>()
                );
                for c in a.candidates() {
                    candidates += 1;
                    registered += u64::from(c.registered);
                    assert_eq!(
                        input.move_values(b, c.action),
                        loaded.move_values(b, c.action)
                    );
                }
            }
            println!(
                "{{\"positions\":{},\"candidates\":{candidates},\"registered\":{registered},\"all_candidate_records_equal\":true,\"play_values_equal\":true}}",
                input.len()
            );
        }
        "fixed" => {
            let old = Book::load(&args[2], limits).unwrap();
            let new = Book::load(&args[3], limits).unwrap();
            let mut candidates = 0;
            assert_eq!(old.stats().positions, new.stats().positions);
            for b in old.positions() {
                let a = old.probe(b).unwrap();
                let c = new.probe(b).unwrap();
                assert_eq!(scalar(a.value()), scalar(c.value()));
                let x: Vec<_> = a
                    .candidates()
                    .map(|v| (v.action, v.registered, scalar(v.value)))
                    .collect();
                let y: Vec<_> = c
                    .candidates()
                    .map(|v| (v.action, v.registered, scalar(v.value)))
                    .collect();
                assert_eq!(x, y);
                candidates += x.len();
            }
            println!(
                "{{\"positions\":{},\"candidates\":{candidates},\"known_values_and_quality_equal\":true}}",
                old.stats().positions
            );
        }
        "workflow" => {
            let out = Path::new(&args[3]);
            let mut engine = Engine::new(
                Model::from_file(&args[2]).unwrap(),
                EngineOptions::default().threads(18).tt_mib(128),
            )
            .unwrap();
            let mut editor = BookEditor::for_engine(&engine).unwrap();
            let cfg = GenerationConfig {
                analysis: AnalysisSchedule::Levels(vec![19]),
                max_plies: 31,
                expansion_loss: 4,
                cumulative_loss: Some(4),
                leaf_loss: Some(4),
                schedule: SchedulePolicy::Promising,
                ..GenerationConfig::default()
            };
            editor.configure_generation(&[Board::new()], cfg).unwrap();
            let a = editor
                .generate(
                    &mut engine,
                    GenerationLimits {
                        time: Some(Duration::from_secs(2)),
                        ..GenerationLimits::default()
                    },
                    StopHandle::new(),
                )
                .unwrap();
            let first = out.join("real-first.work");
            editor.save(&first).unwrap();
            let mut editor = BookEditor::load(&first, limits).unwrap();
            let b = editor
                .generate(
                    &mut engine,
                    GenerationLimits {
                        time: Some(Duration::from_secs(1)),
                        ..GenerationLimits::default()
                    },
                    StopHandle::new(),
                )
                .unwrap();
            editor.save(out.join("real-resumed.work")).unwrap();
            let mut cfg = editor.generation_config().unwrap().clone();
            let starts = editor.generation_starts();
            cfg.max_plies = 32;
            cfg.expansion_loss = 3;
            editor.configure_generation_roots(&starts, cfg).unwrap();
            let c = editor
                .generate(
                    &mut engine,
                    GenerationLimits {
                        nodes: Some(50000),
                        ..GenerationLimits::default()
                    },
                    StopHandle::new(),
                )
                .unwrap();
            editor.save(out.join("real-updated.work")).unwrap();
            let book = editor.export().unwrap();
            book.save(out.join("real.book")).unwrap();
            let loaded = Arc::new(Book::load(out.join("real.book"), limits).unwrap());
            let mut player = BookPlayer::new(
                &engine,
                Some(loaded),
                UsePolicy {
                    min_depth: 16,
                    moves: MovePolicy::RandomWithinLoss { max_loss: 4 },
                    ..UsePolicy::default()
                },
                3,
            )
            .unwrap();
            let choice = player.decide(Board::new());
            let BookDecision::Chosen(chosen) = choice else {
                panic!("fresh book must offer a qualified move: {choice:?}")
            };
            assert!(Board::new().play(chosen.action).is_ok());
            assert!(!chosen.value.conditions().has_imported());
            let mut migrated_new = 0;
            if let Some(path) = args.get(4) {
                let mut old = BookEditor::load(path, limits).unwrap();
                let r = old
                    .generate(
                        &mut engine,
                        GenerationLimits {
                            nodes: Some(50000),
                            ..GenerationLimits::default()
                        },
                        StopHandle::new(),
                    )
                    .unwrap();
                migrated_new = r.new_positions;
                old.save(out.join("migrated-resumed.work")).unwrap();
                BookEditor::load(out.join("migrated-resumed.work"), limits).unwrap();
            }
            println!(
                "{{\"first_new\":{},\"resumed_new\":{},\"updated_new\":{},\"nodes\":[{},{},{}],\"runs\":{},\"legal_choice\":true,\"no_legacy_in_choice\":true,\"migrated_new\":{migrated_new}}}",
                a.new_positions,
                b.new_positions,
                c.new_positions,
                a.nodes,
                b.nodes,
                c.nodes,
                editor.generation_runs().len()
            );
        }
        _ => panic!("unknown audit operation"),
    }
}
