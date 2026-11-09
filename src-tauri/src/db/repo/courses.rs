use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::models::*;
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};
use crate::paths::slugify;

pub(crate) const COURSE_COLS: &str = "id, title, slug, description, instructor, source_url, youtube_playlist_id, \
     cover_path, tags_json, mode, status, playback_rate, created_at, updated_at";

pub(crate) fn map_course(r: &Row<'_>) -> rusqlite::Result<Course> {
    let tags_json: String = r.get(8)?;
    Ok(Course {
        id: r.get(0)?,
        title: r.get(1)?,
        slug: r.get(2)?,
        description: r.get(3)?,
        instructor: r.get(4)?,
        source_url: r.get(5)?,
        youtube_playlist_id: r.get(6)?,
        cover_path: r.get(7)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        mode: r.get(9)?,
        status: r.get(10)?,
        playback_rate: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
    })
}

fn unique_slug(conn: &Connection, title: &str) -> AppResult<String> {
    let base = slugify(title);
    let mut slug = base.clone();
    let mut n = 2;
    while conn
        .query_row("SELECT 1 FROM courses WHERE slug = ?1", [&slug], |_| Ok(()))
        .optional()?
        .is_some()
    {
        slug = format!("{base}-{n}");
        n += 1;
    }
    Ok(slug)
}

pub fn create_course(
    conn: &Connection,
    input: &NewCourse,
    playlist_id: Option<&str>,
) -> AppResult<Course> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(AppError::invalid("course title is required"));
    }
    let id = new_id();
    let ts = now();
    let slug = unique_slug(conn, title)?;
    conn.execute(
        "INSERT INTO courses (id, title, slug, description, instructor, source_url, youtube_playlist_id,
                              cover_path, tags_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            id,
            title,
            slug,
            input.description,
            input.instructor,
            input.source_url,
            playlist_id,
            input.cover_path,
            serde_json::to_string(&input.tags)?,
            ts
        ],
    )?;
    get_course(conn, &id)
}

pub fn get_course(conn: &Connection, id: &str) -> AppResult<Course> {
    conn.query_row(
        &format!("SELECT {COURSE_COLS} FROM courses WHERE id = ?1"),
        [id],
        map_course,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("course"))
}

pub fn list_course_summaries(conn: &Connection) -> AppResult<Vec<CourseSummary>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COURSE_COLS},
           (SELECT count(*) FROM lessons l JOIN modules m ON m.id = l.module_id WHERE m.course_id = c.id),
           (SELECT count(*) FROM lessons l JOIN modules m ON m.id = l.module_id
              WHERE m.course_id = c.id AND l.completed_at IS NOT NULL),
           (SELECT coalesce(sum(l.duration_sec), 0) FROM lessons l JOIN modules m ON m.id = l.module_id
              WHERE m.course_id = c.id),
           (SELECT max(p.updated_at) FROM watch_progress p JOIN lessons l ON l.id = p.lesson_id
              JOIN modules m ON m.id = l.module_id WHERE m.course_id = c.id)
         FROM courses c ORDER BY c.updated_at DESC"
    ))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                map_course(r)?,
                r.get::<_, i32>(14)?,
                r.get::<_, i32>(15)?,
                r.get::<_, i32>(16)?,
                r.get(17)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(course, lesson_count, completed_count, total_duration_sec, last_studied_at)| {
                let next = next_lesson(conn, &course.id)?;
                Ok(CourseSummary {
                    lesson_count,
                    completed_count,
                    total_duration_sec,
                    next_lesson_id: next.as_ref().map(|n| n.0.clone()),
                    next_lesson_title: next.map(|n| n.1),
                    last_studied_at,
                    course,
                })
            },
        )
        .collect()
}

