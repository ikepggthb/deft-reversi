use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct BatchMetrics {
    pub loss: f32,
    pub disc_mae: f32,
    pub n_samples: usize,
}

pub struct EpochSummary {
    pub epoch: usize,
    pub total_epochs: usize,
    pub train_loss_sum: f64,
    pub train_disc_error_sum: f64,
    pub train_samples: u64,
    pub valid_loss: f64,
    pub valid_disc_mae: f64,
    pub step: u64,
}

pub struct ProgressLogger {
    epoch: usize,
    total_epochs: usize,
    total_batches: u64,
    started: Instant,
    last_logged_at: Instant,
    last_logged_samples: u64,
    metrics: Option<MetricsWriter>,
}

impl ProgressLogger {
    pub fn new(
        epoch: usize,
        total_epochs: usize,
        total_batches: u64,
        log_dir: Option<&PathBuf>,
        evaluator_kind: &'static str,
    ) -> io::Result<Self> {
        let now = Instant::now();
        Ok(Self {
            epoch,
            total_epochs,
            total_batches: total_batches.max(1),
            started: now,
            last_logged_at: now,
            last_logged_samples: 0,
            metrics: log_dir
                .map(|dir| MetricsWriter::open(dir, evaluator_kind))
                .transpose()?,
        })
    }

    pub fn log_train(
        &mut self,
        step: u64,
        samples_in_epoch: u64,
        seen_samples: u64,
        loss_sum: f64,
        disc_error_sum: f64,
        last: &BatchMetrics,
        lr: f32,
    ) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.started);
        let interval_elapsed = now.duration_since(self.last_logged_at);
        let interval_samples = samples_in_epoch.saturating_sub(self.last_logged_samples);
        let speed = interval_samples as f64 / interval_elapsed.as_secs_f64().max(1e-9);
        let batch_idx = step.min(self.total_batches);
        let batches_per_sec = batch_idx as f64 / elapsed.as_secs_f64().max(1e-9);
        let remaining_batches = self.total_batches.saturating_sub(batch_idx);
        let eta_epoch = seconds_from_f64(remaining_batches as f64 / batches_per_sec.max(1e-9));
        let eta_total = eta_epoch
            + seconds_from_f64(
                self.total_batches as f64 * self.total_epochs.saturating_sub(self.epoch + 1) as f64
                    / batches_per_sec.max(1e-9),
            );
        let avg_loss = loss_sum / samples_in_epoch.max(1) as f64;
        let disc_mae = disc_error_sum / samples_in_epoch.max(1) as f64;

        eprintln!(
            "epoch={:03}/{:03} train batch={}/{} step={} samples={} total_samples={} avg_loss={:.6} disc_mae={:.3} last_loss={:.6} last_disc_mae={:.3} lr={:.6} speed={:.1}/s elapsed={} eta_epoch={} eta_total={}",
            self.epoch + 1,
            self.total_epochs,
            batch_idx,
            self.total_batches,
            step,
            samples_in_epoch,
            seen_samples,
            avg_loss,
            disc_mae,
            last.loss,
            last.disc_mae,
            lr,
            speed,
            format_duration(elapsed),
            format_duration(eta_epoch),
            format_duration(eta_total),
        );

        let event = TrainLogEvent {
            kind: "train",
            evaluator: self.metrics.as_ref().map(|writer| writer.evaluator_kind),
            epoch: self.epoch + 1,
            total_epochs: self.total_epochs,
            batch: batch_idx,
            total_batches: self.total_batches,
            step,
            samples: samples_in_epoch,
            total_samples: seen_samples,
            avg_loss,
            disc_mae,
            last_loss: Some(last.loss as f64),
            last_disc_mae: Some(last.disc_mae as f64),
            valid_loss: None,
            valid_disc_mae: None,
            lr: Some(lr as f64),
            speed: Some(speed),
            elapsed_seconds: elapsed.as_secs_f64(),
            eta_epoch_seconds: Some(eta_epoch.as_secs_f64()),
            eta_total_seconds: Some(eta_total.as_secs_f64()),
        };
        self.write_event(&event);
        self.last_logged_at = now;
        self.last_logged_samples = samples_in_epoch;
    }

    pub fn log_epoch_summary(&mut self, summary: EpochSummary) {
        let train_loss = summary.train_loss_sum / summary.train_samples.max(1) as f64;
        let train_disc_mae = summary.train_disc_error_sum / summary.train_samples.max(1) as f64;
        eprintln!(
            "epoch={:03}/{:03} summary train_loss={:.6} train_disc_mae={:.3} valid_loss={:.6} valid_disc_mae={:.3} samples={} steps={}",
            summary.epoch + 1,
            summary.total_epochs,
            train_loss,
            train_disc_mae,
            summary.valid_loss,
            summary.valid_disc_mae,
            summary.train_samples,
            summary.step,
        );

        let event = TrainLogEvent {
            kind: "summary",
            evaluator: self.metrics.as_ref().map(|writer| writer.evaluator_kind),
            epoch: summary.epoch + 1,
            total_epochs: summary.total_epochs,
            batch: self.total_batches,
            total_batches: self.total_batches,
            step: summary.step,
            samples: summary.train_samples,
            total_samples: summary.train_samples,
            avg_loss: train_loss,
            disc_mae: train_disc_mae,
            last_loss: None,
            last_disc_mae: None,
            valid_loss: Some(summary.valid_loss),
            valid_disc_mae: Some(summary.valid_disc_mae),
            lr: None,
            speed: None,
            elapsed_seconds: self.started.elapsed().as_secs_f64(),
            eta_epoch_seconds: Some(0.0),
            eta_total_seconds: None,
        };
        self.write_event(&event);
    }

    fn write_event(&mut self, event: &TrainLogEvent) {
        if let Some(writer) = &mut self.metrics {
            if let Err(err) = writer.write(event) {
                eprintln!("warning: failed to write training metrics: {err}");
            }
        }
    }
}

