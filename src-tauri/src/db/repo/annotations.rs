//! Notes, bookmarks, questions, to-dos and references attached to lessons (SPEC §7.5).

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::models::*;
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};

const COLS: &str = "a.id, a.lesson_id, a.type, a.start_sec, a.end_sec, a.title, a.body_md, a.status, a.priority, \
     a.due_date, a.url, a.ref_type, a.answer_md, a.tags_json, a.ai_generated, a.created_at, a.updated_at, a.material_id, a.page";

pub const QUESTION_STATUSES: &[&str] = &["open", "researching", "answered"];
pub const TODO_STATUSES: &[&str] = &["open", "in_progress", "done"];
pub const PRIORITIES: &[&str] = &["low", "medium", "high"];
pub const REF_TYPES: &[&str] = &["paper", "article", "repo", "book", "video", "other"];

fn map(r: &Row<'_>) -> rusqlite::Result<Annotation> {
    let tags: String = r.get(13)?;
    Ok(Annotation {
        id: r.get(0)?,
        lesson_id: r.get(1)?,
        kind: r.get(2)?,
        start_sec: r.get(3)?,
        end_sec: r.get(4)?,
        title: r.get(5)?,
        body_md: r.get(6)?,
        status: r.get(7)?,
        priority: r.get(8)?,
        due_date: r.get(9)?,
        url: r.get(10)?,
        ref_type: r.get(11)?,
        answer_md: r.get(12)?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        ai_generated: r.get(14)?,
        created_at: r.get(15)?,
        updated_at: r.get(16)?,
        material_id: r.get(17)?,
        page: r.get(18)?,
    })
}

fn non_empty(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn one_of(field: &str, v: &Option<String>, allowed: &[&str]) -> AppResult<()> {
    match v {
        Some(s) if !allowed.contains(&s.as_str()) => Err(AppError::invalid(format!(
            "{field} must be one of: {}",
            allowed.join(", ")
        ))),
        _ => Ok(()),
    }
}

fn validate(a: &Annotation) -> AppResult<()> {
    for (name, v) in [("start", a.start_sec), ("end", a.end_sec)] {
        if let Some(x) = v
            && (!x.is_finite() || x < 0.0)
        {
            return Err(AppError::invalid(format!(
                "{name} time must be a non-negative number"
            )));
        }
    }
    if let (Some(s), Some(e)) = (a.start_sec, a.end_sec)
        && e < s
    {
        return Err(AppError::invalid("end time must not be before start time"));
    }
    match a.kind {
        AnnotationType::Question => one_of("status", &a.status, QUESTION_STATUSES)?,
        AnnotationType::Todo => {
            one_of("status", &a.status, TODO_STATUSES)?;
            one_of("priority", &a.priority, PRIORITIES)?;
        }
        AnnotationType::Reference => one_of("reference type", &a.ref_type, REF_TYPES)?,
        _ => {}
    }
    if let Some(d) = &a.due_date
        && chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err()
    {
        return Err(AppError::invalid("due date must be YYYY-MM-DD"));
    }
    match (&a.material_id, a.page) {
        (None, Some(_)) => return Err(AppError::invalid("a page needs a material")),
        (_, Some(p)) if p < 1 => return Err(AppError::invalid("page must be 1 or more")),
        _ => {}
    }
    Ok(())
}

/// A page anchor must name an existing material of the same course, within its page count.
fn check_page(conn: &Connection, a: &Annotation) -> AppResult<()> {
    let Some(mid) = &a.material_id else {
        return Ok(());
    };
    let (pages,): (Option<i32>,) = conn
        .query_row(
            "SELECT mt.page_count FROM materials mt
               JOIN lessons l ON l.id = ?2 JOIN modules m ON m.id = l.module_id
              WHERE mt.id = ?1 AND mt.course_id = m.course_id",
            params![mid, a.lesson_id],
            |r| Ok((r.get(0)?,)),
        )
        .optional()?
        .ok_or_else(|| AppError::invalid("that document isn't part of this course"))?;
    if let (Some(n), Some(p)) = (pages, a.page)
        && p > n
    {
        return Err(AppError::invalid(format!("page must be between 1 and {n}")));
    }
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Annotation> {
    conn.query_row(
        &format!("SELECT {COLS} FROM annotations a WHERE a.id = ?1"),
        [id],
        map,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("annotation"))
}

pub fn list_for_lesson(conn: &Connection, lesson_id: &str) -> AppResult<Vec<Annotation>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM annotations a WHERE a.lesson_id = ?1
         ORDER BY a.start_sec IS NULL, a.start_sec, a.material_id IS NULL, a.material_id, a.page, a.created_at"
    ))?;
    Ok(stmt
        .query_map([lesson_id], map)?
        .collect::<Result<_, _>>()?)
}

