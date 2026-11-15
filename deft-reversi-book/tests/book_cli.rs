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
/// Writes the pattern weights `bytes` as a model with a calibration: the
/// book searches selectively, which needs one.
fn write_model(path: &Path, bytes: &[u8]) {
    let pattern = PatternModel::read_from(std::io::Cursor::new(bytes)).unwrap();
    let model = Model::from_pattern(pattern)
        .with_calibration(legacy_main_rules(), ProbCutSigmas::NORMAL)
        .unwrap();
    let mut out = Vec::new();
    model.write_to(&mut out).unwrap();
    std::fs::write(path, out).unwrap();
}
fn text(p: &Path) -> &str {
    p.to_str().unwrap()
}
#[test]
fn cli_generation_resume_export_extend_and_retarget() {
    let dir = std::env::temp_dir().join(format!("deft-book-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model = dir.join("model.dpr2");
    let mut bytes = Vec::new();
    PatternModel::zero().write_to(&mut bytes).unwrap();
    write_model(&model, &bytes);
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
        "--nodes",
        "50000",
        "--tt-mib",
        "1",
    ]);
    let n = bytes.len();
    bytes[n - 2..].copy_from_slice(&128i16.to_le_bytes());
    write_model(&model, &bytes);
    success(&[
        "reanalyze",
        "--model",
        text(&model),
        "--work",
        text(&work),
        "--stages",
        "2",
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
    std::fs::remove_dir_all(dir).unwrap();
}
