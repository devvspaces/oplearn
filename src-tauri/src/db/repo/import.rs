//! Turning imported playlists/videos into courses, modules and lessons.

use std::collections::HashSet;

use rusqlite::{Connection, params};

use crate::db::models::*;
use crate::db::new_id;
use crate::db::repo::{courses, lessons, modules};
use crate::error::{AppError, AppResult};
use crate::services::import::{ImportedPlaylist, ImportedVideo};

fn video_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

fn insert_video(conn: &Connection, module_id: &str, v: &ImportedVideo) -> AppResult<String> {
    let id = new_id();
    let url = video_url(&v.video_id);
    lessons::insert_lesson(
        conn,
        &lessons::InsertLesson {
            id: &id,
            module_id,
            lesson_type: LessonType::Video,
            title: &v.title,
            description: v.description.as_deref(),
            url: Some(&url),
            youtube_video_id: Some(&v.video_id),
            channel_title: v.channel_title.as_deref(),
            license: v.license,
            duration_sec: v.duration_sec,
        },
    )?;
    Ok(id)
}

/// Creates a course with one "Lectures" module holding one lesson per video, in playlist order.
/// Runs in a single transaction. Returns `(course_id, added)`.
pub fn import_playlist(conn: &Connection, p: &ImportedPlaylist) -> AppResult<(String, i32)> {
    let tx = conn.unchecked_transaction()?;
    let course = courses::create_course(
        &tx,
        &NewCourse {
            title: p.title.clone(),
            description: p.description.clone(),
            instructor: p.channel_title.clone(),
            source_url: Some(format!(
                "https://www.youtube.com/playlist?list={}",
                p.playlist_id
            )),
            cover_path: p.thumbnail_url.clone(),
            tags: vec![],
        },
        Some(&p.playlist_id),
    )?;
    let module = modules::create_module(&tx, &course.id, "Lectures")?;
    let mut seen = HashSet::new();
    let mut added = 0;
    for v in &p.videos {
        if seen.insert(v.video_id.clone()) {
            insert_video(&tx, &module.id, v)?;
            added += 1;
        }
    }
    tx.commit()?;
    Ok((course.id, added))
}

/// Appends videos that are in the playlist but not yet in the course to the last module.
/// Existing lessons (and their notes/progress) are never removed or reordered;
/// missing metadata (duration, license) is filled in.
pub fn resync_playlist(conn: &Connection, course_id: &str, p: &ImportedPlaylist) -> AppResult<i32> {
    let course = courses::get_course(conn, course_id)?;
    if course.youtube_playlist_id.as_deref() != Some(p.playlist_id.as_str()) {
        return Err(AppError::invalid("course is not linked to this playlist"));
    }
    let tx = conn.unchecked_transaction()?;
    let existing: HashSet<String> = lessons::list_course_lessons(&tx, course_id)?
        .into_iter()
        .filter_map(|l| l.youtube_video_id)
        .collect();
    let mods = modules::list_modules(&tx, course_id)?;
    let target = match mods.last() {
        Some(m) => m.id.clone(),
        None => modules::create_module(&tx, course_id, "Lectures")?.id,
    };
    let mut added = 0;
    let mut seen = HashSet::new();
    for v in &p.videos {
        if existing.contains(&v.video_id) {
            tx.execute(
                "UPDATE lessons SET duration_sec = coalesce(duration_sec, ?2), license = coalesce(license, ?3)
                 WHERE youtube_video_id = ?1 AND module_id IN (SELECT id FROM modules WHERE course_id = ?4)",
                params![v.video_id, v.duration_sec, v.license, course_id],
            )?;
        } else if seen.insert(v.video_id.clone()) {
            insert_video(&tx, &target, v)?;
            added += 1;
        }
    }
    courses::touch_course(&tx, course_id)?;
    tx.commit()?;
    Ok(added)
}

