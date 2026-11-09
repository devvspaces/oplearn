//! Course materials (PDFs and pages imported from a provider) and provider metadata.

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::*;
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};
use crate::services::providers::ocw::OcwMeta;

const SELECT: &str = "SELECT id, course_id, lesson_id, solution_of, kind, title, section, description, source_url,
        file_url, local_path, file_type, file_size, page_count, html IS NOT NULL, position FROM materials";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<Material> {
    Ok(Material {
        id: r.get(0)?,
        course_id: r.get(1)?,
        lesson_id: r.get(2)?,
        solution_of: r.get(3)?,
        kind: r.get(4)?,
        title: r.get(5)?,
        section: r.get(6)?,
        description: r.get(7)?,
        source_url: r.get(8)?,
        file_url: r.get(9)?,
        local_path: r.get(10)?,
        file_type: r.get(11)?,
        file_size: r.get::<_, Option<i64>>(12)?.map(|v| v as f64),
        page_count: r.get(13)?,
        has_html: r.get(14)?,
        position: r.get(15)?,
    })
}

pub fn insert(conn: &Connection, m: &NewMaterial) -> AppResult<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO materials (id, course_id, lesson_id, solution_of, provider_uid, kind, title, section, description,
                source_url, file_url, local_path, file_type, file_size, html, position, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            id, m.course_id, m.lesson_id, m.solution_of, m.provider_uid, m.kind, m.title, m.section, m.description,
            m.source_url, m.file_url, m.local_path, m.file_type, m.file_size, m.html, m.position, now()
        ],
    )?;
    Ok(id)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Material> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("material"))
}

pub fn html(conn: &Connection, id: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row("SELECT html FROM materials WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .flatten())
}

pub fn set_html(conn: &Connection, id: &str, html: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE materials SET html = ?2 WHERE id = ?1",
        params![id, html],
    )?;
    Ok(())
}

pub fn set_solution_of(
    conn: &Connection,
    id: &str,
    base: &str,
    lesson_id: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE materials SET solution_of = ?2, lesson_id = coalesce(?3, lesson_id) WHERE id = ?1",
        params![id, base, lesson_id],
    )?;
    Ok(())
}

pub fn list_for_course(conn: &Connection, course_id: &str) -> AppResult<Vec<Material>> {
    let mut st = conn.prepare(&format!(
        "{SELECT} WHERE course_id = ?1 ORDER BY position, title"
    ))?;
    Ok(st.query_map([course_id], map)?.collect::<Result<_, _>>()?)
}

pub fn list_for_lesson(conn: &Connection, lesson_id: &str) -> AppResult<Vec<Material>> {
    let mut st = conn.prepare(&format!(
        "{SELECT} WHERE lesson_id = ?1 ORDER BY solution_of IS NOT NULL, position"
    ))?;
    Ok(st.query_map([lesson_id], map)?.collect::<Result<_, _>>()?)
}

pub fn indexed_pages(conn: &Connection, id: &str) -> AppResult<i32> {
    get(conn, id)?;
    Ok(conn.query_row(
        "SELECT count(*) FROM material_pages_fts WHERE material_id = ?1",
        [id],
        |r| r.get(0),
    )?)
}

