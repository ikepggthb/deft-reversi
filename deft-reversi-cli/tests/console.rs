use deft_reversi_book::{BookEditor, RequestedAnalysis, SearchKind};
use deft_reversi_engine::{
    Action, Board, Engine, EngineOptions, Goal, SearchRequest, Model, PatternModel, Position, ProbCutSigmas,
    Selectivity, legacy_main_rules,
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Fixture {
    fn call(&self, args: &[&str], input: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_deft-reversi-cli"))
            .args(args)
            .env("DEFT_MODEL", self.0.join("model.dpr2"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    fn success(&self, args: &[&str], input: &str) -> String {
        let out = self.call(args, input);
        assert!(
            out.status.success(),
            "{args:?}\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

#[test]
fn commands_work_through_the_binary() {
    let f = Fixture(std::env::temp_dir().join(format!("deft-console-{}", std::process::id())));
    fs::create_dir_all(&f.0).unwrap();
    let model = Model::from_pattern(PatternModel::zero())
        .with_calibration(legacy_main_rules(), ProbCutSigmas::NORMAL)
        .unwrap();
    model
        .write_to(fs::File::create(f.0.join("model.dpr2")).unwrap())
        .unwrap();
    let mut engine = Engine::new(model, EngineOptions::default().tt_mib(1)).unwrap();
    let mut book = BookEditor::for_engine(&engine).unwrap();
    let board = Board::new().canonical();
    let result = engine.search(board, &SearchRequest::new(Goal::Depth(12))).unwrap();
    book.register_move(board, result.best_move.unwrap()).unwrap();
    book.record_search(
        board,
        RequestedAnalysis::External,
        SearchKind::Midgame { depth: 12, selectivity: Selectivity::Full },
        &result,
    ).unwrap();
    let book_path = f.0.join("test.dbook");
    book.export().unwrap().save(&book_path).unwrap();

    let inspect = f.success(&["play", "--mode", "3", "--depth", "2", "--threads", "1", "--tt-mib", "3"],
        "settings\nendgame clear\neval 2 100\nendgame 95 8\nendgame 100 4\nsettings\nhint\nanalyze\nquit\n");
    assert!(inspect.contains("allocated main table 2 MiB"));
    assert!(inspect.contains("Model SHA-256:"));
    assert!(inspect.contains("Binary SHA-256:"));
    assert!(inspect.contains("95%: 8"));
    assert!(inspect.contains("100%: 4"));
    assert!(inspect.contains("Move   Score   Search       PV"));
    assert!(inspect.contains("Board unchanged"));
    assert!(!inspect.contains("unavailable"));
    let file = f.0.join("positions with spaces.obf");
    let forced_pass = "O".repeat(32) + &"X".repeat(30) + "-- X";
    fs::write(&file, format!("{}\n{}\n", Position::initial(), forced_pass)).unwrap();
    let inspect = f.success(&["play", "--mode", "3", "--depth", "1", "--threads", "1", "--tt-mib", "1"],
        &format!("setboard --file \"{}\" --line 2\nsettings\nsetboard --file \"{}\" --line 0\nsetboard {}\nhelp\nquit\n", file.display(), file.display(), Position::initial()));
    assert!(inspect.contains("Position loaded"));
    assert!(inspect.contains("passes."));
    assert!(inspect.contains("position index out of range"));

    let gtp_check = f.success(&["gtp", "--level", "1", "--threads", "1", "--tt-mib", "1"],
        "1 protocol_version\n2 known_command genmove # comment\n3 boardsize 7\n4 komi 6.5\n5 play black f5 extra\n6 showboard\n7 play white e6\n8 showboard\n9 play black f5\n10 showboard\n11 boardsize 8\n12 showboard\n13 final_score\n14 time_settings NaN 0 0\n15 does_not_exist\n16 quit\n");
    let replies: Vec<_> = gtp_check.split("\n\n").filter(|s| !s.is_empty()).collect();
    assert_eq!(replies.len(), 16);
    assert_eq!(replies[0], "=1 2");
    assert_eq!(replies[1], "=2 true");
    for i in [2, 3, 4, 6, 12, 13, 14] {
        assert!(replies[i].starts_with('?'), "{}", replies[i]);
    }
    let board = |r: &str| r.split_once('\n').unwrap().1.to_string();
    assert_eq!(board(replies[5]), board(replies[7]));
    assert_ne!(board(replies[5]), board(replies[9]));
    assert_eq!(board(replies[5]), board(replies[11]));
    let reviewed = f.success(
        &[
            "review",
            "f5d6c3",
            "--eval",
            "2@100",
            "--threads",
            "1",
            "--tt-mib",
            "1",
        ],
        "",
    );
    assert!(reviewed.contains("scored 3/3"));
    assert!(!reviewed.contains("unscored:"));
    assert!(
        !f.call(&["review", "f5f5", "--depth", "1"], "")
            .status
            .success()
    );
    let interactive = f.success(&["play", "--mode", "3", "--eval", "2@100", "--threads", "1", "--tt-mib", "1"],
        "search\nplay f5d6c3\nreview\nsearch --eval 1@100 --endgame 100:2 --time-ms 0\nsearch\nquit\n");
    assert!(interactive.contains("scored 3/3"));
    assert!(interactive.contains("eval 1@100; endgame 100:2; time unlimited"));

    let perft = f.success(&["perft", "6"], "");
    assert!(perft.contains("8200"));
    assert!(
        f.success(&["perft", "6", "--count-pass"], "")
            .contains("8200")
    );
    let settings = ["--threads", "1", "--tt-mib", "1"];
    let mut play = vec!["play", "--depth", "1", "--mode", "3"];
    play.extend(settings);
    let output = f.success(&play, "help\nplay f5d6c3\nundo\nredo\nlevel 1\nl 2\ndepth 1\nexact\ndepth 1\ntime 1\ntime 0\nnew\ninit\nmode 0\nf5\nundo\nredo\nmode 1\nmode 3\ngo\nbook on\nlevel 61\nplay f5f5\nquit\n");
    assert!(output.contains("f5d6c3"));
    assert!(output.contains("White (O) plays"));
    assert!(output.contains("usage: level"));
    assert!(output.contains("No book loaded"));
    // EOF also exits cleanly, and mode 2 finishes an entire game.
    f.success(&play, "");
    assert!(
        f.success(
            &[
                "play",
                "--depth",
                "1",
                "--mode",
                "2",
                "--threads",
                "2",
                "--tt-mib",
                "1"
            ],
            ""
        )
        .contains("Game over:")
    );
    let output = f.success(
        &[
            "play",
            "--book",
            book_path.to_str().unwrap(),
            "--book-loss",
            "0",
            "--depth",
            "1",
            "--mode",
            "3",
            "--threads",
            "1",
            "--tt-mib",
            "1",
        ],
        "book off\nbook on\ngo\nquit\n",
    );
    assert!(output.contains("from the book"));

    let records = f.0.join("games.txt");
    let self_play = [
        "self-play",
        "2",
        "--depth",
        "1",
        "--random",
        "10",
        "--seed",
        "7",
        "--out",
        records.to_str().unwrap(),
        "--threads",
        "1",
        "--tt-mib",
        "1",
    ];
    f.success(&self_play, "");
    let games = fs::read_to_string(&records).unwrap();
    assert_eq!(games.lines().count(), 2);
    for record in games.lines() {
        let mut position = Position::initial();
        for square in record.as_bytes().as_chunks::<2>().0 {
            position = position
                .play(
                    std::str::from_utf8(square)
                        .unwrap()
                        .parse::<Action>()
                        .unwrap(),
                )
                .unwrap();
            if position.must_pass() {
                position = position.play(Action::Pass).unwrap();
            }
        }
        assert!(position.is_terminal());
    }
    f.success(&self_play, "");
    assert_eq!(games, fs::read_to_string(&records).unwrap());
    let mut pos = Position::initial();
    let mut gtp_record = String::new();
    for chunk in games.lines().next().unwrap().as_bytes().as_chunks::<2>().0 {
        let text = std::str::from_utf8(chunk).unwrap();
        gtp_record.push_str(&format!(
            "play {} {text}\n",
            if pos.side_to_move() == deft_reversi_engine::Color::Black {
                "black"
            } else {
                "white"
            }
        ));
        pos = pos.play(text.parse::<Action>().unwrap()).unwrap();
        if pos.must_pass() {
            pos = pos.play(Action::Pass).unwrap();
        }
    }
    gtp_record.push_str("99 final_score\n100 quit\n");
    let finished = f.success(
        &["gtp", "--level", "1", "--threads", "1", "--tt-mib", "1"],
        &gtp_record,
    );
    assert!(!finished.contains('?'));
    let score = pos.final_score(deft_reversi_engine::Color::Black);
    let expected = if score == 0 {
        "0".into()
    } else {
        format!("{}+{}", if score > 0 { "B" } else { "W" }, score.abs())
    };
    assert!(finished.contains(&format!("=99 {expected}\n")));

    let positions = f.0.join("positions.obf");
    fs::write(
        &positions,
        format!("{} X; pass:-12\n", "O".repeat(32) + &"X".repeat(30) + "--"),
    )
    .unwrap();
    let solve = [
        "solve",
        positions.to_str().unwrap(),
        "--exact",
        "--clear-tt",
        "--threads",
        "1",
        "--tt-mib",
        "1",
    ];
    assert!(f.success(&solve, "").contains("ok"));
    let legacy = f.call(
        &[
            "--solve",
            positions.to_str().unwrap(),
            "--eval",
            "1@100",
            "--endgame",
            "100:2",
            "--tsv",
            "--threads",
            "1",
            "--tt-mib",
            "1",
        ],
        "",
    );
    assert!(legacy.status.success());
    assert!(String::from_utf8_lossy(&legacy.stdout).contains("\tExact\t2\t"));
    assert!(String::from_utf8_lossy(&legacy.stderr).contains("Search: eval 1@100; endgame 100:2"));
    let custom = f.success(
        &[
            "solve",
            positions.to_str().unwrap(),
            "--eval-depth",
            "1",
            "--eval-mpc",
            "100",
            "--endgame",
            "100:2",
            "--tsv",
            "--threads",
            "1",
            "--tt-mib",
            "1",
        ],
        "",
    );
    assert!(custom.contains("\tExact\t2\t"));
    let mut machine = solve.to_vec();
    machine.push("--tsv");
    let output = f.success(&machine, "");
    let rows: Vec<_> = output.lines().collect();
    assert_eq!(rows.len(), 2);
    let fields: Vec<_> = rows[1].split('\t').collect();
    assert_eq!(fields.len(), rows[0].split('\t').count());
    assert_eq!(&fields[..5], &["1", "-12", "pass", "Exact", "2"]);
    assert_eq!(fields[12], "Completed");
    assert_eq!(fields[11].split('.').nth(1).unwrap().len(), 9);
    assert!(fields[11].parse::<f64>().unwrap() >= 0.0);

    let gtp = ["gtp", "--level", "1", "--threads", "1", "--tt-mib", "1"];
    let output = f.success(&gtp, "protocol_version\nname\nclear_board\nplay black f5\ngenmove white\nclear_board\ngenmove black\nquit\n");
    let replies: Vec<_> = output.split("\n\n").filter(|r| !r.is_empty()).collect();
    assert_eq!(replies.len(), 8);
    assert_eq!(replies[0], "= 2");
    assert_eq!(replies[1], "= Deft");
    assert!(replies.iter().all(|r| r.starts_with("= ")));
    for i in [4, 6] {
        assert!(replies[i][2..].parse::<Action>().is_ok());
    }
    let output = f.success(
        &["gtp", "--game-ms", "100", "--threads", "1", "--tt-mib", "1"],
        "time_settings 0.1 0 0\ntime_left black 0.1 0\ngenmove black\nclear_board\nquit\n",
    );
    assert!(!output.contains('?'));
    assert!(
        !f.call(&["gtp", "--level", "1", "--game-ms", "100"], "")
            .status
            .success()
    );
    assert!(!f.call(&["gtp", "--level", "61"], "").status.success());
    fs::write(
        &positions,
        format!("{} X; G8:+64\n", "O".repeat(32) + &"X".repeat(30) + "--"),
    )
    .unwrap();
    assert!(!f.call(&solve, "").status.success());
    assert!(!f.call(&machine, "").status.success());
    fs::write(&positions, "bad position\n").unwrap();
    assert!(!f.call(&solve, "").status.success());
    assert!(
        !f.call(&["play", "--model", "/missing/model.dpr2"], "")
            .status
            .success()
    );
}