/// All annotations of one type across courses (global Questions / To-dos / References views),
/// open items first, then newest.
pub fn list_by_type(
    conn: &Connection,
    kind: AnnotationType,
) -> AppResult<Vec<AnnotationWithCourse>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS}, l.title, c.id, c.title
         FROM annotations a JOIN lessons l ON l.id = a.lesson_id JOIN modules m ON m.id = l.module_id
         JOIN courses c ON c.id = m.course_id
         WHERE a.type = ?1
         ORDER BY coalesce(a.status, 'open') IN ('done', 'answered'), a.due_date IS NULL, a.due_date, a.created_at DESC"
    ))?;
    Ok(stmt
        .query_map([kind], |r| {
            Ok(AnnotationWithCourse {
                annotation: map(r)?,
                lesson_title: r.get(19)?,
                course_id: r.get(20)?,
                course_title: r.get(21)?,
            })
        })?
        .collect::<Result<_, _>>()?)
}

/// Every annotation in a course, in outline order (module, lesson, timestamp).
pub fn list_for_course(conn: &Connection, course_id: &str) -> AppResult<Vec<AnnotationInContext>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS}, l.title, l.youtube_video_id, m.id, m.title
         FROM annotations a JOIN lessons l ON l.id = a.lesson_id JOIN modules m ON m.id = l.module_id
         WHERE m.course_id = ?1
         ORDER BY m.position, l.position, a.start_sec IS NULL, a.start_sec, a.material_id IS NULL, a.material_id, a.page, a.created_at"
    ))?;
    Ok(stmt
        .query_map([course_id], |r| {
            Ok(AnnotationInContext {
                annotation: map(r)?,
                lesson_title: r.get(19)?,
                youtube_video_id: r.get(20)?,
                module_id: r.get(21)?,
                module_title: r.get(22)?,
            })
        })?
        .collect::<Result<_, _>>()?)
}

pub fn create(conn: &Connection, n: &NewAnnotation) -> AppResult<Annotation> {
    create_with(conn, n, false)
}

/// Creates an annotation, marking it AI-generated when `ai` is set.
pub fn create_with(conn: &Connection, n: &NewAnnotation, ai: bool) -> AppResult<Annotation> {
    super::lessons::get_lesson(conn, &n.lesson_id)?;
    let ts = now();
    let default_status = match n.kind {
        AnnotationType::Question | AnnotationType::Todo => Some("open".to_string()),
        _ => None,
    };
    let a = Annotation {
        id: new_id(),
        lesson_id: n.lesson_id.clone(),
        kind: n.kind,
        start_sec: n.start_sec,
        end_sec: n.end_sec,
        title: non_empty(n.title.as_deref()),
        body_md: non_empty(n.body_md.as_deref()),
        status: non_empty(n.status.as_deref()).or(default_status),
        priority: non_empty(n.priority.as_deref()),
        due_date: non_empty(n.due_date.as_deref()),
        url: non_empty(n.url.as_deref()),
        ref_type: non_empty(n.ref_type.as_deref()),
        answer_md: non_empty(n.answer_md.as_deref()),
        tags: n.tags.clone().unwrap_or_default(),
        ai_generated: ai,
        created_at: ts.clone(),
        updated_at: ts,
        material_id: non_empty(n.material_id.as_deref()),
        page: n.page,
    };
    validate(&a)?;
    check_page(conn, &a)?;
    write(conn, &a, true)?;
    get(conn, &a.id)
}

pub fn update(conn: &Connection, id: &str, u: &AnnotationUpdate) -> AppResult<Annotation> {
    let mut a = get(conn, id)?;
    if u.start_sec.is_some() {
        a.start_sec = u.start_sec;
    }
    if u.page.is_some() {
        a.page = u.page;
    }
    macro_rules! text {
        ($($f:ident),*) => { $( if let Some(v) = &u.$f { a.$f = non_empty(Some(v)); } )* };
    }
    text!(
        title, body_md, status, priority, due_date, url, ref_type, answer_md
    );
    if let Some(t) = &u.tags {
        a.tags = t.clone();
    }
    // An answer marks a question answered unless the caller set a status explicitly.
    if a.kind == AnnotationType::Question
        && u.answer_md.is_some()
        && u.status.is_none()
        && a.answer_md.is_some()
    {
        a.status = Some("answered".into());
    }
    a.updated_at = now();
    validate(&a)?;
    check_page(conn, &a)?;
    write(conn, &a, false)?;
    get(conn, id)
}

