use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot, Semaphore};
use tracing::{info, warn};

use crate::backend::{self, Backend, BackendError};
use crate::bus::{self, BusClient};
use crate::config::Config;
use crate::transcode;

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("track too short: {duration_secs:.3}s <= {min_duration_secs:.3}s")]
    TrackTooShort {
        duration_secs: f64,
        min_duration_secs: f64,
    },
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
    #[error("backend: {0}")]
    Backend(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub struct PipelineOutput {
    pub duration_secs: f64,
}

pub struct Pipeline {
    tx: mpsc::Sender<PipelineJob>,
}

struct PipelineJob {
    source: PathBuf,
    filename: String,
    reply: oneshot::Sender<Result<PipelineOutput, PipelineError>>,
}

impl Pipeline {
    pub fn start(config: Arc<Config>, backend: Arc<Backend>, bus: BusClient) -> Self {
        let (tx, rx) = mpsc::channel::<PipelineJob>(8192);
        let writer = Arc::new(WriterPool::start(config.clone(), backend.clone()));
        tokio::spawn(dispatcher_loop(config, backend, writer, bus, rx));
        Self { tx }
    }

    /// Submit a single source file. Pipeline takes ownership of `source` —
    /// it deletes the file on disk when done (success OR failure).
    pub async fn submit(
        &self,
        source: PathBuf,
        filename: String,
    ) -> Result<PipelineOutput, PipelineError> {
        let (tx, rx) = oneshot::channel();
        if self
            .tx
            .send(PipelineJob {
                source: source.clone(),
                filename,
                reply: tx,
            })
            .await
            .is_err()
        {
            let _ = tokio::fs::remove_file(&source).await;
            return Err(PipelineError::Internal("pipeline closed".into()));
        }
        rx.await
            .unwrap_or_else(|_| Err(PipelineError::Internal("dispatcher dropped reply".into())))
    }
}

// ──────────────────────────────────────────────────────────────────────
// dispatcher: batches jobs and runs them through ffmpeg
// ──────────────────────────────────────────────────────────────────────

async fn dispatcher_loop(
    config: Arc<Config>,
    backend: Arc<Backend>,
    writer: Arc<WriterPool>,
    bus: BusClient,
    mut rx: mpsc::Receiver<PipelineJob>,
) {
    let sem = Arc::new(Semaphore::new(config.max_transcodes.max(1)));
    let batch_size = config.transcode_batch_size.max(1);
    let wait = Duration::from_millis(config.transcode_batch_wait_ms);

    loop {
        let Some(first) = rx.recv().await else {
            return;
        };
        let mut batch = vec![first];

        if batch_size > 1 {
            let deadline = tokio::time::Instant::now() + wait;
            while batch.len() < batch_size {
                tokio::select! {
                    biased;
                    msg = rx.recv() => match msg {
                        Some(job) => batch.push(job),
                        None => break,
                    },
                    _ = tokio::time::sleep_until(deadline) => break,
                }
            }
        }

        let permit = match sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return,
        };
        let cfg = config.clone();
        let bk = backend.clone();
        let wp = writer.clone();
        let bs = bus.clone();
        tokio::spawn(async move {
            run_batch(cfg, bk, wp, bs, batch).await;
            drop(permit);
        });
    }
}