/// Replaces a material's page text (search + AI context) and records the page count.
pub fn set_pages(conn: &Connection, id: &str, pages: &[String]) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM material_pages_fts WHERE material_id = ?1",
        [id],
    )?;
    for (i, text) in pages.iter().enumerate() {
        let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t.is_empty() {
            tx.execute(
                "INSERT INTO material_pages_fts (material_id, page, text) VALUES (?1, ?2, ?3)",
                params![id, (i + 1) as i64, t],
            )?;
        }
    }
    tx.execute(
        "UPDATE materials SET page_count = ?2 WHERE id = ?1",
        params![id, pages.len() as i64],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn set_meta(conn: &Connection, course_id: &str, meta: &OcwMeta) -> AppResult<()> {
    conn.execute(
        "INSERT INTO course_meta (course_id, json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(course_id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
        params![course_id, serde_json::to_string(meta)?, now()],
    )?;
    Ok(())
}

pub fn get_meta(conn: &Connection, course_id: &str) -> AppResult<Option<OcwMeta>> {
    Ok(conn
        .query_row(
            "SELECT json FROM course_meta WHERE course_id = ?1",
            [course_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|j| serde_json::from_str(&j).ok()))
}

/// Metadata for courses from the shared provider flow (MIT OCW uses `set_meta`).
pub fn set_provider_meta(
    conn: &Connection,
    course_id: &str,
    meta: &crate::services::providers::ProviderMeta,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO course_meta (course_id, json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(course_id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
        params![course_id, serde_json::to_string(meta)?, now()],
    )?;
    Ok(())
}

fn meta_json(conn: &Connection, course_id: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT json FROM course_meta WHERE course_id = ?1",
            [course_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

pub fn course_info(conn: &Connection, course_id: &str) -> AppResult<CourseInfo> {
    let (provider, provider_ref): (String, Option<String>) = conn.query_row(
        "SELECT provider, provider_ref FROM courses WHERE id = ?1",
        [course_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let pages: Vec<Material> = list_for_course(conn, course_id)?
        .into_iter()
        // Course pages only: lesson readings (OpenStax sections, transcripts) have HTML too.
        .filter(|m| m.has_html && m.lesson_id.is_none())
        .collect();
    let find = |k: &str| pages.iter().find(|m| m.kind == k).map(|m| m.id.clone());
    let json = meta_json(conn, course_id)?;
    let (meta, provider_meta) = if provider == "mit_ocw" {
        (json.and_then(|j| serde_json::from_str(&j).ok()), None)
    } else {
        (None, json.and_then(|j| serde_json::from_str(&j).ok()))
    };
    Ok(CourseInfo {
        provider,
        provider_ref,
        meta,
        provider_meta,
        syllabus_id: find("syllabus"),
        calendar_id: find("calendar"),
        pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::annotations::tests::lesson;

    #[test]
    fn insert_pages_and_search() {
        let conn = open_in_memory().unwrap();
        let (course, l) = lesson(&conn);
        let base = insert(
            &conn,
            &NewMaterial {
                course_id: course.clone(),
                lesson_id: Some(l.clone()),
                kind: "assignment".into(),
                title: "PS 1".into(),
                file_type: Some("application/pdf".into()),
                local_path: Some("/x.pdf".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let sol = insert(
            &conn,
            &NewMaterial {
                course_id: course.clone(),
                kind: "assignment".into(),
                title: "PS 1 sol".into(),
                ..Default::default()
            },
        )
        .unwrap();
        set_solution_of(&conn, &sol, &base, Some(&l)).unwrap();
        set_pages(
            &conn,
            &base,
            &[
                "Prove by induction that".into(),
                "".into(),
                "pigeonhole principle".into(),
            ],
        )
        .unwrap();
        let m = get(&conn, &base).unwrap();
        assert_eq!(m.page_count, Some(3));
        let ls = list_for_lesson(&conn, &l).unwrap();
        assert_eq!(
            ls.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(),
            vec!["PS 1", "PS 1 sol"]
        );
        let r = crate::db::repo::search::global_search(&conn, "pigeonhole", 10).unwrap();
        assert_eq!(r.materials.len(), 1);
        assert_eq!(r.materials[0].page, Some(3));
        assert_eq!(r.materials[0].item_id.as_deref(), Some(base.as_str()));
        insert(
            &conn,
            &NewMaterial {
                course_id: course.clone(),
                kind: "syllabus".into(),
                title: "Syllabus".into(),
                html: Some("<p>hi</p>".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let info = course_info(&conn, &course).unwrap();
        assert_eq!(info.provider, "youtube");
        assert!(info.syllabus_id.is_some());
    }
}
