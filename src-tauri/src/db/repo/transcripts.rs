//! Transcript cues and chapters for lessons.

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::{Chapter, Transcript, TranscriptCue};
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};
use crate::services::vtt::Cue;
use crate::services::ytdlp::ChapterInfo;

pub fn get_transcript(conn: &Connection, lesson_id: &str) -> AppResult<Transcript> {
    let (source, lang, fetched_at): (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT transcript_source, transcript_lang, transcript_fetched_at FROM lessons WHERE id = ?1",
            [lesson_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("lesson"))?;
    let mut stmt =
        conn.prepare("SELECT start_sec, end_sec, text FROM transcript_cues WHERE lesson_id = ?1 ORDER BY start_sec, id")?;
    let cues = stmt
        .query_map([lesson_id], |r| {
            Ok(TranscriptCue {
                start: r.get(0)?,
                end: r.get(1)?,
                text: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Transcript {
        lesson_id: lesson_id.into(),
        source,
        lang,
        fetched_at,
        cues,
    })
}

/// Replaces a lesson's imported chapters (user chapters are kept), leaving the transcript alone.
pub fn set_chapters(conn: &Connection, lesson_id: &str, chapters: &[ChapterInfo]) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    let ts = now();
    tx.execute(
        "DELETE FROM chapters WHERE lesson_id = ?1 AND source = 'youtube'",
        [lesson_id],
    )?;
    {
        let mut ins = tx.prepare(
            "INSERT INTO chapters (id, lesson_id, start_sec, end_sec, title, source, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'youtube', ?6, ?6)",
        )?;
        for c in chapters {
            ins.execute(params![new_id(), lesson_id, c.start, c.end, c.title, ts])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Replaces a lesson's transcript and YouTube chapters in one transaction.
/// User-defined chapters are kept. `source` is `manual`, `auto` or `none`.
pub fn replace(
    conn: &Connection,
    lesson_id: &str,
    source: &str,
    lang: Option<&str>,
    cues: &[Cue],
    chapters: &[ChapterInfo],
) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    let ts = now();
    tx.execute(
        "DELETE FROM transcript_cues WHERE lesson_id = ?1",
        [lesson_id],
    )?;
    {
        let mut ins = tx.prepare(
            "INSERT INTO transcript_cues (lesson_id, lang, start_sec, end_sec, text, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        )?;
        for c in cues {
            ins.execute(params![
                lesson_id,
                lang.unwrap_or(""),
                c.start,
                c.end,
                c.text,
                ts
            ])?;
        }
    }
    if !chapters.is_empty() {
        tx.execute(
            "DELETE FROM chapters WHERE lesson_id = ?1 AND source = 'youtube'",
            [lesson_id],
        )?;
        let mut ins = tx.prepare(
            "INSERT INTO chapters (id, lesson_id, start_sec, end_sec, title, source, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'youtube', ?6, ?6)",
        )?;
        for c in chapters {
            ins.execute(params![new_id(), lesson_id, c.start, c.end, c.title, ts])?;
        }
    }
    let n = tx.execute(
        "UPDATE lessons SET transcript_source = ?2, transcript_lang = ?3, transcript_fetched_at = ?4 WHERE id = ?1",
        params![lesson_id, source, lang, ts],
    )?;
    if n == 0 {
        return Err(AppError::not_found("lesson"));
    }
    tx.commit()?;
    Ok(())
}

pub fn get_chapters(conn: &Connection, lesson_id: &str) -> AppResult<Vec<Chapter>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_sec, end_sec, title, source FROM chapters WHERE lesson_id = ?1 ORDER BY start_sec",
    )?;
    Ok(stmt
        .query_map([lesson_id], |r| {
            Ok(Chapter {
                id: r.get(0)?,
                start_sec: r.get(1)?,
                end_sec: r.get(2)?,
                title: r.get(3)?,
                source: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::annotations::tests::lesson;

    fn cue(start: f64, text: &str) -> Cue {
        Cue {
            start,
            end: start + 2.0,
            text: text.into(),
        }
    }

    #[test]
    fn replace_is_idempotent_and_indexed() {
        let conn = open_in_memory().unwrap();
        let (_, l) = lesson(&conn);
        assert_eq!(get_transcript(&conn, &l).unwrap().source, None);
        let chapters = vec![ChapterInfo {
            start: 0.0,
            end: Some(478.0),
            title: "Intro".into(),
        }];
        replace(
            &conn,
            &l,
            "auto",
            Some("en-orig"),
            &[cue(1.0, "hello"), cue(3.0, "mapreduce is great")],
            &chapters,
        )
        .unwrap();
        replace(
            &conn,
            &l,
            "auto",
            Some("en-orig"),
            &[cue(1.0, "hello"), cue(3.0, "mapreduce is great")],
            &chapters,
        )
        .unwrap();
        let t = get_transcript(&conn, &l).unwrap();
        assert_eq!(t.cues.len(), 2);
        assert_eq!(t.source.as_deref(), Some("auto"));
        assert_eq!(get_chapters(&conn, &l).unwrap().len(), 1);
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM transcript_fts WHERE transcript_fts MATCH 'mapreduce'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "old cues are removed from the index");
    }

    #[test]
    fn records_none_when_unavailable() {
        let conn = open_in_memory().unwrap();
        let (_, l) = lesson(&conn);
        replace(&conn, &l, "none", None, &[], &[]).unwrap();
        let t = get_transcript(&conn, &l).unwrap();
        assert_eq!(t.source.as_deref(), Some("none"));
        assert!(t.fetched_at.is_some());
    }
}