/// Adds a single video to a module. Refuses duplicates within the same course.
pub fn add_video(conn: &Connection, module_id: &str, v: &ImportedVideo) -> AppResult<Lesson> {
    let m = modules::get_module(conn, module_id)?;
    let dup = lessons::list_course_lessons(conn, &m.course_id)?
        .iter()
        .any(|l| l.youtube_video_id.as_deref() == Some(v.video_id.as_str()));
    if dup {
        return Err(AppError::invalid("this video is already in the course"));
    }
    let id = insert_video(conn, module_id, v)?;
    courses::touch_course(conn, &m.course_id)?;
    lessons::get_lesson(conn, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn video(i: usize) -> ImportedVideo {
        ImportedVideo {
            video_id: format!("vid{i:08}"),
            title: format!("Lecture {}", i + 1),
            description: None,
            channel_title: Some("MIT".into()),
            duration_sec: Some(4800),
            license: Some(License::CreativeCommon),
        }
    }

    fn playlist(n: usize) -> ImportedPlaylist {
        ImportedPlaylist {
            playlist_id: "PLtest1234567".into(),
            title: "6.824 Distributed Systems".into(),
            description: Some("Spring 2020".into()),
            channel_title: Some("MIT".into()),
            thumbnail_url: None,
            videos: (0..n).map(video).collect(),
            skipped: 0,
        }
    }

    #[test]
    fn import_preserves_playlist_order_and_attribution() {
        let conn = open_in_memory().unwrap();
        let (course_id, added) = import_playlist(&conn, &playlist(20)).unwrap();
        assert_eq!(added, 20);
        let d = courses::get_course_detail(&conn, &course_id).unwrap();
        assert_eq!(
            d.course.youtube_playlist_id.as_deref(),
            Some("PLtest1234567")
        );
        assert_eq!(d.modules.len(), 1);
        assert_eq!(d.modules[0].module.title, "Lectures");
        let titles: Vec<_> = d.modules[0]
            .lessons
            .iter()
            .map(|l| l.title.clone())
            .collect();
        let expected: Vec<_> = (1..=20).map(|i| format!("Lecture {i}")).collect();
        assert_eq!(titles, expected);
        let l = &d.modules[0].lessons[0];
        assert_eq!(l.channel_title.as_deref(), Some("MIT"));
        assert_eq!(l.license, Some(License::CreativeCommon));
        assert_eq!(
            l.url.as_deref(),
            Some("https://www.youtube.com/watch?v=vid00000000")
        );
    }

    #[test]
    fn resync_appends_only_new_videos_and_keeps_layout() {
        let conn = open_in_memory().unwrap();
        let (course_id, _) = import_playlist(&conn, &playlist(3)).unwrap();
        let first = courses::get_course_detail(&conn, &course_id)
            .unwrap()
            .modules[0]
            .lessons[0]
            .id
            .clone();
        crate::db::repo::progress::save_position(&conn, &first, 120.0).unwrap();
        let w2 = modules::create_module(&conn, &course_id, "Week 2").unwrap();
        assert_eq!(resync_playlist(&conn, &course_id, &playlist(5)).unwrap(), 2);
        assert_eq!(
            resync_playlist(&conn, &course_id, &playlist(5)).unwrap(),
            0,
            "idempotent"
        );
        let d = courses::get_course_detail(&conn, &course_id).unwrap();
        assert_eq!(d.modules[0].lessons.len(), 3);
        assert_eq!(d.modules[1].module.id, w2.id);
        assert_eq!(d.modules[1].lessons.len(), 2);
        assert_eq!(d.modules[0].lessons[0].last_position_sec, Some(120.0));
    }

    #[test]
    fn add_video_rejects_duplicates() {
        let conn = open_in_memory().unwrap();
        let (course_id, _) = import_playlist(&conn, &playlist(2)).unwrap();
        let m = &courses::get_course_detail(&conn, &course_id)
            .unwrap()
            .modules[0];
        assert!(add_video(&conn, &m.module.id, &video(0)).is_err());
        let l = add_video(&conn, &m.module.id, &video(7)).unwrap();
        assert_eq!(l.position, 2);
    }
}
