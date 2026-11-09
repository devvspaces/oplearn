//! Download queue persistence (SPEC §7.3). One row per lesson.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::models::*;
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};

pub const MAX_ATTEMPTS: i32 = 5;

const SELECT: &str = "SELECT d.id, d.lesson_id, l.title, c.id, c.title, c.slug, l.youtube_video_id, l.duration_sec,
        d.status, d.quality, d.progress, d.bytes_done, d.bytes_total, d.speed_bps, d.eta_sec, d.error_code,
        d.error_message, d.attempts, d.next_retry_at, d.created_at, d.updated_at, l.local_quality
   FROM downloads d JOIN lessons l ON l.id = d.lesson_id JOIN modules m ON m.id = l.module_id
   JOIN courses c ON c.id = m.course_id";

fn map(r: &Row<'_>) -> rusqlite::Result<Download> {
    Ok(Download {
        id: r.get(0)?,
        lesson_id: r.get(1)?,
        lesson_title: r.get(2)?,
        course_id: r.get(3)?,
        course_title: r.get(4)?,
        course_slug: r.get(5)?,
        youtube_video_id: r.get(6)?,
        duration_sec: r.get(7)?,
        status: r.get(8)?,
        quality: r.get(9)?,
        progress: r.get(10)?,
        bytes_done: r.get::<_, Option<i64>>(11)?.map(|v| v as f64),
        bytes_total: r.get::<_, Option<i64>>(12)?.map(|v| v as f64),
        speed_bps: r.get(13)?,
        eta_sec: r.get(14)?,
        error_code: r.get(15)?,
        error_message: r.get(16)?,
        attempts: r.get(17)?,
        next_retry_at: r.get(18)?,
        created_at: r.get(19)?,
        updated_at: r.get(20)?,
        local_quality: r.get(21)?,
    })
}

/// Exponential backoff after `attempts` failed tries: 30 s, 60 s, 120 s … capped at 10 min.
pub fn backoff_secs(attempts: i32) -> i64 {
    let exp = (attempts.max(1) - 1).min(10) as u32;
    (30i64 * 2i64.pow(exp)).min(600)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Download> {
    conn.query_row(&format!("{SELECT} WHERE d.id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("download"))
}

pub fn get_for_lesson(conn: &Connection, lesson_id: &str) -> AppResult<Option<Download>> {
    Ok(conn
        .query_row(
            &format!("{SELECT} WHERE d.lesson_id = ?1"),
            [lesson_id],
            map,
        )
        .optional()?)
}

/// Active jobs first (running, queued, paused, error), then finished ones; outline order within.
pub fn list(conn: &Connection) -> AppResult<Vec<Download>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} ORDER BY CASE d.status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 WHEN 'paused' THEN 2
                  WHEN 'error' THEN 3 ELSE 4 END, c.created_at, m.position, l.position"
    ))?;
    Ok(stmt.query_map([], map)?.collect::<Result<_, _>>()?)
}

/// Queues a video lesson. Idempotent: an active job is left alone; a paused, failed or stale
/// finished job is reset to queued.
pub fn enqueue(conn: &Connection, lesson_id: &str, quality: &str) -> AppResult<Download> {
    let (video, local): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT youtube_video_id, local_video_path FROM lessons WHERE id = ?1",
            [lesson_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("lesson"))?;
    if video.is_none() {
        return Err(AppError::invalid(
            "only YouTube video lessons can be downloaded",
        ));
    }
    let ts = now();
    match get_for_lesson(conn, lesson_id)? {
        Some(d) if matches!(d.status, DownloadStatus::Queued | DownloadStatus::Running) => Ok(d),
        Some(d) if d.status == DownloadStatus::Done && local.is_some() => Ok(d),
        Some(d) => {
            conn.execute(
                "UPDATE downloads SET status='queued', quality=?2, attempts=0, error_code=NULL, error_message=NULL,
                        next_retry_at=NULL, updated_at=?3 WHERE id=?1",
                params![d.id, quality, ts],
            )?;
            get(conn, &d.id)
        }
        None => {
            if local.is_some() {
                return Err(AppError::invalid("this lesson is already downloaded"));
            }
            let id = new_id();
            conn.execute(
                "INSERT INTO downloads (id, lesson_id, status, quality, progress, attempts, created_at, updated_at)
                 VALUES (?1, ?2, 'queued', ?3, 0, 0, ?4, ?4)",
                params![id, lesson_id, quality, ts],
            )?;
            get(conn, &id)
        }
    }
}

