use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::db::models::*;
use crate::db::repo::{courses, downloads, lessons, settings};
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};
use crate::services::downloads::{
    CONCURRENCY_KEY, DownloadManager, DownloadsChanged, LessonMediaChanged, NOTICE_KEY,
    QUALITY_KEY, default_quality, emit_row,
};
use crate::services::media::{Ffmpeg, dir_size, remove_dir_within};
use crate::services::updater::{self, UpdateResult};
use crate::services::ytdlp::{COOKIE_BROWSERS, COOKIES_BROWSER_KEY, Quality, StopReason};
use crate::state::AppState;

type Mgr<'a> = State<'a, Arc<DownloadManager>>;

fn quality_or_default(state: &AppState, q: Option<String>) -> AppResult<Quality> {
    match q {
        Some(s) => Quality::parse(&s)
            .ok_or_else(|| AppError::invalid("quality must be 360p, 480p, 720p or 1080p")),
        None => Ok(default_quality(state)),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn list_downloads(state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    downloads::list(&*state.db()?)
}

#[tauri::command]
#[specta::specta]
pub async fn enqueue_lesson_download(
    app: AppHandle,
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    lesson_id: String,
    quality: Option<String>,
) -> AppResult<Download> {
    let q = quality_or_default(&state, quality)?;
    let d = downloads::enqueue(&*state.db()?, &lesson_id, q.as_str())?;
    let _ = DownloadsChanged.emit(&app);
    mgr.wake();
    Ok(d)
}

/// Sets a course to Stream or Download mode. Download queues every lesson not yet on disk;
/// Stream stops and removes pending jobs (finished files are kept).
#[tauri::command]
#[specta::specta]
pub async fn set_course_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    course_id: String,
    mode: CourseMode,
    quality: Option<String>,
) -> AppResult<i32> {
    let q = quality_or_default(&state, quality)?;
    let conn = state.db()?;
    courses::update_course(
        &conn,
        &course_id,
        &CourseUpdate {
            mode: Some(mode),
            ..Default::default()
        },
    )?;
    let n = match mode {
        CourseMode::Download => downloads::enqueue_course(&conn, &course_id, q.as_str())?,
        CourseMode::Stream => {
            let pending: Vec<Download> = downloads::list(&conn)?
                .into_iter()
                .filter(|d| d.course_id == course_id && d.status != DownloadStatus::Done)
                .collect();
            for d in &pending {
                mgr.stop(&d.id, StopReason::Cancel);
                downloads::delete(&conn, &d.id)?;
            }
            pending.len() as i32
        }
    };
    drop(conn);
    let _ = DownloadsChanged.emit(&app);
    mgr.wake();
    Ok(n)
}

/// Per-lesson override: `download` queues it, `stream` cancels any pending job, `None` follows the course.
#[tauri::command]
#[specta::specta]
pub async fn set_lesson_mode_override(
    app: AppHandle,
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    lesson_id: String,
    mode: Option<CourseMode>,
) -> AppResult<()> {
    let conn = state.db()?;
    conn.execute(
        "UPDATE lessons SET mode_override = ?2, updated_at = ?3 WHERE id = ?1",
        params![lesson_id, mode, now()],
    )?;
    match mode {
        Some(CourseMode::Download) => {
            let q = default_quality(&state);
            downloads::enqueue(&conn, &lesson_id, q.as_str())?;
        }
        Some(CourseMode::Stream) => {
            if let Some(d) = downloads::get_for_lesson(&conn, &lesson_id)?
                .filter(|d| d.status != DownloadStatus::Done)
            {
                mgr.stop(&d.id, StopReason::Cancel);
                downloads::delete(&conn, &d.id)?;
            }
        }
        None => {}
    }
    drop(conn);
    let _ = DownloadsChanged.emit(&app);
    mgr.wake();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn pause_download(
    app: AppHandle,
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    id: String,
) -> AppResult<()> {
    downloads::set_status(&*state.db()?, &id, DownloadStatus::Paused)?;
    mgr.stop(&id, StopReason::Pause);
    emit_row(&app, &id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn resume_download(
    app: AppHandle,
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    id: String,
) -> AppResult<()> {
    downloads::set_status(&*state.db()?, &id, DownloadStatus::Queued)?;
    emit_row(&app, &id);
    mgr.wake();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn retry_download(
    app: AppHandle,
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    id: String,
) -> AppResult<()> {
    downloads::retry(&*state.db()?, &id)?;
    emit_row(&app, &id);
    mgr.wake();
    Ok(())
}

/// Stops a job, removes it from the queue and deletes its partial files.
#[tauri::command]
#[specta::specta]
pub async fn cancel_download(
    app: AppHandle,
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    id: String,
) -> AppResult<()> {
    let d = downloads::get(&*state.db()?, &id)?;
    let was_running = mgr.stop(&id, StopReason::Cancel);
    downloads::delete(&*state.db()?, &id)?;
    let has_file = lessons::get_lesson(&*state.db()?, &d.lesson_id)?
        .local_video_path
        .is_some();
    if !has_file {
        if was_running {
            // Give yt-dlp a moment to exit before removing its files.
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        }
        let dir = state
            .paths
            .media_dir()
            .join(&d.course_slug)
            .join(&d.lesson_id);
        remove_dir_within(&state.paths.media_dir(), &dir)?;
    }
    let _ = DownloadsChanged.emit(&app);
    mgr.wake();
    Ok(())
}

/// Deletes downloaded media for a course (or one lesson) but keeps notes, progress and transcripts.
#[tauri::command]
#[specta::specta]
pub async fn delete_local_files(
    app: AppHandle,
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    course_id: String,
    lesson_id: Option<String>,
) -> AppResult<f64> {
    let (slug, ids) = {
        let conn = state.db()?;
        let course = courses::get_course(&conn, &course_id)?;
        let ids: Vec<String> = match &lesson_id {
            Some(l) => vec![l.clone()],
            None => lessons::list_course_lessons(&conn, &course_id)?
                .into_iter()
                .map(|l| l.id)
                .collect(),
        };
        for id in &ids {
            if let Some(d) = downloads::get_for_lesson(&conn, id)? {
                mgr.stop(&d.id, StopReason::Cancel);
            }
        }
        (course.slug, ids)
    };
    let media = state.paths.media_dir();
    let mut freed = 0u64;
    for id in &ids {
        let dir = media.join(&slug).join(id);
        freed += dir_size(&dir);
        remove_dir_within(&media, &dir)?;
    }
    downloads::forget_local(&*state.db()?, &ids)?;
    for id in ids {
        let _ = LessonMediaChanged { lesson_id: id }.emit(&app);
    }
    let _ = DownloadsChanged.emit(&app);
    Ok(freed as f64)
}

#[tauri::command]
#[specta::specta]
pub async fn get_storage_usage(state: State<'_, AppState>) -> AppResult<StorageUsage> {
    let conn = state.db()?;
    let media = state.paths.media_dir();
    let mut out = Vec::new();
    for s in courses::list_course_summaries(&conn)? {
        let bytes = dir_size(&media.join(&s.course.slug));
        let downloaded: i32 = conn.query_row(
            "SELECT count(*) FROM lessons l JOIN modules m ON m.id = l.module_id
             WHERE m.course_id = ?1 AND l.local_video_path IS NOT NULL",
            [&s.course.id],
            |r| r.get(0),
        )?;
        if bytes > 0 || downloaded > 0 {
            out.push(CourseStorage {
                course_id: s.course.id,
                course_title: s.course.title,
                bytes: bytes as f64,
                downloaded_lessons: downloaded,
            });
        }
    }
    out.sort_by(|a, b| b.bytes.total_cmp(&a.bytes));
    Ok(StorageUsage {
        total_bytes: dir_size(&media) as f64,
        media_dir: media.display().to_string(),
        courses: out,
    })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SizeEstimate {
    pub lessons: i32,
    pub bytes: f64,
    /// Lessons without a known duration (not included in `bytes`).
    pub unknown: i32,
}

/// Estimated download size for the lessons of a course that aren't on disk yet.
#[tauri::command]
#[specta::specta]
pub async fn estimate_download_size(
    state: State<'_, AppState>,
    course_id: String,
    quality: String,
) -> AppResult<SizeEstimate> {
    let q = Quality::parse(&quality).ok_or_else(|| AppError::invalid("unknown quality"))?;
    let all = lessons::list_course_lessons(&*state.db()?, &course_id)?;
    let pending: Vec<_> = all
        .into_iter()
        .filter(|l| {
            l.lesson_type == LessonType::Video
                && l.youtube_video_id.is_some()
                && l.local_video_path.is_none()
        })
        .collect();
    let secs: f64 = pending
        .iter()
        .filter_map(|l| l.duration_sec)
        .map(f64::from)
        .sum();
    Ok(SizeEstimate {
        lessons: pending.len() as i32,
        bytes: secs * q.est_bitrate() / 8.0,
        unknown: pending.iter().filter(|l| l.duration_sec.is_none()).count() as i32,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSettings {
    pub quality: Quality,
    pub concurrency: i32,
    pub notice_acknowledged: bool,
    /// Advanced, off by default (SPEC §7.9): browser for yt-dlp's `--cookies-from-browser`.
    #[specta(optional)]
    #[serde(default)]
    pub cookies_browser: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn get_download_settings(state: State<'_, AppState>) -> AppResult<DownloadSettings> {
    let conn = state.db()?;
    Ok(DownloadSettings {
        quality: settings::get::<String>(&conn, QUALITY_KEY)?
            .and_then(|s| Quality::parse(&s))
            .unwrap_or(Quality::P720),
        concurrency: settings::get::<i32>(&conn, CONCURRENCY_KEY)?
            .unwrap_or(2)
            .clamp(1, 4),
        notice_acknowledged: settings::get::<bool>(&conn, NOTICE_KEY)?.unwrap_or(false),
        cookies_browser: settings::get::<String>(&conn, COOKIES_BROWSER_KEY)?
            .filter(|b| COOKIE_BROWSERS.contains(&b.as_str())),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn set_download_settings(
    state: State<'_, AppState>,
    mgr: Mgr<'_>,
    settings_in: DownloadSettings,
) -> AppResult<()> {
    let cookies = settings_in
        .cookies_browser
        .filter(|b| !b.is_empty())
        .map(|b| b.to_ascii_lowercase());
    if let Some(b) = &cookies
        && !COOKIE_BROWSERS.contains(&b.as_str())
    {
        return Err(AppError::invalid(format!(
            "unsupported browser for cookies: {b} (use one of {})",
            COOKIE_BROWSERS.join(", ")
        )));
    }
    let conn = state.db()?;
    settings::set(&conn, QUALITY_KEY, settings_in.quality.as_str())?;
    settings::set(&conn, COOKIES_BROWSER_KEY, &cookies)?;
    settings::set(&conn, CONCURRENCY_KEY, &settings_in.concurrency.clamp(1, 4))?;
    settings::set(&conn, NOTICE_KEY, &settings_in.notice_acknowledged)?;
    mgr.wake();
    Ok(())
}

/// Grabs the frame at `sec` from the lesson's local video and attaches it to an annotation.
/// Returns the image path relative to the data dir (used as `lectern-attachment:<path>` in Markdown).
#[tauri::command]
#[specta::specta]
pub async fn capture_frame(
    state: State<'_, AppState>,
    annotation_id: String,
    sec: f64,
) -> AppResult<String> {
    let (video, slug, lesson_id) = {
        let conn = state.db()?;
        let a = crate::db::repo::annotations::get(&conn, &annotation_id)?;
        let l = lessons::get_lesson(&conn, &a.lesson_id)?;
        let course = courses::get_course(&conn, &lessons::course_id_of_lesson(&conn, &l.id)?)?;
        let video = l
            .local_video_path
            .ok_or_else(|| AppError::invalid("frame capture needs a downloaded video"))?;
        (video, course.slug, l.id)
    };
    let ffmpeg = Ffmpeg::locate(&state.paths.bin_dir())?;
    let id = new_id();
    let rel = format!("media/{slug}/{lesson_id}/frames/{id}.png");
    ffmpeg
        .capture_frame(
            std::path::Path::new(&video),
            sec,
            &state.paths.data_dir.join(&rel),
        )
        .await?;
    let ts = now();
    state.db()?.execute(
        "INSERT INTO attachments (id, annotation_id, kind, path, created_at, updated_at) VALUES (?1, ?2, 'image', ?3, ?4, ?4)",
        params![id, annotation_id, rel, ts],
    )?;
    Ok(rel)
}

#[tauri::command]
#[specta::specta]
pub async fn check_ytdlp_update(state: State<'_, AppState>) -> AppResult<UpdateResult> {
    updater::check(&state, true).await
}
