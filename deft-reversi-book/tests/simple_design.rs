use deft_reversi_book::legacy::{self, AnalysisQuality, ImportedValue, ValueOrigin};
use deft_reversi_book::*;
use deft_reversi_engine::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "deft-shared-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn path(&self, n: &str) -> PathBuf {
        self.0.join(n)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn typed_old_work_converts_without_losing_current_records_and_saves_only_new_formats() {
    let dir = Temp::new();
    let input = dir.path("old.work");
    std::fs::write(&input, include_bytes!("fixtures/search-v7.work")).unwrap();
    let old_bytes = std::fs::read(&input).unwrap();
    let editor = BookEditor::load(&input, ReadLimits::default()).unwrap();
    let report = editor.migration_report().unwrap();
    assert_eq!(report.source_version, 7);
    assert_eq!(report.positions, editor.len() as u64);
    assert!(!editor.generation_runs().is_empty());
    let boards: Vec<_> = editor.positions().collect();
    let current = editor.export().unwrap();
    let work = dir.path("new.work");
    let play = dir.path("new.book");
    editor.save(&work).unwrap();
    current.save(&play).unwrap();
    assert_eq!(
        u32::from_le_bytes(std::fs::read(&work).unwrap()[8..12].try_into().unwrap()),
        8
    );
    assert_eq!(
        u32::from_le_bytes(std::fs::read(&play).unwrap()[8..12].try_into().unwrap()),
        4
    );
    let loaded = BookEditor::load(&work, ReadLimits::default()).unwrap();
    assert_eq!(loaded.migration_report(), None);
    let played = Book::load(&play, ReadLimits::default()).unwrap();
    for b in boards {
        let a = current.probe(b).unwrap();
        let c = played.probe(b).unwrap();
        assert_eq!(a.value(), c.value());
        assert_eq!(a.final_score(), c.final_score());
        assert_eq!(
            a.candidates()
                .map(|c| (c.action, c.registered, c.value, c.final_score))
                .collect::<Vec<_>>(),
            a.moves()
                .map(|c| (
                    c.action,
                    c.registered,
                    c.value.map(EvaluationRef::to_owned),
                    c.final_score.map(EvaluationRef::to_owned)
                ))
                .collect::<Vec<_>>()
        );
        for candidate in a.candidates() {
            assert_eq!(
                editor.move_values(b, candidate.action),
                loaded.move_values(b, candidate.action)
            );
        }
    }
    assert_eq!(std::fs::read(&input).unwrap(), old_bytes);
}
#[test]
fn old_play_keeps_its_values_and_typed_search_information_without_history_api() {
    let dir = Temp::new();
    let input = dir.path("old.book");
    std::fs::write(&input, include_bytes!("fixtures/search-v3.book")).unwrap();
    let old = Book::load(&input, ReadLimits::default()).unwrap();
    let path = dir.path("new.book");
    old.save(&path).unwrap();
    let new = Book::load(&path, ReadLimits::default()).unwrap();
    assert_eq!(old.stats().positions, new.stats().positions);
    let mut searches = 0;
    for b in old.positions() {
        let a = old.probe(b).unwrap();
        let c = new.probe(b).unwrap();
        assert_eq!(a.value(), c.value());
        assert_eq!(
            a.candidates()
                .map(|v| (v.action, v.registered, v.value, v.final_score))
                .collect::<Vec<_>>(),
            c.candidates()
                .map(|v| (v.action, v.registered, v.value, v.final_score))
                .collect::<Vec<_>>()
        );
        searches += a
            .candidates()
            .filter(|v| {
                v.value.is_some_and(|v| {
                    matches!(
                        v.source(),
                        EvaluationSource::Search(_) | EvaluationSource::SearchMove(_)
                    )
                })
            })
            .count();
    }
    assert!(searches > 0);
}
#[test]
fn manual_values_need_explicit_opt_in_and_do_not_claim_a_search_depth() {
    let mut engine = Engine::new(
        Model::from_pattern(PatternModel::zero()),
        EngineOptions::default().tt_mib(1),
    )
    .unwrap();
    let mut e = BookEditor::for_engine(&engine).unwrap();
    let b = Board::new().canonical();
    let a = Action::Place(b.legal_moves().next().unwrap());
    e.set_manual(b, a, 5).unwrap();
    let value = e.move_values(b, a).unwrap().manual.unwrap();
    assert!(matches!(
        value.source(),
        EvaluationSource::Manual { depth: 0, .. }
    ));
    assert_eq!(
        UsePolicy::default().check(value, e.identity()),
        Err(BookMiss::ManualRejected)
    );
    assert_eq!(
        UsePolicy {
            allow_manual: true,
            ..UsePolicy::default()
        }
        .check(value, e.identity()),
        Ok(())
    );
    assert!(e.record_candidate(b, a, value).is_err());
    assert!(
        e.record_manual(
            b,
            a,
            Evaluation::import(ImportedValue {
                origin: ValueOrigin::Search,
                ..ImportedValue::from(value)
            })
            .unwrap()
        )
        .is_err()
    );
    let r = engine
        .search(b, &SearchRequest::new(Goal::Depth(2)))
        .unwrap();
    e.record_search(
        b,
        RequestedAnalysis::External,
        SearchKind::Midgame {
            depth: 2,
            selectivity: Selectivity::Full,
        },
        &r,
    )
    .unwrap();
    assert_eq!(e.move_values(b, a).unwrap().selected, Some(value));
    e.clear_manual(b, a).unwrap();
    assert!(e.move_values(b, a).unwrap().manual.is_none());
}
#[test]
fn one_schedule_is_validated_and_round_trips_without_inactive_settings() {
    let dir = Temp::new();
    let mut e = BookEditor::new([0; 32]);
    for schedule in [
        AnalysisSchedule::Levels(vec![2, 4]),
        AnalysisSchedule::Stages(vec![Stage {
            depth: 2,
            selectivity: Selectivity::Full,
        }]),
    ] {
        let cfg = GenerationConfig {
            analysis: schedule,
            ..GenerationConfig::default()
        };
        assert!(cfg.validate().is_ok());
        e.configure_generation(&[Board::new()], cfg.clone())
            .unwrap();
        let path = dir.path("work");
        e.save(&path).unwrap();
        assert_eq!(
            BookEditor::load(&path, ReadLimits::default())
                .unwrap()
                .generation_config(),
            Some(&cfg)
        );
    }
    assert!(
        GenerationConfig {
            analysis: AnalysisSchedule::Levels(vec![]),
            ..GenerationConfig::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        GenerationConfig {
            analysis: AnalysisSchedule::Stages(vec![]),
            ..GenerationConfig::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn manual_input_does_not_lower_the_required_quality_of_real_searches() {
    let engine = Engine::new(
        Model::from_pattern(PatternModel::zero()),
        EngineOptions::default().tt_mib(1),
    )
    .unwrap();
    for manual_first in [false, true] {
        let mut e = BookEditor::for_engine(&engine).unwrap();
        let b = Board::new().canonical();
        let actions: Vec<_> = b.legal_moves().map(Action::Place).collect();
        if manual_first {
            e.set_manual(b, actions[0], 4).unwrap();
        }
        for &a in &actions[1..] {
            e.record_imported(
                b,
                a,
                ImportedValue {
                    score: 0,
                    quality: AnalysisQuality {
                        depth: 12,
                        selectivity: Selectivity::Full,
                        solved: false,
                    },
                    bound: legacy::Bound::Exact,
                    identity: e.identity(),
                    origin: ValueOrigin::Imported,
                },
            )
            .unwrap();
        }
        if !manual_first {
            e.set_manual(b, actions[0], 4).unwrap();
        }
        let book = e.export().unwrap();
        let value = book.probe(b).unwrap().value().unwrap();
        let policy = UsePolicy {
            allow_manual: true,
            allow_imported: true,
            ..UsePolicy::default()
        };
        assert_eq!(policy.check(value, e.identity()), Ok(()));
        assert_eq!(
            UsePolicy {
                min_depth: 13,
                ..policy
            }
            .check(value, e.identity()),
            Err(BookMiss::MidgameDepth)
        );
    }
}
