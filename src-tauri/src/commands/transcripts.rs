use serde::Serialize;
use specta::Type;
use tauri::State;
use tauri::ipc::Channel;

use crate::db::models::{Chapter, LessonType, Transcript};
use crate::db::repo::{lessons, transcripts};
use crate::error::AppResult;
use crate::services::transcripts::fetch_for_lesson;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn get_transcript(
    state: State<'_, AppState>,
    lesson_id: String,
) -> AppResult<Transcript> {
    transcripts::get_transcript(&*state.db()?, &lesson_id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_chapters(
    state: State<'_, AppState>,
    lesson_id: String,
) -> AppResult<Vec<Chapter>> {
    transcripts::get_chapters(&*state.db()?, &lesson_id)
}

/// Fetches (or re-fetches) subtitles and chapters for one video lesson via yt-dlp.
#[tauri::command]
#[specta::specta]
pub async fn fetch_transcript(
    state: State<'_, AppState>,
    lesson_id: String,
) -> AppResult<Transcript> {
    fetch_for_lesson(&state, &lesson_id).await
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptProgress {
    pub done: i32,
    pub total: i32,
    pub lesson_title: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BulkTranscriptReport {
    pub fetched: i32,
    pub unavailable: i32,
    pub failed: i32,
    pub skipped: i32,
}

/// Fetches transcripts for every video lesson in a course that doesn't have one yet,
/// one at a time with a short pause between requests to stay clear of YouTube rate limits.
#[tauri::command]
#[specta::specta]
pub async fn fetch_course_transcripts(
    state: State<'_, AppState>,
    course_id: String,
    on_progress: Channel<TranscriptProgress>,
) -> AppResult<BulkTranscriptReport> {
    let pending: Vec<(String, String)> = {
        let conn = state.db()?;
        let all = lessons::list_course_lessons(&conn, &course_id)?;
        let mut out = Vec::new();
        for l in all
            .into_iter()
            .filter(|l| l.lesson_type == LessonType::Video && l.youtube_video_id.is_some())
        {
            if transcripts::get_transcript(&conn, &l.id)?.source.is_none() {
                out.push((l.id, l.title));
            }
        }
        out
    };
    let total = pending.len() as i32;
    let mut report = BulkTranscriptReport {
        fetched: 0,
        unavailable: 0,
        failed: 0,
        skipped: 0,
    };
    for (i, (id, title)) in pending.into_iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }
        let error = match fetch_for_lesson(&state, &id).await {
            Ok(t) if t.source.as_deref() == Some("none") => {
                report.unavailable += 1;
                None
            }
            Ok(_) => {
                report.fetched += 1;
                None
            }
            Err(e) => {
                report.failed += 1;
                Some(e.message)
            }
        };
        let _ = on_progress.send(TranscriptProgress {
            done: i as i32 + 1,
            total,
            lesson_title: title,
            error,
        });
    }
    Ok(report)
}
