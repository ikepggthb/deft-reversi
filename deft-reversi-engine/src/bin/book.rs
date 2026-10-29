//! Offline book adapter. No application history, file policy, or signals in search.
use deft_reversi_engine::{Board, Engine, EngineOptions, Selectivity, StopHandle, book::*};
use std::{collections::BTreeMap, error::Error, path::Path, time::Duration};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> std::result::Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        help();
        return Ok(());
    };
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
        "selectivity",
        "loss",
        "black-loss",
        "white-loss",
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
        "build" | "extend" | "reanalyze" | "resume" | "export"
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
    let limits = ReadLimits {
        max_bytes: mib(num("read-mib", 512)?)?,
        max_positions: num("read-positions", 2_000_000)?,
        max_candidates: num("read-candidates", 32_000_000)?,
    };
    if matches!(command.as_str(), "inspect" | "verify") {
        if let Some(path) = flags.get("work") {
            let e = BookEditor::load(path, limits)?;
            e.validate()?;
            println!(
                "workspace positions={} revision={} managed_bytes={}",
                e.len(),
                e.revision(),
                e.estimated_bytes()
            );
        } else {
            let b = Book::load(required("book")?, limits)?;
            println!("{:?}", b.stats());
        }
        return Ok(());
    }
    if command == "export" {
        let e = BookEditor::load(required("work")?, limits)?;
        e.export()?.save(required("book")?)?;
        return Ok(());
    }
    if !matches!(
        command.as_str(),
        "build" | "extend" | "reanalyze" | "resume"
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
    let mut engine = Engine::from_file(
        required("model")?,
        EngineOptions {
            threads: usize::try_from(num("threads", 1)?)?,
            tt_mib: usize::try_from(num("tt-mib", 256)?)?,
        },
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
        BookEditor::load(work, limits)?
    };
    let previous_roots = editor.generation_starts();
    let previous_config = editor.generation_config().cloned();
    if command == "reanalyze" {
        editor.retarget(&engine)?;
    }
    let config_flags = [
        "stages",
        "selectivity",
        "loss",
        "black-loss",
        "white-loss",
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
            config.stages = s
                .split(',')
                .map(|d| {
                    Ok(Stage {
                        depth: d.parse()?,
                        selectivity: sel,
                    })
                })
                .collect::<std::result::Result<Vec<_>, Box<dyn Error>>>()?;
        } else if flags.contains_key("selectivity") {
            return Err("--selectivity requires --stages".into());
        }
        if let Some(v) = flags.get("black-loss") {
            config.black_loss = Some(v.parse()?);
        }
        if let Some(v) = flags.get("white-loss") {
            config.white_loss = Some(v.parse()?);
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
        if command == "reanalyze" {
            config.reuse_completed = false;
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
    Ok(())
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
        "book build|extend|reanalyze|resume --model MODEL --work FILE [--book FILE]\n  --seconds N | --nodes N | --new-positions N  (at least one required)\n  --positions OBF --stages 12,16,20 --selectivity 95 --loss 2 --max-plies 24\n  --black-loss N --white-loss N --new-weight 3 --deepen-weight 1
  --threads 1 --tt-mib 256 --memory-mib 512 --checkpoint-seconds 60\n  --legacy-main-mpc\nbook export --work FILE --book FILE\nbook inspect|verify --work FILE | --book FILE\n  --read-mib 512 --read-positions 2000000 --read-candidates 32000000
  (read/allocation bounds; resume retains stored generation settings)"
    );
}
