#![cfg(feature = "book-cli")]
use deft_reversi_book::*;
use deft_reversi_engine::*;
use std::{
    path::Path,
    process::{Command, Output},
};
fn call(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_book"))
        .args(args)
        .output()
        .unwrap()
}
fn success(args: &[&str]) -> Output {
    let r = call(args);
    assert!(
        r.status.success(),
        "args={args:?}\n{}",
        String::from_utf8_lossy(&r.stderr)
    );
    r
}
fn text(p: &Path) -> &str {
    p.to_str().unwrap()
}
#[test]
fn cli_generation_resume_export_extend_and_retarget() {
    let version = success(&["--version"]);
    assert!(String::from_utf8_lossy(&version.stdout).contains(&format!(
        "endgame_pattern_ordering={}",
        cfg!(feature = "endgame-pattern-ordering")
    )));
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("deft-book-cli-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("model.dpr2");
    let mut bytes = Vec::new();
    PatternModel::zero().write_to(&mut bytes).unwrap();
    std::fs::write(&model, &bytes).unwrap();
    let work = dir.join("state.dbook-work");
    let book = dir.join("play.dbook");
    let copied = dir.join("copied.dbook");
    let positions = dir.join("roots.obf");
    let b = Board::new()
        .play(Action::Place(Board::new().legal_moves().next().unwrap()))
        .unwrap();
    let cells: String = (0..64)
        .map(|i| {
            if b.player_bits() & (1 << i) != 0 {
                'O'
            } else if b.opponent_bits() & (1 << i) != 0 {
                'X'
            } else {
                '-'
            }
        })
        .collect();
    std::fs::write(&positions, format!("{cells} O\n")).unwrap();
    let r = success(&[
        "build",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--book",
        text(&book),
        "--positions",
        text(&positions),
        "--stages",
        "2",
        "--selectivity",
        "100",
        "--max-plies",
        "1",
        "--nodes",
        "1",
        "--tt-mib",
        "1",
    ]);
    assert!(String::from_utf8_lossy(&r.stdout).contains("NodeLimit"));
    success(&[
        "resume",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--book",
        text(&book),
        "--nodes",
        "10000",
        "--tt-mib",
        "1",
    ]);
    success(&["inspect", "--work", text(&work)]);
    let detail = success(&[
        "inspect",
        "--work",
        text(&work),
        "--positions",
        text(&positions),
    ]);
    let detail = String::from_utf8_lossy(&detail.stdout);
    assert!(detail.contains("requested: ParentDepth"));
    assert!(detail.contains("completed: Midgame { depth: 2"));
    assert!(detail.contains("analysis=Derived"));
    let detail = success(&[
        "inspect",
        "--book",
        text(&book),
        "--positions",
        text(&positions),
    ]);
    assert!(String::from_utf8_lossy(&detail.stdout).contains("position player="));
    success(&["verify", "--book", text(&book)]);
    success(&["export", "--work", text(&work), "--book", text(&copied)]);
    assert_eq!(
        std::fs::read(&book).unwrap(),
        std::fs::read(&copied).unwrap()
    );
    let before = std::fs::read(&work).unwrap();
    assert!(
        !call(&["export", "--work", text(&work), "--book", text(&work)])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&work).unwrap(), before);

    assert!(
        !call(&[
            "build",
            "--model",
            text(&model),
            "--work",
            text(&work),
            "--nodes",
            "1"
        ])
        .status
        .success()
    );
    assert_eq!(std::fs::read(&work).unwrap(), before);
    success(&[
        "extend",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--stages",
        "4",
        "--selectivity",
        "100",
        "--nodes",
        "50000",
        "--tt-mib",
        "1",
    ]);
    success(&[
        "extend",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--levels",
        "1,2",
        "--schedule",
        "frontier",
        "--cumulative-loss",
        "5",
        "--leaf-loss",
        "2",
        "--nodes",
        "50000",
        "--tt-mib",
        "1",
    ]);
    assert_eq!(
        BookEditor::load(&work, ReadLimits::default())
            .unwrap()
            .generation_config()
            .unwrap()
            .analysis,
        AnalysisSchedule::Levels(vec![1, 2])
    );
    let loss_config = BookEditor::load(&work, ReadLimits::default())
        .unwrap()
        .generation_config()
        .unwrap()
        .clone();
    assert_eq!(loss_config.schedule, SchedulePolicy::Frontier);
    assert_eq!(loss_config.cumulative_loss, Some(5));
    assert_eq!(loss_config.leaf_loss, Some(2));
    for extra in [
        vec!["--levels", "2"],
        vec!["--cumulative-loss", "5"],
        vec!["--schedule", "breadth"],
        vec!["--leaf-loss", "2"],
        vec!["--levels", "2", "--stages", "3"],
    ] {
        let mut args = vec![
            "resume",
            "--model",
            text(&model),
            "--work",
            text(&work),
            "--nodes",
            "0",
        ];
        args.extend(extra);
        assert!(!call(&args).status.success());
    }
    assert!(
        !call(&[
            "extend",
            "--model",
            text(&model),
            "--work",
            text(&work),
            "--levels",
            "2",
            "--stages",
            "3",
            "--nodes",
            "0"
        ])
        .status
        .success()
    );
    let n = bytes.len();
    bytes[n - 2..].copy_from_slice(&128i16.to_le_bytes());
    std::fs::write(&model, &bytes).unwrap();
    success(&[
        "reanalyze",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--stages",
        "2",
        "--selectivity",
        "100",
        "--nodes",
        "50000",
        "--tt-mib",
        "1",
    ]);
    let editor = BookEditor::load(&work, ReadLimits::default()).unwrap();
    assert_eq!(editor.generation_roots(), vec![b.canonical()]);
    assert_eq!(editor.generation_config().unwrap().max_plies, 1);
    assert_eq!(editor.generation_starts()[0].side, BookSide::White);
    assert!(
        !call(&[
            "resume",
            "--model",
            text(&model),
            "--work",
            text(&work),
            "--nodes",
            "1",
            "--stages",
            "2"
        ])
        .status
        .success()
    );
    let registered_before = editor.stats().registered_moves;
    success(&["repair", "--work", text(&work), "--book", text(&book)]);
    let inspected = success(&["inspect", "--work", text(&work)]);
    assert!(String::from_utf8_lossy(&inspected.stdout).contains("root_lines"));
    assert!(
        !call(&[
            "resume",
            "--model",
            text(&model),
            "--work",
            text(&work),
            "--nodes",
            "0",
            "--propagation",
            "quality"
        ])
        .status
        .success()
    );
    assert!(
        !call(&["repair", "--work", text(&work), "--book", text(&work)])
            .status
            .success()
    );
    success(&[
        "leaves",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--levels",
        "1",
        "--nodes",
        "1",
        "--tt-mib",
        "1",
    ]);
    assert!(
        BookEditor::load(&work, ReadLimits::default())
            .unwrap()
            .generation_config()
            .unwrap()
            .frontier_only
    );
    success(&[
        "resume",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--nodes",
        "50000",
        "--tt-mib",
        "1",
    ]);
    assert_eq!(
        BookEditor::load(&work, ReadLimits::default())
            .unwrap()
            .stats()
            .registered_moves,
        registered_before
    );
    success(&[
        "extend",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--levels",
        "1",
        "--nodes",
        "0",
        "--tt-mib",
        "1",
    ]);
    let config = BookEditor::load(&work, ReadLimits::default())
        .unwrap()
        .generation_config()
        .unwrap()
        .clone();
    assert!(!config.frontier_only);
    assert!(config.reuse_completed);
    std::fs::remove_dir_all(dir).unwrap();
}
