//! Fetching transcripts (and chapters) for lessons via yt-dlp, without downloading video.

use std::collections::HashSet;
use std::sync::Mutex;

use crate::db::models::Transcript;
use crate::db::new_id;
use crate::db::repo::{lessons, settings, transcripts};
use crate::error::{AppError, AppResult};
use crate::services::vtt;
use crate::services::ytdlp::{TrackKind, choose_track};
use crate::state::AppState;

pub const SUBTITLE_LANGS_KEY: &str = "subtitle_langs";

/// Lessons with a fetch in flight, so double-clicks/StrictMode don't spawn duplicate yt-dlp runs.
static IN_FLIGHT: Mutex<Option<HashSet<String>>> = Mutex::new(None);

struct InFlight(String);
impl InFlight {
    fn acquire(id: &str) -> AppResult<Self> {
        let mut g = IN_FLIGHT
            .lock()
            .map_err(|_| AppError::internal("lock poisoned"))?;
        if !g.get_or_insert_with(HashSet::new).insert(id.to_string()) {
            return Err(AppError::invalid(
                "a transcript fetch for this lesson is already running",
            ));
        }
        Ok(Self(id.to_string()))
    }
}
impl Drop for InFlight {
    fn drop(&mut self) {
        if let Ok(mut g) = IN_FLIGHT.lock()
            && let Some(set) = g.as_mut()
        {
            set.remove(&self.0);
        }
    }
}

pub async fn fetch_for_lesson(state: &AppState, lesson_id: &str) -> AppResult<Transcript> {
    let _guard = InFlight::acquire(lesson_id)?;
    // Read what we need, then release the DB lock before any network work.
    let (video_id, prefs) = {
        let conn = state.db()?;
        let lesson = lessons::get_lesson(&conn, lesson_id)?;
        let video_id = lesson
            .youtube_video_id
            .ok_or_else(|| AppError::invalid("this lesson has no YouTube video"))?;
        let prefs: Vec<String> = settings::get(&conn, SUBTITLE_LANGS_KEY)?
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or_else(|| vec!["en".into()]);
        (video_id, prefs)
    };
    let ytdlp = state.ytdlp()?;
    let probe = ytdlp.probe_subtitles(&video_id).await?;
    let Some((track, kind)) = choose_track(&probe, &prefs) else {
        let conn = state.db()?;
        transcripts::replace(&conn, lesson_id, "none", None, &[], &probe.chapters)?;
        return transcripts::get_transcript(&conn, lesson_id);
    };
    let dir = state.paths.data_dir.join("tmp").join(new_id());
    let downloaded = ytdlp.download_subtitle(&video_id, &track, kind, &dir).await;
    let text = downloaded.and_then(|p| Ok(std::fs::read_to_string(p)?));
    let _ = std::fs::remove_dir_all(&dir);
    let cues = vtt::parse(&text?, kind == TrackKind::Auto);
    let source = if kind == TrackKind::Manual {
        "manual"
    } else {
        "auto"
    };
    let conn = state.db()?;
    transcripts::replace(
        &conn,
        lesson_id,
        source,
        Some(&track),
        &cues,
        &probe.chapters,
    )?;
    transcripts::get_transcript(&conn, lesson_id)
}

#[cfg(test)]
mod tests {
    use crate::services::{
        vtt,
        ytdlp::{TrackKind, YtDlp, choose_track},
    };

    /// Real network round-trip. Run with `cargo test -- --ignored live_transcript`.
    #[tokio::test]
    #[ignore]
    async fn live_transcript_for_mit_lecture_1() {
        let dir = tempfile::tempdir().unwrap();
        let y = YtDlp::locate(dir.path()).unwrap();
        let probe = y.probe_subtitles("cQP8WApzIQQ").await.unwrap();
        assert!(!probe.chapters.is_empty());
        let (track, kind) = choose_track(&probe, &["en".into()]).unwrap();
        assert_eq!((track.as_str(), kind), ("en-orig", TrackKind::Auto));
        let path = y
            .download_subtitle("cQP8WApzIQQ", &track, kind, dir.path())
            .await
            .unwrap();
        let cues = vtt::parse(&std::fs::read_to_string(path).unwrap(), true);
        assert!(cues.len() > 500, "got {} cues", cues.len());
        assert!(
            cues.iter()
                .any(|c| c.text.to_lowercase().contains("mapreduce"))
        );
    }
}
