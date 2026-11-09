use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::*;
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};

const LESSON_SELECT: &str = "SELECT l.id, l.module_id, l.position, l.type, l.title, l.description, l.url,
        l.youtube_video_id, l.channel_title, l.license, l.duration_sec, l.local_video_path, l.thumbnail_path,
        l.completed_at, l.due_date, p.last_position_sec, l.local_quality, d.kind, d.page_count
   FROM lessons l
   JOIN modules m ON m.id = l.module_id
   LEFT JOIN watch_progress p ON p.lesson_id = l.id
   LEFT JOIN materials d ON l.type = 'reading' AND d.id = (SELECT x.id FROM materials x
        WHERE x.lesson_id = l.id AND x.solution_of IS NULL AND x.kind != 'video' ORDER BY x.position LIMIT 1)";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<Lesson> {
    Ok(Lesson {
        id: r.get(0)?,
        module_id: r.get(1)?,
        position: r.get(2)?,
        lesson_type: r.get(3)?,
        title: r.get(4)?,
        description: r.get(5)?,
        url: r.get(6)?,
        youtube_video_id: r.get(7)?,
        channel_title: r.get(8)?,
        license: r.get(9)?,
        duration_sec: r.get(10)?,
        local_video_path: r.get(11)?,
        thumbnail_path: r.get(12)?,
        completed_at: r.get(13)?,
        due_date: r.get(14)?,
        last_position_sec: r.get(15)?,
        local_quality: r.get(16)?,
        doc_kind: r.get(17)?,
        doc_pages: r.get(18)?,
    })
}

/// All lessons of a course in outline order (module position, then lesson position).
pub fn list_course_lessons(conn: &Connection, course_id: &str) -> AppResult<Vec<Lesson>> {
    let mut stmt = conn.prepare(&format!(
        "{LESSON_SELECT} WHERE m.course_id = ?1 ORDER BY m.position, l.position"
    ))?;
    Ok(stmt
        .query_map([course_id], map)?
        .collect::<Result<_, _>>()?)
}

pub fn get_lesson(conn: &Connection, id: &str) -> AppResult<Lesson> {
    conn.query_row(&format!("{LESSON_SELECT} WHERE l.id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("lesson"))
}

pub fn course_id_of_lesson(conn: &Connection, lesson_id: &str) -> AppResult<String> {
    conn.query_row(
        "SELECT m.course_id FROM lessons l JOIN modules m ON m.id = l.module_id WHERE l.id = ?1",
        [lesson_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("lesson"))
}

pub fn get_lesson_context(conn: &Connection, id: &str) -> AppResult<LessonContext> {
    let lesson = get_lesson(conn, id)?;
    let module = super::modules::get_module(conn, &lesson.module_id)?;
    let course = super::courses::get_course(conn, &module.course_id)?;
    let all = list_course_lessons(conn, &course.id)?;
    let idx = all.iter().position(|l| l.id == id).unwrap_or(0);
    Ok(LessonContext {
        prev_lesson_id: idx
            .checked_sub(1)
            .and_then(|i| all.get(i))
            .map(|l| l.id.clone()),
        next_lesson_id: all.get(idx + 1).map(|l| l.id.clone()),
        module_title: module.title,
        course,
        lesson,
    })
}

pub fn create_lesson(conn: &Connection, input: &NewLesson) -> AppResult<Lesson> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(AppError::invalid("lesson title is required"));
    }
    super::modules::get_module(conn, &input.module_id)?;
    let id = new_id();
    insert_lesson(
        conn,
        &InsertLesson {
            id: &id,
            module_id: &input.module_id,
            lesson_type: input.lesson_type,
            title,
            description: input.description.as_deref(),
            url: input.url.as_deref(),
            youtube_video_id: None,
            channel_title: None,
            license: None,
            duration_sec: None,
        },
    )?;
    get_lesson(conn, &id)
}

pub(crate) struct InsertLesson<'a> {
    pub id: &'a str,
    pub module_id: &'a str,
    pub lesson_type: LessonType,
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub url: Option<&'a str>,
    pub youtube_video_id: Option<&'a str>,
    pub channel_title: Option<&'a str>,
    pub license: Option<License>,
    pub duration_sec: Option<i32>,
}

