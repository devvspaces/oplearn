use tauri::State;

use crate::db::models::Lesson;
use crate::db::repo::{courses, import as repo};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::services::import::{
    ImportReport, ImportSource, ImportedPlaylist, ImportedVideo, parse_youtube_url,
};
use crate::services::youtube::YouTubeApi;
use crate::state::AppState;

/// Whether an API failure should fall back to yt-dlp (everything except a definitive "not found").
fn should_fall_back(e: &AppError) -> bool {
    e.code != ErrorCode::NotFound
}

/// Adds a client's spent quota units to today's total (best effort).
pub fn record_units(state: &AppState, api: &YouTubeApi<'_>) {
    if let Ok(c) = state.db() {
        let _ = crate::db::repo::yt_search::add_units(&c, api.units(), chrono::Utc::now());
    }
}

pub async fn fetch_playlist(
    state: &AppState,
    playlist_id: &str,
) -> AppResult<(ImportedPlaylist, ImportSource)> {
    if let Some(key) = state.api_key()? {
        let api = YouTubeApi::new(&state.http, &key);
        let r = api.fetch_playlist(playlist_id).await;
        record_units(state, &api);
        match r {
            Ok(p) => return Ok((p, ImportSource::Api)),
            Err(e) if should_fall_back(&e) => {
                log::warn!("Data API failed ({e}); falling back to yt-dlp")
            }
            Err(e) => return Err(e),
        }
    }
    Ok((
        state.ytdlp()?.dump_playlist_flat(playlist_id).await?,
        ImportSource::Ytdlp,
    ))
}

pub async fn fetch_video(state: &AppState, video_id: &str) -> AppResult<ImportedVideo> {
    if let Some(key) = state.api_key()? {
        let api = YouTubeApi::new(&state.http, &key);
        let r = api.fetch_video(video_id).await;
        record_units(state, &api);
        match r {
            Ok(v) => return Ok(v),
            Err(e) if should_fall_back(&e) => {
                log::warn!("Data API failed ({e}); falling back to yt-dlp")
            }
            Err(e) => return Err(e),
        }
    }
    state.ytdlp()?.dump_video(video_id).await
}

/// Creates a course from a playlist URL (or a watch URL that carries `list=`).
#[tauri::command]
#[specta::specta]
pub async fn import_playlist(state: State<'_, AppState>, url: String) -> AppResult<ImportReport> {
    let parsed = parse_youtube_url(&url)
        .ok_or_else(|| AppError::invalid("that doesn't look like a YouTube URL"))?;
    let playlist_id = parsed.playlist_id.ok_or_else(|| {
        AppError::invalid(
            "this URL has no playlist; use “Add video” to add a single video to a course",
        )
    })?;
    let (playlist, source) = fetch_playlist(&state, &playlist_id).await?;
    if playlist.videos.is_empty() {
        return Err(AppError::invalid("the playlist has no available videos"));
    }
    let (course_id, added) = repo::import_playlist(&*state.db()?, &playlist)?;
    Ok(ImportReport {
        course_id,
        added,
        skipped: playlist.skipped,
        source,
    })
}

/// Re-fetches the linked playlist and appends new videos without touching existing lessons.
#[tauri::command]
#[specta::specta]
pub async fn resync_course(
    state: State<'_, AppState>,
    course_id: String,
) -> AppResult<ImportReport> {
    let course = courses::get_course(&*state.db()?, &course_id)?;
    let playlist_id = course
        .youtube_playlist_id
        .ok_or_else(|| AppError::invalid("this course isn't linked to a playlist"))?;
    let (playlist, source) = fetch_playlist(&state, &playlist_id).await?;
    let added = repo::resync_playlist(&*state.db()?, &course_id, &playlist)?;
    Ok(ImportReport {
        course_id,
        added,
        skipped: playlist.skipped,
        source,
    })
}

/// Adds one video (by URL) to the end of a module.
#[tauri::command]
#[specta::specta]
pub async fn add_video_to_module(
    state: State<'_, AppState>,
    module_id: String,
    url: String,
) -> AppResult<Lesson> {
    let parsed = parse_youtube_url(&url)
        .ok_or_else(|| AppError::invalid("that doesn't look like a YouTube URL"))?;
    let video_id = parsed
        .video_id
        .ok_or_else(|| AppError::invalid("this URL has no video id"))?;
    let video = fetch_video(&state, &video_id).await?;
    repo::add_video(&*state.db()?, &module_id, &video)
}
