//! Download manager (SPEC §7.3): a background loop that runs queued jobs with bounded concurrency,
//! persists progress, verifies results, retries with backoff, and supports pause/cancel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use tokio::sync::{Notify, watch};

use crate::db::models::{Download, DownloadStatus};
use crate::db::repo::{downloads, settings, transcripts};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::services::media::{Ffmpeg, find_video, quality_label, verify_download};
use crate::services::ytdlp::{DownloadOptions, DownloadOutcome, Quality, StopReason};
use crate::state::AppState;

pub const CONCURRENCY_KEY: &str = "download_concurrency";
pub const QUALITY_KEY: &str = "download_quality";
pub const NOTICE_KEY: &str = "download_notice_ack";

/// Emitted whenever a job's row changes (throttled while running).
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct DownloadUpdate(pub Download);

/// Emitted when jobs are added or removed, so lists refetch.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct DownloadsChanged;

/// Emitted when a lesson's local file appears or disappears (refresh course/lesson views).
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct LessonMediaChanged {
    pub lesson_id: String,
}

pub fn concurrency(state: &AppState) -> usize {
    state
        .db()
        .ok()
        .and_then(|c| settings::get::<i32>(&c, CONCURRENCY_KEY).ok().flatten())
        .unwrap_or(2)
        .clamp(1, 4) as usize
}

pub fn default_quality(state: &AppState) -> Quality {
    state
        .db()
        .ok()
        .and_then(|c| settings::get::<String>(&c, QUALITY_KEY).ok().flatten())
        .and_then(|s| Quality::parse(&s))
        .unwrap_or(Quality::P720)
}

#[derive(Default)]
pub struct DownloadManager {
    wake: Notify,
    jobs: Mutex<HashMap<String, watch::Sender<Option<StopReason>>>>,
}

impl DownloadManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn wake(&self) {
        self.wake.notify_one();
    }

    fn running(&self) -> usize {
        self.jobs.lock().map(|j| j.len()).unwrap_or(0)
    }

    /// Asks a running job to stop. Returns false if it wasn't running.
    pub fn stop(&self, download_id: &str, reason: StopReason) -> bool {
        match self
            .jobs
            .lock()
            .ok()
            .and_then(|j| j.get(download_id).cloned())
        {
            Some(tx) => tx.send(Some(reason)).is_ok(),
            None => false,
        }
    }

    pub fn start(self: &Arc<Self>, app: AppHandle) {
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let state = app.state::<AppState>();
                let mut started = false;
                if mgr.running() < concurrency(&state) {
                    let claimed = state
                        .db()
                        .and_then(|c| downloads::claim_next(&c, Utc::now()));
                    match claimed {
                        Ok(Some(d)) => {
                            let (tx, rx) = watch::channel(None);
                            if let Ok(mut jobs) = mgr.jobs.lock() {
                                jobs.insert(d.id.clone(), tx);
                            }
                            emit_row(&app, &d.id);
                            tauri::async_runtime::spawn(run_job(app.clone(), mgr.clone(), d, rx));
                            started = true;
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("download queue: {e}"),
                    }
                }
                if !started {
                    tokio::select! {
                        _ = mgr.wake.notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                    }
                }
            }
        });
    }
}

pub fn emit_row(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let row = state.db().and_then(|c| downloads::get(&c, id));
    match row {
        Ok(d) => {
            let _ = DownloadUpdate(d).emit(app);
        }
        Err(_) => {
            let _ = DownloadsChanged.emit(app);
        }
    }
}

fn retryable(e: &AppError) -> bool {
    matches!(e.code, ErrorCode::Http | ErrorCode::Timeout | ErrorCode::Io)
        || e.message.contains("verification")
}