/// Appends a lesson at the end of its module.
pub(crate) fn insert_lesson(conn: &Connection, l: &InsertLesson<'_>) -> AppResult<()> {
    let pos: i32 = conn.query_row(
        "SELECT coalesce(max(position) + 1, 0) FROM lessons WHERE module_id = ?1",
        [l.module_id],
        |r| r.get(0),
    )?;
    let ts = now();
    conn.execute(
        "INSERT INTO lessons (id, module_id, position, type, title, description, url, youtube_video_id,
                              channel_title, license, duration_sec, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)",
        params![
            l.id,
            l.module_id,
            pos,
            l.lesson_type,
            l.title,
            l.description,
            l.url,
            l.youtube_video_id,
            l.channel_title,
            l.license,
            l.duration_sec,
            ts
        ],
    )?;
    Ok(())
}

pub fn update_lesson(conn: &Connection, id: &str, u: &LessonUpdate) -> AppResult<Lesson> {
    let cur = get_lesson(conn, id)?;
    let title = match &u.title {
        Some(t) if t.trim().is_empty() => {
            return Err(AppError::invalid("lesson title is required"));
        }
        Some(t) => t.trim().to_string(),
        None => cur.title,
    };
    conn.execute(
        "UPDATE lessons SET title=?2, description=?3, url=?4, due_date=?5, updated_at=?6 WHERE id=?1",
        params![
            id,
            title,
            u.description.clone().or(cur.description),
            u.url.clone().or(cur.url),
            u.due_date.clone().or(cur.due_date),
            now()
        ],
    )?;
    get_lesson(conn, id)
}