/// Queues every video lesson of a course that isn't downloaded yet. Returns how many were queued.
pub fn enqueue_course(conn: &Connection, course_id: &str, quality: &str) -> AppResult<i32> {
    let mut stmt = conn.prepare(
        "SELECT l.id FROM lessons l JOIN modules m ON m.id = l.module_id
         WHERE m.course_id = ?1 AND l.type = 'video' AND l.youtube_video_id IS NOT NULL
           AND l.local_video_path IS NULL AND coalesce(l.mode_override, '') != 'stream'
         ORDER BY m.position, l.position",
    )?;
    let ids: Vec<String> = stmt
        .query_map([course_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for id in &ids {
        enqueue(&tx, id, quality)?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}

/// Atomically claims the next runnable job (queued, or failed and due for retry) and marks it running.
pub fn claim_next(conn: &Connection, at: DateTime<Utc>) -> AppResult<Option<Download>> {
    let tx = conn.unchecked_transaction()?;
    let at_s = at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let id: Option<String> = tx
        .query_row(
            "SELECT d.id FROM downloads d JOIN lessons l ON l.id = d.lesson_id JOIN modules m ON m.id = l.module_id
             JOIN courses c ON c.id = m.course_id
             WHERE d.status = 'queued'
                OR (d.status = 'error' AND d.next_retry_at IS NOT NULL AND d.next_retry_at <= ?1 AND d.attempts < ?2)
             ORDER BY d.status = 'error', c.created_at, m.position, l.position LIMIT 1",
            params![at_s, MAX_ATTEMPTS],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = id else { return Ok(None) };
    tx.execute(
        "UPDATE downloads SET status='running', attempts=attempts+1, started_at=?2, next_retry_at=NULL,
                error_code=NULL, error_message=NULL, speed_bps=NULL, eta_sec=NULL, updated_at=?2 WHERE id=?1",
        params![id, at_s],
    )?;
    tx.commit()?;
    Ok(Some(get(conn, &id)?))
}

pub fn update_progress(
    conn: &Connection,
    id: &str,
    progress: f64,
    bytes_done: Option<u64>,
    bytes_total: Option<u64>,
    speed: Option<f64>,
    eta: Option<u64>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE downloads SET progress=?2, bytes_done=?3, bytes_total=coalesce(?4, bytes_total), speed_bps=?5,
                eta_sec=?6, updated_at=?7 WHERE id=?1 AND status='running'",
        params![
            id,
            progress.clamp(0.0, 1.0),
            bytes_done.map(|v| v as i64),
            bytes_total.map(|v| v as i64),
            speed,
            eta.map(|v| v.min(i32::MAX as u64) as i64),
            now()
        ],
    )?;
    Ok(())
}

/// Marks the job done and records the verified file on the lesson, in one transaction.
pub fn mark_done(
    conn: &Connection,
    id: &str,
    video_path: &str,
    thumb_path: Option<&str>,
    size: u64,
    local_quality: Option<&str>,
) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    let ts = now();
    let lesson: String =
        tx.query_row("SELECT lesson_id FROM downloads WHERE id = ?1", [id], |r| {
            r.get(0)
        })?;
    tx.execute(
        "UPDATE downloads SET status='done', progress=1, bytes_done=?2, bytes_total=?2, speed_bps=NULL, eta_sec=NULL,
                error_code=NULL, error_message=NULL, finished_at=?3, updated_at=?3 WHERE id=?1",
        params![id, size as i64, ts],
    )?;
    tx.execute(
        "UPDATE lessons SET local_video_path=?2, thumbnail_path=coalesce(?3, thumbnail_path), local_quality=?5,
                updated_at=?4 WHERE id=?1",
        params![lesson, video_path, thumb_path, ts, local_quality],
    )?;
    tx.commit()?;
    Ok(())
}

/// Records a failure. Retryable failures are scheduled with backoff until `MAX_ATTEMPTS`.
pub fn mark_error(
    conn: &Connection,
    id: &str,
    code: &str,
    message: &str,
    retryable: bool,
    at: DateTime<Utc>,
) -> AppResult<()> {
    let attempts: i32 =
        conn.query_row("SELECT attempts FROM downloads WHERE id = ?1", [id], |r| {
            r.get(0)
        })?;
    let next = (retryable && attempts < MAX_ATTEMPTS).then(|| {
        (at + chrono::Duration::seconds(backoff_secs(attempts)))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    });
    conn.execute(
        "UPDATE downloads SET status='error', error_code=?2, error_message=?3, next_retry_at=?4, speed_bps=NULL,
                eta_sec=NULL, updated_at=?5 WHERE id=?1",
        params![id, code, message, next, now()],
    )?;
    Ok(())
}

pub fn set_status(conn: &Connection, id: &str, status: DownloadStatus) -> AppResult<Download> {
    let n = conn.execute(
        "UPDATE downloads SET status=?2, speed_bps=NULL, eta_sec=NULL, updated_at=?3 WHERE id=?1",
        params![id, status, now()],
    )?;
    if n == 0 {
        return Err(AppError::not_found("download"));
    }
    get(conn, id)
}

/// Manual retry: resets attempts and queues immediately.
pub fn retry(conn: &Connection, id: &str) -> AppResult<Download> {
    conn.execute(
        "UPDATE downloads SET status='queued', attempts=0, next_retry_at=NULL, error_code=NULL, error_message=NULL,
                updated_at=?2 WHERE id=?1",
        params![id, now()],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM downloads WHERE id = ?1", [id])?;
    Ok(())
}

/// After a crash or quit, jobs that were running resume from their partial files.
pub fn requeue_interrupted(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE downloads SET status='queued', speed_bps=NULL, eta_sec=NULL, attempts=max(attempts-1, 0), updated_at=?1
         WHERE status='running'",
        [now()],
    )?)
}

/// Clears local media for lessons (paths and download rows); files are removed by the caller.
pub fn forget_local(conn: &Connection, lesson_ids: &[String]) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    for id in lesson_ids {
        tx.execute(
            "UPDATE lessons SET local_video_path=NULL, local_quality=NULL, thumbnail_path=NULL, updated_at=?2 WHERE id=?1",
            params![id, now()],
        )?;
        tx.execute("DELETE FROM downloads WHERE lesson_id = ?1", [id])?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::{annotations, import};
    use crate::services::import::{ImportedPlaylist, ImportedVideo};

    fn setup(n: usize) -> (Connection, String, Vec<String>) {
        let conn = open_in_memory().unwrap();
        let p = ImportedPlaylist {
            playlist_id: "PLtest1234567".into(),
            title: "C".into(),
            description: None,
            channel_title: None,
            thumbnail_url: None,
            videos: (0..n)
                .map(|i| ImportedVideo {
                    video_id: format!("vid{i:08}"),
                    title: format!("L{i}"),
                    description: None,
                    channel_title: None,
                    duration_sec: Some(600),
                    license: None,
                })
                .collect(),
            skipped: 0,
        };
        let (course, _) = import::import_playlist(&conn, &p).unwrap();
        let ids = crate::db::repo::lessons::list_course_lessons(&conn, &course)
            .unwrap()
            .into_iter()
            .map(|l| l.id)
            .collect();
        (conn, course, ids)
    }

    #[test]
    fn backoff_schedule() {
        assert_eq!(backoff_secs(1), 30);
        assert_eq!(backoff_secs(2), 60);
        assert_eq!(backoff_secs(3), 120);
        assert_eq!(backoff_secs(9), 600);
    }

    #[test]
    fn enqueue_is_idempotent_and_claims_in_outline_order() {
        let (conn, course, ids) = setup(3);
        assert_eq!(enqueue_course(&conn, &course, "720p").unwrap(), 3);
        assert_eq!(
            enqueue_course(&conn, &course, "720p").unwrap(),
            3,
            "re-enqueue is a no-op per lesson"
        );
        assert_eq!(list(&conn).unwrap().len(), 3);
        let t = Utc::now();
        let a = claim_next(&conn, t).unwrap().unwrap();
        let b = claim_next(&conn, t).unwrap().unwrap();
        assert_eq!(
            (a.lesson_id.as_str(), b.lesson_id.as_str()),
            (ids[0].as_str(), ids[1].as_str())
        );
        assert_eq!(a.status, DownloadStatus::Running);
        assert_eq!(a.attempts, 1);
        assert_eq!(
            list(&conn).unwrap()[0].status,
            DownloadStatus::Running,
            "active first"
        );
    }

    #[test]
    fn retries_with_backoff_until_max_attempts() {
        let (conn, _c, ids) = setup(1);
        enqueue(&conn, &ids[0], "720p").unwrap();
        let t0 = Utc::now();
        let d = claim_next(&conn, t0).unwrap().unwrap();
        mark_error(&conn, &d.id, "http", "network", true, t0).unwrap();
        assert!(
            claim_next(&conn, t0).unwrap().is_none(),
            "not before the backoff"
        );
        let later = t0 + chrono::Duration::seconds(31);
        assert_eq!(claim_next(&conn, later).unwrap().unwrap().attempts, 2);
        mark_error(&conn, &d.id, "not_found", "removed", false, later).unwrap();
        assert!(
            claim_next(&conn, later + chrono::Duration::hours(1))
                .unwrap()
                .is_none(),
            "non-retryable"
        );
        assert_eq!(retry(&conn, &d.id).unwrap().status, DownloadStatus::Queued);
        assert_eq!(get(&conn, &d.id).unwrap().attempts, 0);
    }

    #[test]
    fn interrupted_jobs_resume_and_pause_is_respected() {
        let (conn, _c, ids) = setup(2);
        enqueue(&conn, &ids[0], "720p").unwrap();
        let p = enqueue(&conn, &ids[1], "720p").unwrap();
        let running = claim_next(&conn, Utc::now()).unwrap().unwrap();
        update_progress(
            &conn,
            &running.id,
            0.4,
            Some(400),
            Some(1000),
            Some(10.0),
            Some(60),
        )
        .unwrap();
        set_status(&conn, &p.id, DownloadStatus::Paused).unwrap();
        assert_eq!(requeue_interrupted(&conn).unwrap(), 1);
        let r = get(&conn, &running.id).unwrap();
        assert_eq!(r.status, DownloadStatus::Queued);
        assert_eq!(r.progress, 0.4, "progress kept for resume");
        assert_eq!(r.attempts, 0, "an interruption isn't a failed attempt");
        let next = claim_next(&conn, Utc::now()).unwrap().unwrap();
        assert_eq!(next.id, running.id, "paused jobs are skipped");
        assert!(claim_next(&conn, Utc::now()).unwrap().is_none());
    }

    #[test]
    fn done_sets_lesson_path_and_forget_keeps_notes() {
        let (conn, _c, ids) = setup(1);
        let d = enqueue(&conn, &ids[0], "720p").unwrap();
        claim_next(&conn, Utc::now()).unwrap();
        mark_done(
            &conn,
            &d.id,
            "/m/video.mp4",
            Some("/m/thumb.jpg"),
            5_000_000,
            Some("720p"),
        )
        .unwrap();
        let l = crate::db::repo::lessons::get_lesson(&conn, &ids[0]).unwrap();
        assert_eq!(l.local_video_path.as_deref(), Some("/m/video.mp4"));
        assert_eq!(l.local_quality.as_deref(), Some("720p"));
        assert_eq!(
            get(&conn, &d.id).unwrap().local_quality.as_deref(),
            Some("720p")
        );
        assert!(
            enqueue(&conn, &ids[0], "720p").is_ok(),
            "done + file present returns existing row"
        );
        annotations::create(
            &conn,
            &annotations::tests::new(&ids[0], AnnotationType::Note, 5.0, "keep me"),
        )
        .unwrap();
        forget_local(&conn, &ids).unwrap();
        assert!(
            crate::db::repo::lessons::get_lesson(&conn, &ids[0])
                .unwrap()
                .local_video_path
                .is_none()
        );
        assert!(list(&conn).unwrap().is_empty());
        assert_eq!(
            annotations::list_for_lesson(&conn, &ids[0]).unwrap().len(),
            1
        );
    }
}
