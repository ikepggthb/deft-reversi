mod dataset;
mod loss;
mod nnue_train;
mod pattern_train;
mod patterns;
mod residual;
mod train_log;

use clap::{Parser, Subcommand};
use deft_reversi_engine::train_api::MpcConfig;
use deft_reversi_engine::train_api::{EngineFile, EvaluatorData, Metadata, NnueEvaluatorData};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "deft-reversi-train")]
#[command(about = "Training utilities for deft-reversi")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Patterns {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    PackNnue(PackNnueArgs),
    Repack(RepackArgs),
    SetMpc(SetMpcArgs),
    AnnotateResidual(residual::AnnotateResidualArgs),
    Train {
        #[command(subcommand)]
        kind: TrainKind,
    },
}

#[derive(clap::Args)]
struct PackNnueArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "nnue-trained")]
    eval_name: String,
    #[arg(long, default_value = "0")]
    eval_version: String,
    #[arg(long)]
    mpc: Option<PathBuf>,
}

#[derive(clap::Args)]
struct SetMpcArgs {
    #[arg(long = "eval")]
    eval_path: PathBuf,
    #[arg(long)]
    mpc: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

#[derive(clap::Args)]
struct RepackArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

#[derive(Subcommand)]
enum TrainKind {
    Pattern(pattern_train::PatternTrainArgs),
    Nnue(nnue_train::NnueTrainArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Patterns { args } => match patterns::run(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::PackNnue(args) => match pack_nnue(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::SetMpc(args) => match set_mpc(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::Repack(args) => match repack(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::AnnotateResidual(args) => match residual::run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::Train { kind } => {
            let result = match kind {
                TrainKind::Pattern(args) => pattern_train::run(args),
                TrainKind::Nnue(args) => nnue_train::run(args),
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

fn pack_nnue(args: PackNnueArgs) -> Result<(), String> {
    let input = fs::read_to_string(&args.input).map_err(|err| err.to_string())?;
    let data: NnueEvaluatorData = serde_json::from_str(&input).map_err(|err| err.to_string())?;
    let mpc = read_mpc_or_default(args.mpc.as_deref())?;
    let engine_file = EngineFile {
        format_version: 4,
        metadata: Metadata {
            eval_name: args.eval_name,
            eval_version: args.eval_version,
            ..Metadata::default()
        },
        evaluator: EvaluatorData::Nnue(data),
        mpc,
    };
    write_engine_file(&engine_file, &args.out)
}

fn repack(args: RepackArgs) -> Result<(), String> {
    let mut engine_file = read_engine_file(&args.input)?;
    engine_file.format_version = 4;
    write_engine_file(&engine_file, &args.out)
}

fn set_mpc(args: SetMpcArgs) -> Result<(), String> {
    let mut engine_file = read_engine_file(&args.eval_path)?;
    engine_file.mpc = read_mpc_or_default(Some(&args.mpc))?;
    write_engine_file(&engine_file, &args.out)
}

fn read_mpc_or_default(path: Option<&std::path::Path>) -> Result<MpcConfig, String> {
    let Some(path) = path else {
        return Ok(MpcConfig::default());
    };
    let input = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mpc: MpcConfig = serde_json::from_str(&input).map_err(|err| err.to_string())?;
    mpc.validate().map_err(|err| err.to_string())?;
    Ok(mpc)
}

fn read_engine_file(path: &std::path::Path) -> Result<EngineFile, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    match std::str::from_utf8(&bytes) {
        Ok(input) => EngineFile::read_string(input).map_err(|err| err.to_string()),
        Err(_) => EngineFile::from_bytes(&bytes).map_err(|err| err.to_string()),
    }
}

fn write_engine_file(engine_file: &EngineFile, path: &std::path::Path) -> Result<(), String> {
    if path.extension().is_some_and(|ext| ext == "json") {
        let json = serde_json::to_string_pretty(engine_file).map_err(|err| err.to_string())?;
        fs::write(path, json).map_err(|err| err.to_string())
    } else {
        engine_file
            .write_file(path.to_str().ok_or("output path is not valid UTF-8")?)
            .map_err(|err| err.to_string())
    }
}
