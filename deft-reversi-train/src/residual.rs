use crate::dataset::{PhaseRange, RD_MAGIC, RD_RECORD_SIZE};
use clap::Args;
use deft_reversi_engine::{evaluator_from_path, Board, Evaluator};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;

#[derive(Args, Clone)]
pub struct AnnotateResidualArgs {
    #[arg(long)]
    eval: PathBuf,
    #[arg(long)]
    data: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "12..63")]
    phase_range: PhaseRange,
    #[arg(long, default_value_t = 8)]
    verify_samples: usize,
}

pub fn run(args: AnnotateResidualArgs) -> Result<(), String> {
    let evaluator = evaluator_from_path(
        args.eval
            .to_str()
            .ok_or_else(|| format!("eval path is not valid UTF-8: {}", args.eval.display()))?,
    )
    .map_err(|err| err.to_string())?;
    let tasks = collect_tasks(&args.data, &args.out, args.phase_range)?;
    process_tasks_parallel(tasks, evaluator, args.verify_samples)?;
    Ok(())
}

#[derive(Clone)]
struct Task {
    phase: usize,
    split: &'static str,
    input: PathBuf,
    output: PathBuf,
}

fn collect_tasks(data: &Path, out: &Path, range: PhaseRange) -> Result<Vec<Task>, String> {
    let mut tasks = Vec::new();
    for phase in range.start..=range.end {
        for split in ["train", "valid"] {
            let input = data
                .join(format!("phase_{phase}"))
                .join(format!("{split}.rd"));
            if input.exists() {
                let output = out
                    .join(format!("phase_{phase}"))
                    .join(format!("{split}.rd"));
                tasks.push(Task {
                    phase,
                    split,
                    input,
                    output,
                });
            }
        }
    }
    if tasks.is_empty() {
        return Err(format!(
            "{}: no train/valid .rd files in phase range {}..{}",
            data.display(),
            range.start,
            range.end
        ));
    }
    Ok(tasks)
}

fn process_tasks_parallel(
    tasks: Vec<Task>,
    evaluator: Arc<dyn Evaluator>,
    verify_samples: usize,
) -> Result<(), String> {
    let tasks = Arc::new(tasks);
    let next = Arc::new(AtomicUsize::new(0));
    let error = Arc::new(Mutex::new(None::<String>));
    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(tasks.len().max(1));

    thread::scope(|scope| {
        for _ in 0..workers {
            let tasks = Arc::clone(&tasks);
            let next = Arc::clone(&next);
            let evaluator = Arc::clone(&evaluator);
            let error = Arc::clone(&error);
            scope.spawn(move || loop {
                if error.lock().unwrap().is_some() {
                    return;
                }
                let idx = next.fetch_add(1, Ordering::Relaxed);
                let Some(task) = tasks.get(idx) else {
                    return;
                };
                if let Err(err) = process_file(task, evaluator.as_ref(), verify_samples) {
                    *error.lock().unwrap() = Some(err.to_string());
                    return;
                }
            });
        }
    });

    let result = if let Some(message) = error.lock().unwrap().take() {
        Err(message)
    } else {
        Ok(())
    };
    result
}

fn process_file(task: &Task, evaluator: &dyn Evaluator, verify_samples: usize) -> io::Result<()> {
    if let Some(parent) = task.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut reader = BufReader::new(File::open(&task.input)?);
    let mut writer = BufWriter::new(File::create(&task.output)?);
    let mut header = [0u8; RD_MAGIC.len()];
    reader.read_exact(&mut header)?;
    if header != RD_MAGIC {
        return Err(invalid_data(format!(
            "{}: invalid rd header",
            task.input.display()
        )));
    }
    writer.write_all(RD_MAGIC)?;

    let mut count = 0u64;
    let mut clamped = 0u64;
    let mut verified = 0usize;
    loop {
        let mut rec = [0u8; RD_RECORD_SIZE];
        match reader.read_exact(&mut rec) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(err),
        }
        let own = u64::from_le_bytes(rec[0..8].try_into().unwrap());
        let opponent = u64::from_le_bytes(rec[8..16].try_into().unwrap());
        let value = i16::from_le_bytes(rec[16..18].try_into().unwrap());
        let board = Board {
            player: own,
            opponent,
        };
        let eval_score = evaluator.evaluate(&board);
        let raw_residual = i32::from(value) - eval_score;
        let residual = raw_residual.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        if raw_residual != i32::from(residual) {
            clamped += 1;
        }
        if verified < verify_samples {
            let restored = i32::from(residual) + eval_score;
            let expected = i32::from(value).clamp(
                eval_score + i32::from(i16::MIN),
                eval_score + i32::from(i16::MAX),
            );
            if restored != expected {
                return Err(invalid_data(format!(
                    "{}: residual verification failed record={} restored={} expected={}",
                    task.input.display(),
                    count,
                    restored,
                    expected
                )));
            }
            verified += 1;
        }
        rec[16..18].copy_from_slice(&residual.to_le_bytes());
        writer.write_all(&rec)?;
        count += 1;
    }
    writer.flush()?;
    if clamped > 0 {
        eprintln!(
            "warning: phase_{} {} clamped {} residuals",
            task.phase, task.split, clamped
        );
    }
    println!(
        "annotated phase_{} {} records={} verified={} -> {}",
        task.phase,
        task.split,
        count,
        verified,
        task.output.display()
    );
    Ok(())
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