pub fn delete_lesson(conn: &Connection, id: &str) -> AppResult<()> {
    let l = get_lesson(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM lessons WHERE id = ?1", [id])?;
    super::notebooks::delete_for(&tx, NotebookScope::Lesson, id)?;
    renumber(&tx, &l.module_id)?;
    tx.commit()?;
    Ok(())
}

pub fn set_completed(conn: &Connection, id: &str, completed: bool) -> AppResult<Lesson> {
    let ts = now();
    let n = conn.execute(
        "UPDATE lessons SET completed_at = CASE WHEN ?2 THEN coalesce(completed_at, ?3) ELSE NULL END,
                updated_at = ?3 WHERE id = ?1",
        params![id, completed, ts],
    )?;
    if n == 0 {
        return Err(AppError::not_found("lesson"));
    }
    conn.execute(
        "UPDATE plan_items SET done = ?2, updated_at = ?3 WHERE lesson_id = ?1",
        params![id, completed, ts],
    )?;
    get_lesson(conn, id)
}

/// Persists a full drag-and-drop layout for a course: for each module, the exact lesson order.
/// Every lesson of the course must appear exactly once, and only modules of this course may be used.
pub fn apply_layout(conn: &Connection, course_id: &str, layout: &[ModuleOrder]) -> AppResult<()> {
    let modules: HashSet<String> = super::modules::list_modules(conn, course_id)?
        .into_iter()
        .map(|m| m.id)
        .collect();
    let lessons: HashSet<String> = list_course_lessons(conn, course_id)?
        .into_iter()
        .map(|l| l.id)
        .collect();
    let mut seen = HashSet::new();
    for mo in layout {
        if !modules.contains(&mo.module_id) {
            return Err(AppError::invalid(
                "layout references a module outside this course",
            ));
        }
        for id in &mo.lesson_ids {
            if !lessons.contains(id) || !seen.insert(id.clone()) {
                return Err(AppError::invalid(
                    "layout must list each lesson of the course exactly once",
                ));
            }
        }
    }
    if seen.len() != lessons.len() {
        return Err(AppError::invalid("layout is missing lessons"));
    }
    let tx = conn.unchecked_transaction()?;
    let ts = now();
    for mo in layout {
        for (i, id) in mo.lesson_ids.iter().enumerate() {
            tx.execute(
                "UPDATE lessons SET module_id=?2, position=?3, updated_at=?4 WHERE id=?1",
                params![id, mo.module_id, i as i32, ts],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Rewrites positions of a module's lessons to 0..n, keeping their current order.
pub(crate) fn renumber(conn: &Connection, module_id: &str) -> AppResult<()> {
    let mut stmt =
        conn.prepare("SELECT id FROM lessons WHERE module_id = ?1 ORDER BY position, created_at")?;
    let ids: Vec<String> = stmt
        .query_map([module_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE lessons SET position=?2 WHERE id=?1",
            params![id, i as i32],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::{courses, modules};

    fn setup() -> (Connection, String, String, Vec<String>) {
        let conn = open_in_memory().unwrap();
        let c = courses::create_course(
            &conn,
            &NewCourse {
                title: "C".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let m = modules::create_module(&conn, &c.id, "Lectures").unwrap();
        let ids = (0..5)
            .map(|i| {
                create_lesson(
                    &conn,
                    &NewLesson {
                        module_id: m.id.clone(),
                        lesson_type: LessonType::Video,
                        title: format!("L{i}"),
                        description: None,
                        url: None,
                    },
                )
                .unwrap()
                .id
            })
            .collect();
        (conn, c.id, m.id, ids)
    }

    fn titles(conn: &Connection, course: &str) -> Vec<Vec<String>> {
        courses::get_course_detail(conn, course)
            .unwrap()
            .modules
            .into_iter()
            .map(|m| m.lessons.into_iter().map(|l| l.title).collect())
            .collect()
    }

    #[test]
    fn layout_moves_lessons_between_modules() {
        let (conn, course, m1, ids) = setup();
        let m2 = modules::create_module(&conn, &course, "Week 2").unwrap();
        apply_layout(
            &conn,
            &course,
            &[
                ModuleOrder {
                    module_id: m1.clone(),
                    lesson_ids: vec![ids[1].clone(), ids[0].clone()],
                },
                ModuleOrder {
                    module_id: m2.id.clone(),
                    lesson_ids: ids[2..].to_vec(),
                },
            ],
        )
        .unwrap();
        assert_eq!(
            titles(&conn, &course),
            vec![vec!["L1", "L0"], vec!["L2", "L3", "L4"]]
        );
        let ctx = get_lesson_context(&conn, &ids[0]).unwrap();
        assert_eq!(ctx.prev_lesson_id.as_deref(), Some(ids[1].as_str()));
        assert_eq!(ctx.next_lesson_id.as_deref(), Some(ids[2].as_str()));
    }

    #[test]
    fn layout_rejects_missing_or_duplicate_lessons() {
        let (conn, course, m1, ids) = setup();
        let missing = [ModuleOrder {
            module_id: m1.clone(),
            lesson_ids: ids[..4].to_vec(),
        }];
        assert!(apply_layout(&conn, &course, &missing).is_err());
        let mut dup = ids.clone();
        dup[4] = ids[0].clone();
        assert!(
            apply_layout(
                &conn,
                &course,
                &[ModuleOrder {
                    module_id: m1,
                    lesson_ids: dup
                }]
            )
            .is_err()
        );
    }

    #[test]
    fn delete_module_moves_lessons_to_neighbour() {
        let (conn, course, m1, ids) = setup();
        let m2 = modules::create_module(&conn, &course, "Week 2").unwrap();
        apply_layout(
            &conn,
            &course,
            &[
                ModuleOrder {
                    module_id: m1.clone(),
                    lesson_ids: ids[..2].to_vec(),
                },
                ModuleOrder {
                    module_id: m2.id.clone(),
                    lesson_ids: ids[2..].to_vec(),
                },
            ],
        )
        .unwrap();
        modules::delete_module(&conn, &m2.id).unwrap();
        assert_eq!(
            titles(&conn, &course),
            vec![vec!["L0", "L1", "L2", "L3", "L4"]]
        );
        assert!(modules::delete_module(&conn, &m1).is_err());
    }

    #[test]
    fn delete_lesson_renumbers_and_completion_toggles() {
        let (conn, course, _m, ids) = setup();
        delete_lesson(&conn, &ids[1]).unwrap();
        let d = courses::get_course_detail(&conn, &course).unwrap();
        let pos: Vec<i32> = d.modules[0].lessons.iter().map(|l| l.position).collect();
        assert_eq!(pos, vec![0, 1, 2, 3]);
        assert!(
            set_completed(&conn, &ids[0], true)
                .unwrap()
                .completed_at
                .is_some()
        );
        assert!(
            set_completed(&conn, &ids[0], false)
                .unwrap()
                .completed_at
                .is_none()
        );
    }
}