async fn run_job(
    app: AppHandle,
    mgr: Arc<DownloadManager>,
    d: Download,
    stop: watch::Receiver<Option<StopReason>>,
) {
    let result = download_one(&app, &d, stop).await;
    if let Ok(mut jobs) = mgr.jobs.lock() {
        jobs.remove(&d.id);
    }
    let state = app.state::<AppState>();
    match result {
        Ok(DownloadOutcome::Completed) => {
            let _ = LessonMediaChanged {
                lesson_id: d.lesson_id.clone(),
            }
            .emit(&app);
            // Make the transcript available offline too (best effort, after the video).
            let needs_transcript = state
                .db()
                .and_then(|c| transcripts::get_transcript(&c, &d.lesson_id))
                .map(|t| t.source.is_none())
                .unwrap_or(false);
            if needs_transcript {
                let app2 = app.clone();
                let lesson = d.lesson_id.clone();
                tauri::async_runtime::spawn(async move {
                    let st = app2.state::<AppState>();
                    if let Err(e) =
                        crate::services::transcripts::fetch_for_lesson(&st, &lesson).await
                    {
                        log::warn!("transcript after download failed: {e}");
                    }
                });
            }
        }
        Ok(DownloadOutcome::Stopped(_)) => {}
        Err(e) => {
            log::warn!("download {} failed: {e}", d.id);
            let code = serde_json::to_value(e.code)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            if let Ok(c) = state.db() {
                let _ =
                    downloads::mark_error(&c, &d.id, &code, &e.message, retryable(&e), Utc::now());
            }
        }
    }
    emit_row(&app, &d.id);
    mgr.wake();
}

async fn download_one(
    app: &AppHandle,
    d: &Download,
    stop: watch::Receiver<Option<StopReason>>,
) -> AppResult<DownloadOutcome> {
    let state = app.state::<AppState>();
    let video_id = d
        .youtube_video_id
        .clone()
        .ok_or_else(|| AppError::invalid("lesson has no video"))?;
    let ytdlp = state.ytdlp()?;
    let ffmpeg = Ffmpeg::locate(&state.paths.bin_dir())?;
    let quality = Quality::parse(&d.quality).unwrap_or(Quality::P720);
    let out_dir = state
        .paths
        .media_dir()
        .join(&d.course_slug)
        .join(&d.lesson_id);

    let mut last = Instant::now() - Duration::from_secs(1);
    let on_progress = |s: crate::services::ytdlp::ProgressSnapshot| {
        if last.elapsed() < Duration::from_millis(500) {
            return;
        }
        last = Instant::now();
        if let Ok(c) = state.db() {
            let _ = downloads::update_progress(
                &c,
                &d.id,
                s.fraction,
                Some(s.bytes_done),
                s.bytes_total,
                s.speed_bps,
                s.eta_sec,
            );
        }
        emit_row(app, &d.id);
    };
    let opts = DownloadOptions {
        video_id: &video_id,
        quality,
        out_dir: &out_dir,
        ffmpeg_dir: ffmpeg.dir(),
    };
    let outcome = ytdlp.download_video(&opts, on_progress, stop).await?;
    if !matches!(outcome, DownloadOutcome::Completed) {
        return Ok(outcome);
    }

    // SPEC §7.3 invariant: only record the path after the merged file passes a sanity check.
    let video = find_video(&out_dir)
        .ok_or_else(|| AppError::internal("download finished but no video file was found"))?;
    let size = std::fs::metadata(&video)?.len();
    let probed = ffmpeg.probe_duration(&video).await?;
    verify_download(size, probed, d.duration_sec.map(f64::from)).map_err(|m| {
        AppError::new(
            ErrorCode::Internal,
            format!("downloaded file failed verification: {m}"),
        )
    })?;
    let local_quality = ffmpeg.probe_height(&video).await.ok().map(quality_label);
    let thumb = out_dir.join("video.jpg");
    let conn = state.db()?;
    downloads::mark_done(
        &conn,
        &d.id,
        &video.to_string_lossy(),
        thumb
            .is_file()
            .then(|| thumb.to_string_lossy().into_owned())
            .as_deref(),
        size,
        local_quality.as_deref(),
    )?;
    Ok(DownloadOutcome::Completed)
}