async fn run_batch(
    config: Arc<Config>,
    backend: Arc<Backend>,
    writer: Arc<WriterPool>,
    bus: BusClient,
    jobs: Vec<PipelineJob>,
) {
    // 1. Probe durations in parallel; reject too-short tracks immediately.
    let durations = futures::future::join_all(
        jobs.iter()
            .map(|j| transcode::probe_duration(&j.source, &config.ffprobe_bin)),
    )
    .await;

    let mut accepted: Vec<(PipelineJob, f64, (PathBuf, PathBuf))> = Vec::with_capacity(jobs.len());
    for (job, dur) in jobs.into_iter().zip(durations) {
        let secs = dur.unwrap_or(0.0);
        if secs > 0.0 && secs <= transcode::MIN_UPLOAD_DURATION_SECS {
            let _ = tokio::fs::remove_file(&job.source).await;
            let _ = job.reply.send(Err(PipelineError::TrackTooShort {
                duration_secs: secs,
                min_duration_secs: transcode::MIN_UPLOAD_DURATION_SECS,
            }));
            continue;
        }
        let outs = transcode::stage_outputs(&config.result_path(), &job.filename);
        accepted.push((job, secs, outs));
    }
    if accepted.is_empty() {
        return;
    }

    // 2. ffmpeg: try multi-input batch first; on failure, fall back per-file.
    let started = Instant::now();
    let inputs: Vec<&Path> = accepted.iter().map(|(j, _, _)| j.source.as_path()).collect();
    let outputs: Vec<(PathBuf, PathBuf)> =
        accepted.iter().map(|(_, _, o)| o.clone()).collect();

    let batch_res = transcode::run_ffmpeg_batch(&config.ffmpeg_bin, &inputs, &outputs).await;

    let n = accepted.len();
    match batch_res {
        Ok(()) => {
            info!(
                "[batch] ffmpeg ok n={} {}ms",
                n,
                started.elapsed().as_millis()
            );
            for (job, secs, (hq, sq)) in accepted {
                spawn_commit(&backend, &writer, &config, &bus, job, secs, hq, sq);
            }
        }
        Err(err) if n > 1 => {
            warn!("[batch] ffmpeg failed n={n}: {err}; retrying per-file");
            for (job, secs, (hq, sq)) in accepted {
                let cfg = config.clone();
                let bk = backend.clone();
                let wp = writer.clone();
                let bs = bus.clone();
                tokio::spawn(async move {
                    let single =
                        transcode::run_ffmpeg_batch(&cfg.ffmpeg_bin, &[job.source.as_path()], &[(hq.clone(), sq.clone())])
                            .await;
                    match single {
                        Ok(()) => spawn_commit(&bk, &wp, &cfg, &bs, job, secs, hq, sq),
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&job.source).await;
                            warn!("[batch] single ffmpeg failed for {}: {e}", job.filename);
                            let _ = job.reply.send(Err(PipelineError::Ffmpeg(e.to_string())));
                        }
                    }
                });
            }
        }
        Err(err) => {
            // n == 1: clean up and report
            for (job, _, _) in accepted {
                let _ = tokio::fs::remove_file(&job.source).await;
                let _ = job.reply.send(Err(PipelineError::Ffmpeg(err.to_string())));
            }
        }
    }
}

fn spawn_commit(
    backend: &Arc<Backend>,
    writer: &Arc<WriterPool>,
    config: &Arc<Config>,
    bus: &BusClient,
    job: PipelineJob,
    duration_secs: f64,
    hq: PathBuf,
    sq: PathBuf,
) {
    let bk = backend.clone();
    let wp = writer.clone();
    let cfg = config.clone();
    let bs = bus.clone();
    tokio::spawn(async move {
        let res = commit_pair(&bk, &wp, &cfg, &job.filename, hq, sq).await;
        let _ = tokio::fs::remove_file(&job.source).await;
        if res.is_ok() {
            publish_uploaded(&bs, &cfg, &job.filename);
        }
        let _ = job.reply.send(res.map(|()| PipelineOutput { duration_secs }));
    });
}

fn publish_uploaded(bus: &BusClient, config: &Config, filename: &str) {
    if !bus.enabled() || config.event_base_url.is_empty() {
        return;
    }
    let Some(sc_track_id) = bus::sc_track_id_from_filename(filename) else {
        return;
    };
    let storage_url = format!("{}/redirect/hq/{}.ogg", config.event_base_url, filename);
    bus.publish_track_uploaded(sc_track_id, storage_url);
}