/// Stores an AI answer on a question: sets the answer, marks it answered and AI-generated.
pub fn set_ai_answer(conn: &Connection, id: &str, answer_md: &str) -> AppResult<Annotation> {
    let a = get(conn, id)?;
    if a.kind != AnnotationType::Question {
        return Err(AppError::invalid("only questions can get an AI answer"));
    }
    let answer = answer_md.trim();
    if answer.is_empty() {
        return Err(AppError::invalid("the answer is empty"));
    }
    conn.execute(
        "UPDATE annotations SET answer_md = ?2, status = 'answered', ai_generated = 1, updated_at = ?3 WHERE id = ?1",
        params![id, answer, now()],
    )?;
    get(conn, id)
}

fn write(conn: &Connection, a: &Annotation, insert: bool) -> AppResult<()> {
    let tags = serde_json::to_string(&a.tags)?;
    if insert {
        conn.execute(
            "INSERT INTO annotations (id, rid, lesson_id, type, start_sec, end_sec, title, body_md, status, priority,
                 due_date, url, ref_type, answer_md, tags_json, ai_generated, created_at, updated_at, material_id, page)
             VALUES (?1, (SELECT coalesce(max(rid), 0) + 1 FROM annotations), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                 ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                a.id, a.lesson_id, a.kind, a.start_sec, a.end_sec, a.title, a.body_md, a.status, a.priority,
                a.due_date, a.url, a.ref_type, a.answer_md, tags, a.ai_generated, a.created_at, a.updated_at,
                a.material_id, a.page
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE annotations SET start_sec=?2, end_sec=?3, title=?4, body_md=?5, status=?6, priority=?7,
                 due_date=?8, url=?9, ref_type=?10, answer_md=?11, tags_json=?12, updated_at=?13, page=?14 WHERE id=?1",
            params![
                a.id, a.start_sec, a.end_sec, a.title, a.body_md, a.status, a.priority, a.due_date, a.url,
                a.ref_type, a.answer_md, tags, a.updated_at, a.page
            ],
        )?;
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    if conn.execute("DELETE FROM annotations WHERE id = ?1", [id])? == 0 {
        return Err(AppError::not_found("annotation"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::{courses, lessons, modules};

    pub(crate) fn lesson(conn: &Connection) -> (String, String) {
        let c = courses::create_course(
            conn,
            &NewCourse {
                title: "C".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let m = modules::create_module(conn, &c.id, "M").unwrap();
        let l = lessons::create_lesson(
            conn,
            &NewLesson {
                module_id: m.id,
                lesson_type: LessonType::Video,
                title: "Lecture 1".into(),
                description: None,
                url: None,
            },
        )
        .unwrap();
        (c.id, l.id)
    }

    pub(crate) fn new(lesson_id: &str, kind: AnnotationType, at: f64, body: &str) -> NewAnnotation {
        NewAnnotation {
            lesson_id: lesson_id.into(),
            kind,
            start_sec: Some(at),
            end_sec: None,
            title: None,
            body_md: Some(body.into()),
            status: None,
            priority: None,
            due_date: None,
            url: None,
            ref_type: None,
            answer_md: None,
            tags: None,
            material_id: None,
            page: None,
        }
    }

    #[test]
    fn ai_answers_and_flags() {
        let conn = open_in_memory().unwrap();
        let (_, l) = lesson(&conn);
        let q = create(&conn, &new(&l, AnnotationType::Question, 5.0, "why?")).unwrap();
        assert!(!q.ai_generated);
        let a = set_ai_answer(&conn, &q.id, " because ").unwrap();
        assert_eq!(
            (a.answer_md.as_deref(), a.status.as_deref(), a.ai_generated),
            (Some("because"), Some("answered"), true)
        );
        let note = create(&conn, &new(&l, AnnotationType::Note, 5.0, "x")).unwrap();
        assert!(set_ai_answer(&conn, &note.id, "y").is_err());
        assert!(set_ai_answer(&conn, &q.id, "  ").is_err());
        let r = create_with(&conn, &new(&l, AnnotationType::Note, 1.0, "ai"), true).unwrap();
        assert!(r.ai_generated);
    }

    #[test]
    fn page_anchors_are_validated_and_sorted() {
        let conn = open_in_memory().unwrap();
        let (course, l) = lesson(&conn);
        let mat = crate::db::repo::materials::insert(
            &conn,
            &NewMaterial {
                course_id: course.clone(),
                lesson_id: Some(l.clone()),
                kind: "assignment".into(),
                title: "Problem Set 1".into(),
                file_type: Some("application/pdf".into()),
                ..Default::default()
            },
        )
        .unwrap();
        conn.execute("UPDATE materials SET page_count = 4 WHERE id = ?1", [&mat])
            .unwrap();
        conn.execute("UPDATE lessons SET type = 'reading' WHERE id = ?1", [&l])
            .unwrap();
        let les = super::super::lessons::get_lesson(&conn, &l).unwrap();
        assert_eq!(
            (les.doc_kind.as_deref(), les.doc_pages),
            (Some("assignment"), Some(4))
        );
        let at_page = |page: Option<i32>, material: Option<&str>| NewAnnotation {
            start_sec: None,
            page,
            material_id: material.map(Into::into),
            ..new(&l, AnnotationType::Note, 0.0, "x")
        };
        let p3 = create(&conn, &at_page(Some(3), Some(&mat))).unwrap();
        assert_eq!(
            (p3.page, p3.material_id.as_deref()),
            (Some(3), Some(mat.as_str()))
        );
        create(&conn, &at_page(Some(1), Some(&mat))).unwrap();
        assert!(
            create(&conn, &at_page(Some(5), Some(&mat))).is_err(),
            "past the last page"
        );
        assert!(create(&conn, &at_page(Some(0), Some(&mat))).is_err());
        assert!(
            create(&conn, &at_page(Some(2), None)).is_err(),
            "page without a document"
        );
        assert!(create(&conn, &at_page(Some(2), Some("nope"))).is_err());
        let pages: Vec<_> = list_for_lesson(&conn, &l)
            .unwrap()
            .into_iter()
            .map(|a| a.page)
            .collect();
        assert_eq!(pages, vec![Some(1), Some(3)]);
        let moved = update(
            &conn,
            &p3.id,
            &AnnotationUpdate {
                page: Some(4),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(moved.page, Some(4));
        assert!(
            update(
                &conn,
                &p3.id,
                &AnnotationUpdate {
                    page: Some(9),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn crud_ordering_and_fts() {
        let conn = open_in_memory().unwrap();
        let (course, l) = lesson(&conn);
        let late = create(
            &conn,
            &new(&l, AnnotationType::Note, 300.0, "Raft elects a leader"),
        )
        .unwrap();
        let early = create(&conn, &new(&l, AnnotationType::Bookmark, 12.5, "")).unwrap();
        assert_eq!(early.body_md, None, "blank text is stored as NULL");
        let ids: Vec<_> = list_for_lesson(&conn, &l)
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, vec![early.id.clone(), late.id.clone()]);

        let hit: String = conn
            .query_row(
                "SELECT a.id FROM annotations_fts f JOIN annotations a ON a.rid = f.rowid WHERE annotations_fts MATCH 'leaders'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, late.id);

        update(
            &conn,
            &late.id,
            &AnnotationUpdate {
                body_md: Some("Paxos instead".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM annotations_fts WHERE annotations_fts MATCH 'raft'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "FTS reflects updates");

        let ctx = list_for_course(&conn, &course).unwrap();
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx[0].lesson_title, "Lecture 1");
        delete(&conn, &early.id).unwrap();
        assert!(delete(&conn, &early.id).is_err());
    }

    #[test]
    fn question_defaults_and_answering() {
        let conn = open_in_memory().unwrap();
        let (_, l) = lesson(&conn);
        let q = create(
            &conn,
            &new(&l, AnnotationType::Question, 60.0, "Why linearizability?"),
        )
        .unwrap();
        assert_eq!(q.status.as_deref(), Some("open"));
        let q = update(
            &conn,
            &q.id,
            &AnnotationUpdate {
                answer_md: Some("Because…".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(q.status.as_deref(), Some("answered"));
    }

    #[test]
    fn validation() {
        let conn = open_in_memory().unwrap();
        let (_, l) = lesson(&conn);
        let mut bad = new(&l, AnnotationType::Todo, 5.0, "x");
        bad.priority = Some("urgent".into());
        assert!(create(&conn, &bad).is_err());
        let mut bad = new(&l, AnnotationType::Note, 5.0, "x");
        bad.end_sec = Some(1.0);
        assert!(create(&conn, &bad).is_err());
        let mut bad = new(&l, AnnotationType::Todo, 5.0, "x");
        bad.due_date = Some("tomorrow".into());
        assert!(create(&conn, &bad).is_err());
        assert!(create(&conn, &new("missing", AnnotationType::Note, 1.0, "x")).is_err());
    }
}