/// The lesson to "Continue" with: the most recently watched incomplete lesson,
/// otherwise the first incomplete lesson in outline order.
pub fn next_lesson(conn: &Connection, course_id: &str) -> AppResult<Option<(String, String)>> {
    let recent = conn
        .query_row(
            "SELECT l.id, l.title FROM lessons l JOIN modules m ON m.id = l.module_id
             JOIN watch_progress p ON p.lesson_id = l.id
             WHERE m.course_id = ?1 AND l.completed_at IS NULL
             ORDER BY p.updated_at DESC LIMIT 1",
            [course_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if recent.is_some() {
        return Ok(recent);
    }
    Ok(conn
        .query_row(
            "SELECT l.id, l.title FROM lessons l JOIN modules m ON m.id = l.module_id
             WHERE m.course_id = ?1 AND l.completed_at IS NULL
             ORDER BY m.position, l.position LIMIT 1",
            [course_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

pub fn get_course_detail(conn: &Connection, id: &str) -> AppResult<CourseDetail> {
    let course = get_course(conn, id)?;
    let modules = super::modules::list_modules(conn, id)?;
    let mut lessons = super::lessons::list_course_lessons(conn, id)?;
    let modules = modules
        .into_iter()
        .map(|module| {
            let (mine, rest): (Vec<_>, Vec<_>) =
                lessons.drain(..).partition(|l| l.module_id == module.id);
            lessons = rest;
            ModuleWithLessons {
                module,
                lessons: mine,
            }
        })
        .collect();
    Ok(CourseDetail { course, modules })
}

/// `Some("")` clears an optional text field; `None` leaves it unchanged.
fn non_empty(v: &str) -> Option<String> {
    let t = v.trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub fn update_course(conn: &Connection, id: &str, u: &CourseUpdate) -> AppResult<Course> {
    let mut c = get_course(conn, id)?;
    if let Some(t) = &u.title {
        let t = t.trim();
        if t.is_empty() {
            return Err(AppError::invalid("course title is required"));
        }
        c.title = t.to_string();
    }
    if let Some(v) = &u.description {
        c.description = non_empty(v);
    }
    if let Some(v) = &u.instructor {
        c.instructor = non_empty(v);
    }
    if let Some(v) = &u.source_url {
        c.source_url = non_empty(v);
    }
    if let Some(v) = &u.cover_path {
        c.cover_path = non_empty(v);
    }
    if let Some(tags) = &u.tags {
        c.tags = tags.clone();
    }
    if let Some(s) = u.status {
        c.status = s;
    }
    if let Some(m) = u.mode {
        c.mode = m;
    }
    if let Some(rate) = u.playback_rate {
        if !(0.25..=4.0).contains(&rate) {
            return Err(AppError::invalid("playback rate out of range"));
        }
        c.playback_rate = rate;
    }
    conn.execute(
        "UPDATE courses SET title=?2, description=?3, instructor=?4, source_url=?5, cover_path=?6,
                tags_json=?7, status=?8, mode=?9, playback_rate=?10, updated_at=?11 WHERE id=?1",
        params![
            id,
            c.title,
            c.description,
            c.instructor,
            c.source_url,
            c.cover_path,
            serde_json::to_string(&c.tags)?,
            c.status,
            c.mode,
            c.playback_rate,
            now()
        ],
    )?;
    get_course(conn, id)
}

pub fn delete_course(conn: &Connection, id: &str) -> AppResult<Course> {
    let c = get_course(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    // Notebooks have no foreign key (they can be scoped to a course or a lesson).
    tx.execute(
        "DELETE FROM notebooks WHERE (scope = 'course' AND scope_id = ?1) OR (scope = 'lesson' AND scope_id IN
           (SELECT l.id FROM lessons l JOIN modules m ON m.id = l.module_id WHERE m.course_id = ?1))",
        [id],
    )?;
    tx.execute("DELETE FROM courses WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(c)
}

pub fn touch_course(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE courses SET updated_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn create_and_update_course() {
        let conn = open_in_memory().unwrap();
        let c = create_course(
            &conn,
            &NewCourse {
                title: "Distributed Systems".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(c.slug, "distributed-systems");
        let c2 = create_course(
            &conn,
            &NewCourse {
                title: "Distributed Systems".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(c2.slug, "distributed-systems-2");
        let u = update_course(
            &conn,
            &c.id,
            &CourseUpdate {
                playback_rate: Some(1.5),
                tags: Some(vec!["mit".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(u.playback_rate, 1.5);
        assert_eq!(u.tags, vec!["mit".to_string()]);
        assert!(
            create_course(
                &conn,
                &NewCourse {
                    title: "  ".into(),
                    ..Default::default()
                },
                None
            )
            .is_err()
        );
    }
}