async fn commit_pair(
    _backend: &Arc<Backend>,
    writer: &Arc<WriterPool>,
    _config: &Arc<Config>,
    filename: &str,
    hq_tmp: PathBuf,
    sq_tmp: PathBuf,
) -> Result<(), PipelineError> {
    let hq_key = backend::key_for("hq", filename);
    let sq_key = backend::key_for("sq", filename);

    let (hq_res, sq_res) = tokio::join!(
        writer.commit(hq_key, hq_tmp.clone(), filename.to_string(), "hq"),
        writer.commit(sq_key, sq_tmp.clone(), filename.to_string(), "sq"),
    );
    if let Err(e) = &hq_res {
        warn!("[commit] hq {filename} failed: {e}");
        let _ = tokio::fs::remove_file(&hq_tmp).await;
    }
    if let Err(e) = &sq_res {
        warn!("[commit] sq {filename} failed: {e}");
        let _ = tokio::fs::remove_file(&sq_tmp).await;
    }
    hq_res?;
    sq_res?;
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────
// writer pool: bounded parallel backend commits with retry
// ──────────────────────────────────────────────────────────────────────

struct WriterJob {
    key: String,
    src: PathBuf,
    filename: String,
    quality: &'static str,
    reply: oneshot::Sender<Result<(), PipelineError>>,
}

struct WriterPool {
    tx: mpsc::Sender<WriterJob>,
}

impl WriterPool {
    fn start(config: Arc<Config>, backend: Arc<Backend>) -> Self {
        let n = config.upload_concurrency.max(1);
        let (tx, rx) = mpsc::channel::<WriterJob>(8192);
        let rx = Arc::new(tokio::sync::Mutex::new(rx));
        for _ in 0..n {
            let rx = rx.clone();
            let bk = backend.clone();
            let cfg = config.clone();
            tokio::spawn(async move {
                loop {
                    let job = {
                        let mut g = rx.lock().await;
                        g.recv().await
                    };
                    let Some(job) = job else {
                        return;
                    };
                    let res = process_writer_job(&cfg, &bk, &job).await;
                    let _ = job.reply.send(res);
                }
            });
        }
        Self { tx }
    }

    async fn commit(
        &self,
        key: String,
        src: PathBuf,
        filename: String,
        quality: &'static str,
    ) -> Result<(), PipelineError> {
        let (tx, rx) = oneshot::channel();
        if self
            .tx
            .send(WriterJob {
                key,
                src,
                filename,
                quality,
                reply: tx,
            })
            .await
            .is_err()
        {
            return Err(PipelineError::Internal("writer pool closed".into()));
        }
        rx.await
            .unwrap_or_else(|_| Err(PipelineError::Internal("writer dropped reply".into())))
    }
}

async fn process_writer_job(
    cfg: &Config,
    backend: &Backend,
    job: &WriterJob,
) -> Result<(), PipelineError> {
    let mut attempt: u32 = 0;
    let max_attempts = cfg.upload_retries.saturating_add(1);
    loop {
        let res = backend
            .commit_transcode(&job.key, &job.src, &cfg.ffprobe_bin, &job.filename, job.quality)
            .await;
        match res {
            Ok(()) => return Ok(()),
            Err(BackendError::NotFound) => {
                return Err(PipelineError::Backend("source disappeared".into()));
            }
            Err(e) => {
                attempt += 1;
                if (attempt as usize) >= max_attempts {
                    let _ = tokio::fs::remove_file(&job.src).await;
                    return Err(PipelineError::Backend(e.to_string()));
                }
                let backoff_ms = cfg
                    .upload_retry_base_ms
                    .saturating_mul(1u64 << attempt.min(6));
                warn!(
                    "[writer] {} attempt {}/{} failed: {} (backoff {}ms)",
                    job.key, attempt, max_attempts, e, backoff_ms
                );
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
        }
    }
}
