//! Offline book adapter. No application history, file policy, or signals in search.
//!
//! CLIは引数→GenerationConfig/GenerationLimitsへの変換、モデル・ファイルの読書き、
//! Ctrl-Cと定期保存を担当する。評価の採否や生成順序の実装はライブラリ側に置く。
//! run内のコマンド別分岐から、configure_generation_roots / generate_with_checkpoint、
//! export / save / loadへ進むとCLIと実装の対応を追える。
use deft_reversi_book::*;
use deft_reversi_engine::{Board, Engine, EngineOptions, Model, Selectivity, StopHandle};
use std::{collections::BTreeMap, error::Error, path::Path, time::Duration};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn load_work(path: impl AsRef<Path>, limits: ReadLimits) -> deft_reversi_book::Result<BookEditor> {
    let editor = BookEditor::load(path, limits)?;
    if let Some(m) = editor.migration_report() {
        eprintln!(
            "converted work v{}: positions={} registered_moves={} changed_position_values={} changed_candidate_values={} jobs_rebuilt={}; child values are now propagated; source file is unchanged until saved",
            m.source_version,
            m.positions,
            m.registered_moves,
            m.changed_position_values,
            m.changed_candidate_values,
            m.jobs_rebuilt
        );
    }
    Ok(editor)
}
fn run() -> std::result::Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        help();
        return Ok(());
    };
    if command == "--version" || command == "version" {
        println!(
            "deft_reversi_book {} search_revision={} endgame_pattern_ordering={}",
            env!("CARGO_PKG_VERSION"),
            deft_reversi_engine::SEARCH_SEMANTICS_REVISION,
            cfg!(feature = "endgame-pattern-ordering")
        );
        return Ok(());
    }
    if command == "--help" || command == "help" {
        help();
        return Ok(());
    }
    let allowed = [
        "model",
        "work",
        "book",
        "positions",
        "stages",
        "levels",
        "legacy-values",
        "schedule",
        "line-plies",
        "jobs",
        "propagation",
        "selectivity",
        "loss",
        "black-loss",
        "white-loss",
        "cumulative-loss",
        "leaf-loss",
        "new-weight",
        "deepen-weight",
        "max-plies",
        "threads",
        "tt-mib",
        "memory-mib",
        "read-mib",
        "read-positions",
        "read-candidates",
        "seconds",
        "nodes",
        "new-positions",
        "checkpoint-seconds",
        "legacy-main-mpc",
    ];
    let mut flags = BTreeMap::new();
    while let Some(flag) = args.next() {
        let k = flag
            .strip_prefix("--")
            .ok_or("expected --option")?
            .to_string();
        if !allowed.contains(&k.as_str()) {
            return Err(format!("unknown option --{k}").into());
        }
        let v = if k == "legacy-main-mpc" {
            "true".to_string()
        } else {
            args.next().ok_or("option value required")?
        };
        if flags.insert(k, v).is_some() {
            return Err("duplicate option".into());
        }
    }
    if matches!(
        command.as_str(),
        "build" | "extend" | "reanalyze" | "resume" | "leaves" | "export" | "repair"
    ) && let (Some(work), Some(book)) = (flags.get("work"), flags.get("book"))
        && destination(work)? == destination(book)?
    {
        return Err("--work and --book must be different files".into());
    }
    let required = |k: &str| {
        flags
            .get(k)
            .map(String::as_str)
            .ok_or_else(|| format!("--{k} is required"))
    };
    let num = |k: &str, default: u64| -> std::result::Result<u64, Box<dyn Error>> {
        Ok(flags
            .get(k)
            .map(|v| v.parse())
            .transpose()?
            .unwrap_or(default))
    };
    let mib = |n: u64| n.checked_mul(1024 * 1024).ok_or("memory size overflow");
    if flags.contains_key("propagation") {
        return Err(
            "--propagation was removed: registered child values are always propagated".into(),
        );
    }
    let limits = ReadLimits {
        max_bytes: mib(num("read-mib", 512)?)?,
        max_positions: num("read-positions", 2_000_000)?,
        max_candidates: num("read-candidates", 32_000_000)?,
    };
    if matches!(command.as_str(), "inspect" | "verify" | "export" | "repair") {
        let allowed = [
            "work",
            "book",
            "read-mib",
            "read-positions",
            "read-candidates",
        ];
        if flags.keys().any(|key| {
            !allowed.contains(&key.as_str())
                && !(command == "repair" && key == "propagation")
                && !(command == "inspect" && key == "positions")
        }) {
            return Err("option is not applicable to this command".into());
        }
    }
    if matches!(command.as_str(), "inspect" | "verify") {
        if flags.contains_key("work") == flags.contains_key("book") {
            return Err("specify exactly one of --work or --book".into());
        }
        if let Some(path) = flags.get("work") {
            let e = load_work(path, limits)?;
            e.validate()?;
            if command == "inspect" {
                let identity: String = e.identity().iter().map(|b| format!("{b:02x}")).collect();
                println!("identity={identity} propagation={:?}", "child-values");
                println!("generation={:?}", e.generation_config());
                println!("profiles={:#?}", e.analysis_profiles().collect::<Vec<_>>());
                println!("runs={:#?}", e.generation_runs());
                println!("{:#?}", e.stats());
                if let Some(path) = flags.get("positions") {
                    let snapshot = e.export()?;
                    for root in read_positions(path)? {
                        inspect_position(&snapshot, Some(&e), root.board);
                    }
                }
            }
            println!(
                "workspace positions={} revision={} managed_bytes={}",
                e.len(),
                e.revision(),
                e.estimated_bytes()
            );
        } else {
            let b = Book::load(required("book")?, limits)?;
            println!("{:?}", b.stats());
            if command == "inspect" {
                println!(
                    "profiles={:#?}\nruns={:#?}",
                    b.analysis_profiles().collect::<Vec<_>>(),
                    b.generation_runs()
                );
                if let Some(path) = flags.get("positions") {
                    for root in read_positions(path)? {
                        inspect_position(&b, None, root.board);
                    }
                }
            }
        }
        return Ok(());
    }
    if flags.contains_key("propagation") {
        return Err(
            "--propagation was removed: registered child values are always propagated".into(),
        );
    }
    if command == "repair" {
        let mut editor = load_work(required("work")?, limits)?;
        editor.repair()?;
        editor.save(required("work")?)?;
        if let Some(path) = flags.get("book") {
            editor.export()?.save(path)?;
        }
        return Ok(());
    }
    if command == "export" {
        let e = load_work(required("work")?, limits)?;
        e.export()?.save(required("book")?)?;
        return Ok(());
    }
    if !matches!(
        command.as_str(),
        "build" | "extend" | "reanalyze" | "resume" | "leaves"
    ) {
        return Err("unknown command".into());
    }
    if !["seconds", "nodes", "new-positions"]
        .iter()
        .any(|k| flags.contains_key(*k))
    {
        return Err("explicit --seconds, --nodes or --new-positions budget required".into());
    }
    let work = required("work")?;
    let mut engine = Engine::new(
        Model::from_file(required("model")?)?,
        EngineOptions::default()
            .threads(usize::try_from(num("threads", 1)?)?)
            .tt_mib(usize::try_from(num("tt-mib", 256)?)?),
    )?;
    if flags.contains_key("legacy-main-mpc") {
        engine.set_legacy_main_mpc()?;
    }
    let mut editor = if command == "build" {
        if Path::new(work).exists() {
            return Err("workspace already exists; use resume or a new path".into());
        }
        BookEditor::for_engine(&engine)?
    } else {
        load_work(work, limits)?
    };
    let previous_roots = editor.generation_starts();
    let previous_config = editor.generation_config().cloned();
    if command == "reanalyze" {
        editor.retarget(&engine)?;
    }
    if editor.identity() == analysis_identity(&engine)? {
        editor.set_model_label(required("model")?.to_string())?;
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(std::env::current_exe()?)?;
        let build = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        editor.set_build_label(format!("host-executable-sha256/{build}"))?;
    }
    let config_flags = [
        "stages",
        "levels",
        "legacy-values",
        "schedule",
        "line-plies",
        "jobs",
        "propagation",
        "selectivity",
        "loss",
        "black-loss",
        "white-loss",
        "cumulative-loss",
        "leaf-loss",
        "new-weight",
        "deepen-weight",
        "max-plies",
        "memory-mib",
        "checkpoint-seconds",
        "positions",
    ];
    if command == "resume" && config_flags.iter().any(|f| flags.contains_key(*f)) {
        return Err("resume retains stored settings; use extend/reanalyze to change them".into());
    }
    if command != "resume" {
        let mut config = previous_config.unwrap_or_default();
        config.unregistered_only = command == "leaves";
        config.line_plies = u8::try_from(num("line-plies", u64::from(config.line_plies))?)?;
        config.max_parallel_jobs =
            u16::try_from(num("jobs", u64::from(config.max_parallel_jobs))?)?;
        if let Some(value) = flags.get("schedule") {
            config.schedule = match value.as_str() {
                "breadth" => SchedulePolicy::Breadth,
                "frontier" | "promising" => SchedulePolicy::Promising,
                _ => return Err("schedule must be breadth or frontier".into()),
            };
        }
        if flags.contains_key("levels")
            && (flags.contains_key("stages") || flags.contains_key("selectivity"))
        {
            return Err("--levels conflicts with --stages/--selectivity".into());
        }
        if let Some(levels) = flags.get("levels") {
            config.analysis = AnalysisSchedule::Levels(
                levels
                    .split(',')
                    .map(str::parse)
                    .collect::<std::result::Result<_, _>>()?,
            );
        }
        if let Some(s) = flags.get("stages") {
            let sel = match num("selectivity", 95)? {
                68 => Selectivity::Percent68,
                77 => Selectivity::Percent77,
                85 => Selectivity::Percent85,
                95 => Selectivity::Percent95,
                98 => Selectivity::Percent98,
                99 => Selectivity::Percent99,
                100 => Selectivity::Full,
                _ => return Err("selectivity must be 68/77/85/95/98/99/100".into()),
            };
            config.analysis = AnalysisSchedule::Stages(
                s.split(',')
                    .map(|d| {
                        Ok(Stage {
                            depth: d.parse()?,
                            selectivity: sel,
                        })
                    })
                    .collect::<std::result::Result<Vec<_>, Box<dyn Error>>>()?,
            );
        } else if flags.contains_key("selectivity") {
            return Err("--selectivity requires --stages".into());
        }
        if let Some(v) = flags.get("black-loss") {
            config.black_loss = Some(v.parse()?);
        }
        if let Some(v) = flags.get("white-loss") {
            config.white_loss = Some(v.parse()?);
        }
        if let Some(v) = flags.get("cumulative-loss") {
            config.cumulative_loss = if v == "none" { None } else { Some(v.parse()?) };
        }
        if let Some(v) = flags.get("leaf-loss") {
            config.leaf_loss = if v == "none" { None } else { Some(v.parse()?) };
        }
        config.new_job_weight = u8::try_from(num("new-weight", u64::from(config.new_job_weight))?)?;
        config.deepen_job_weight =
            u8::try_from(num("deepen-weight", u64::from(config.deepen_job_weight))?)?;
        config.expansion_loss = u8::try_from(num("loss", u64::from(config.expansion_loss))?)?;
        config.max_plies = u8::try_from(num("max-plies", u64::from(config.max_plies))?)?;
        config.max_memory_bytes = mib(num("memory-mib", config.max_memory_bytes / (1024 * 1024))?)?;
        config.checkpoint_interval = Duration::from_secs(num(
            "checkpoint-seconds",
            config.checkpoint_interval.as_secs(),
        )?);
        config.reuse_completed = !matches!(command.as_str(), "reanalyze" | "leaves");
        if let Some(value) = flags.get("legacy-values") {
            config.allow_imported = match value.as_str() {
                "reject" => false,
                "recorded" => true,
                _ => return Err("legacy-values must be reject or recorded".into()),
            };
        }
        let roots = if let Some(p) = flags.get("positions") {
            read_positions(p)?
        } else {
            let r = previous_roots;
            if r.is_empty() {
                vec![GenerationRoot {
                    board: Board::new(),
                    side: BookSide::Black,
                }]
            } else {
                r
            }
        };
        editor.configure_generation_roots(&roots, config)?;
    }
    let budgets = GenerationLimits {
        time: flags
            .get("seconds")
            .map(|s| s.parse().map(Duration::from_secs))
            .transpose()?,
        nodes: flags.get("nodes").map(|s| s.parse()).transpose()?,
        new_positions: flags.get("new-positions").map(|s| s.parse()).transpose()?,
    };
    let stop = StopHandle::new();
    let handler = stop.clone();
    ctrlc::set_handler(move || handler.stop())?;
    let result = editor.generate_with_checkpoint(&mut engine, budgets, stop, |e, r| {
        e.save(work)?;
        eprintln!(
            "checkpoint stages={} new={} nodes={} elapsed={:.3}s",
            r.completed_stages,
            r.new_positions,
            r.nodes,
            r.elapsed.as_secs_f64()
        );
        Ok(())
    });
    let report = match result {
        Ok(r) => r,
        Err(error) => {
            editor.save(work)?;
            return Err(error.into());
        }
    };
    if let Some(path) = flags.get("book") {
        editor.export()?.save(path)?;
    }
    println!(
        "reason={:?} stages={} new={} nodes={} elapsed={:.3}s positions={}",
        report.reason,
        report.completed_stages,
        report.new_positions,
        report.nodes,
        report.elapsed.as_secs_f64(),
        editor.len()
    );
    let p = &report.profile;
    println!(
        "profile searches={} parallel={} engine={:.6}s resources={:.6}s expand={:.6}s schedule={:.6}s plan={:.6}s reachability={:.6}s publish={:.6}s checkpoint={:.6}s reach_hits={} reach_misses={}",
        p.move_searches,
        p.max_parallel_jobs,
        p.engine_wall.as_secs_f64(),
        p.resources_wall.as_secs_f64(),
        p.expansion_wall.as_secs_f64(),
        p.scheduling_wall.as_secs_f64(),
        p.planning_wall.as_secs_f64(),
        p.reachability_wall.as_secs_f64(),
        p.publish_wall.as_secs_f64(),
        p.checkpoint_wall.as_secs_f64(),
        p.reachability_hits,
        p.reachability_misses
    );
    Ok(())
}
fn inspect_value(label: &str, value: Option<Evaluation>) {
    if let Some(value) = value {
        println!(
            "  {label} score={:?} source={:?}",
            value.result(),
            value.source()
        );
    }
}
fn inspect_position(book: &Book, editor: Option<&BookEditor>, board: Board) {
    println!(
        "position player={:016x} opponent={:016x}",
        board.player_bits(),
        board.opponent_bits()
    );
    let Some(entry) = book.probe(board) else {
        println!("  missing");
        return;
    };
    inspect_value("position-value", entry.value());
    inspect_value("position-final_score", entry.final_score());
    for c in entry.candidates() {
        println!(" move={:?} registered={}", c.action, c.registered);
        inspect_value("effective", c.value);
        inspect_value("certificate", c.final_score);
        if let Some(editor) = editor {
            let ev = editor
                .move_values(board, c.action)
                .expect("snapshot/editor candidate match");
            println!("  source={:?}", ev.selected_from);
            inspect_value("direct", ev.direct);
            inspect_value("frontier", ev.move_search);
            inspect_value("manual", ev.manual);
            inspect_value("propagated", ev.child);
            inspect_value("propagated-certificate", ev.child_final_score);
        }
    }
}
fn read_positions(path: &str) -> std::result::Result<Vec<GenerationRoot>, Box<dyn Error>> {
    let mut result = Vec::new();
    for (index, line) in std::fs::read_to_string(path)?.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let mut parts = l.split_whitespace();
        let cells = parts.next().ok_or("board required")?;
        let turn = parts.next().ok_or("turn required")?.trim_end_matches(';');
        if cells.len() != 64 {
            return Err(format!("invalid board on line {}", index + 1).into());
        }
        let (mut p, mut o) = (0, 0);
        for (i, b) in cells.bytes().enumerate() {
            match b {
                b'X' | b'x' | b'*' => p |= 1 << i,
                b'O' | b'o' => o |= 1 << i,
                b'-' | b'.' => {}
                _ => return Err("invalid cell".into()),
            }
        }
        let (p, o) = match turn {
            "X" | "x" | "*" => (p, o),
            "O" | "o" => (o, p),
            _ => return Err("invalid side".into()),
        };
        result.push(GenerationRoot {
            board: Board::from_bits(p, o).ok_or("overlapping board")?,
            side: if matches!(turn, "X" | "x" | "*") {
                BookSide::Black
            } else {
                BookSide::White
            },
        });
    }
    if result.is_empty() {
        return Err("no input positions".into());
    }
    Ok(result)
}
fn destination(path: &str) -> std::io::Result<std::path::PathBuf> {
    let path = Path::new(path);
    if path.exists() {
        return std::fs::canonicalize(path);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(
        std::fs::canonicalize(parent)?.join(path.file_name().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "output filename required")
        })?),
    )
}
fn help() {
    println!(
        "book build|extend|reanalyze|leaves|resume --model MODEL --work FILE [--book FILE]\n  --seconds N | --nodes N | --new-positions N  (at least one required)\n  --levels 4,8,12 (alternative to --stages/--selectivity)\n  --positions OBF --stages 12,16,20 --selectivity 95 --loss 2 --max-plies 24\n  --legacy-values reject|recorded\n  --line-plies 8 --jobs 0 (automatic)\n  --schedule breadth|frontier\n  --cumulative-loss N|none --leaf-loss N|none\n  --black-loss N --white-loss N --new-weight 3 --deepen-weight 1
  --threads 1 --tt-mib 256 --memory-mib 512 --checkpoint-seconds 60\n  --memory-mib limits estimated book data and job storage; engine/TT and temporary I/O memory are separate\n  --legacy-main-mpc\nbook repair --work FILE [--book FILE]\nbook export --work FILE --book FILE\nbook inspect|verify --work FILE | --book FILE\n  inspect --positions OBF (candidate evidence and completed modes)\n  --read-mib 512 --read-positions 2000000 --read-candidates 32000000
  (read/allocation bounds; resume retains stored generation settings)"
    );
}