pub fn total_batches(n_samples: u64, batch_size: usize) -> u64 {
    let batch_size = batch_size.max(1) as u64;
    n_samples.saturating_add(batch_size - 1) / batch_size
}

struct MetricsWriter {
    evaluator_kind: &'static str,
    csv: File,
    jsonl: File,
}

impl MetricsWriter {
    fn open(dir: &PathBuf, evaluator_kind: &'static str) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let csv_path = dir.join("metrics.csv");
        let csv_exists = csv_path.exists() && csv_path.metadata()?.len() > 0;
        let mut csv = OpenOptions::new()
            .create(true)
            .append(true)
            .open(csv_path)?;
        if !csv_exists {
            writeln!(
                csv,
                "kind,evaluator,epoch,total_epochs,batch,total_batches,step,samples,total_samples,avg_loss,disc_mae,last_loss,last_disc_mae,valid_loss,valid_disc_mae,lr,speed,elapsed_seconds,eta_epoch_seconds,eta_total_seconds"
            )?;
        }
        let jsonl = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("metrics.jsonl"))?;
        Ok(Self {
            evaluator_kind,
            csv,
            jsonl,
        })
    }

    fn write(&mut self, event: &TrainLogEvent) -> io::Result<()> {
        writeln!(
            self.csv,
            "{},{},{},{},{},{},{},{},{},{:.8},{:.8},{},{},{},{},{},{},{:.3},{},{}",
            event.kind,
            self.evaluator_kind,
            event.epoch,
            event.total_epochs,
            event.batch,
            event.total_batches,
            event.step,
            event.samples,
            event.total_samples,
            event.avg_loss,
            event.disc_mae,
            optional_f64(event.last_loss),
            optional_f64(event.last_disc_mae),
            optional_f64(event.valid_loss),
            optional_f64(event.valid_disc_mae),
            optional_f64(event.lr),
            optional_f64(event.speed),
            event.elapsed_seconds,
            optional_f64(event.eta_epoch_seconds),
            optional_f64(event.eta_total_seconds),
        )?;
        serde_json::to_writer(&mut self.jsonl, event)?;
        writeln!(self.jsonl)?;
        self.csv.flush()?;
        self.jsonl.flush()
    }
}

#[derive(Serialize)]
struct TrainLogEvent {
    kind: &'static str,
    evaluator: Option<&'static str>,
    epoch: usize,
    total_epochs: usize,
    batch: u64,
    total_batches: u64,
    step: u64,
    samples: u64,
    total_samples: u64,
    avg_loss: f64,
    disc_mae: f64,
    last_loss: Option<f64>,
    last_disc_mae: Option<f64>,
    valid_loss: Option<f64>,
    valid_disc_mae: Option<f64>,
    lr: Option<f64>,
    speed: Option<f64>,
    elapsed_seconds: f64,
    eta_epoch_seconds: Option<f64>,
    eta_total_seconds: Option<f64>,
}

fn optional_f64(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.8}")).unwrap_or_default()
}

fn seconds_from_f64(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds.max(0.0).min(u64::MAX as f64))
}

fn format_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}