/// Marks interrupted jobs for resumption; call once at startup before `start`.
pub fn recover(state: &AppState) {
    match state.db().and_then(|c| downloads::requeue_interrupted(&c)) {
        Ok(n) if n > 0 => log::info!("resuming {n} interrupted downloads"),
        Ok(_) => {}
        Err(e) => log::warn!("could not requeue downloads: {e}"),
    }
}

/// Fills `lessons.local_quality` for files downloaded before it was tracked.
pub async fn backfill_quality(state: &AppState) {
    let rows: Vec<(String, String)> = match state.db().and_then(|c| {
        let mut st = c.prepare(
            "SELECT id, local_video_path FROM lessons
              WHERE local_video_path IS NOT NULL AND local_quality IS NULL",
        )?;
        let v = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }) {
        Ok(v) => v,
        Err(e) => return log::warn!("quality backfill: {e}"),
    };
    if rows.is_empty() {
        return;
    }
    let Ok(ff) = Ffmpeg::locate(&state.paths.bin_dir()) else {
        return;
    };
    for (id, path) in rows {
        if let Ok(h) = ff.probe_height(std::path::Path::new(&path)).await {
            let q = quality_label(h);
            if let Ok(c) = state.db() {
                let _ = c.execute(
                    "UPDATE lessons SET local_quality = ?2 WHERE id = ?1",
                    rusqlite::params![id, q],
                );
            }
        }
    }
}

/// Used by commands: the status a job must not be overwritten from.
pub fn is_active(status: DownloadStatus) -> bool {
    matches!(status, DownloadStatus::Queued | DownloadStatus::Running)
}

#[cfg(test)]
mod tests {
    use crate::services::media::{Ffmpeg, find_video, verify_download};
    use crate::services::ytdlp::{DownloadOptions, DownloadOutcome, Quality, StopReason, YtDlp};

    /// Real network round-trip: download, pause, resume from the .part file, verify.
    /// Run with `cargo test --lib -- --ignored live_download --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_download_pause_resume_verify() {
        let bin = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let y = YtDlp::locate(bin.path()).unwrap();
        let ff = Ffmpeg::locate(bin.path()).unwrap();
        let opts = DownloadOptions {
            video_id: "cQP8WApzIQQ",
            quality: Quality::P360,
            out_dir: out.path(),
            ffmpeg_dir: ff.dir(),
        };

        // Pause once a few MB have arrived (extraction alone can take 40+ s).
        let (tx, rx) = tokio::sync::watch::channel(None);
        let mut first_bytes = 0u64;
        let r = y
            .download_video(
                &opts,
                |s| {
                    first_bytes = s.bytes_done;
                    if s.bytes_done > 5_000_000 {
                        let _ = tx.send(Some(StopReason::Pause));
                    }
                },
                rx,
            )
            .await
            .unwrap();
        assert!(matches!(r, DownloadOutcome::Stopped(StopReason::Pause)));
        println!("paused after {first_bytes} bytes");

        let (_tx2, rx2) = tokio::sync::watch::channel(None);
        let mut resumed_at: Option<u64> = None;
        let mut last = 0u64;
        let r = y
            .download_video(
                &opts,
                |s| {
                    resumed_at.get_or_insert(s.bytes_done);
                    last = s.bytes_done;
                },
                rx2,
            )
            .await
            .unwrap();
        assert!(matches!(r, DownloadOutcome::Completed));
        println!("resumed at {resumed_at:?}, finished at {last}");
        assert!(
            resumed_at.unwrap_or(0) > 0,
            "resumed from the partial file rather than restarting"
        );

        let video = find_video(out.path()).expect("merged video");
        let size = std::fs::metadata(&video).unwrap().len();
        let dur = ff.probe_duration(&video).await.unwrap();
        println!("{} {size} bytes {dur}s", video.display());
        verify_download(size, dur, Some(4775.0)).unwrap();
        assert!(out.path().join("video.jpg").is_file());
    }
}
