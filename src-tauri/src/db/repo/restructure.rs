//! Applying a module structure to a course (AI restructure) with a one-step undo snapshot.

use std::collections::HashSet;

use rusqlite::{Connection, params};

use crate::db::models::StructureModule;
use crate::db::repo::{lessons, modules, settings};
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};

pub const UNSORTED: &str = "Unsorted";

fn undo_key(course_id: &str) -> String {
    format!("restructure_undo:{course_id}")
}

/// The course's current structure (module titles + lesson order).
pub fn snapshot(conn: &Connection, course_id: &str) -> AppResult<Vec<StructureModule>> {
    let all = lessons::list_course_lessons(conn, course_id)?;
    Ok(modules::list_modules(conn, course_id)?
        .into_iter()
        .map(|m| StructureModule {
            lesson_ids: all
                .iter()
                .filter(|l| l.module_id == m.id)
                .map(|l| l.id.clone())
                .collect(),
            title: m.title,
            summary: None,
        })
        .collect())
}

/// Makes a model proposal valid: every course lesson exactly once (missing ones go to an
/// "Unsorted" module at the end), unknown/duplicate ids dropped, empty modules removed, titles
/// trimmed. Returns (modules, unsorted, dropped).
pub fn normalize(
    proposal: Vec<StructureModule>,
    course_lessons: &[String],
) -> (Vec<StructureModule>, i32, i32) {
    let valid: HashSet<&str> = course_lessons.iter().map(String::as_str).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut dropped = 0;
    let mut out: Vec<StructureModule> = proposal
        .into_iter()
        .map(|m| {
            let ids: Vec<String> = m
                .lesson_ids
                .into_iter()
                .filter(|id| {
                    let ok = valid.contains(id.as_str()) && seen.insert(id.clone());
                    if !ok {
                        dropped += 1;
                    }
                    ok
                })
                .collect();
            let title = m.title.trim();
            StructureModule {
                title: if title.is_empty() {
                    "Module".into()
                } else {
                    title.chars().take(120).collect()
                },
                summary: m
                    .summary
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                lesson_ids: ids,
            }
        })
        .filter(|m| !m.lesson_ids.is_empty())
        .collect();
    let missing: Vec<String> = course_lessons
        .iter()
        .filter(|id| !seen.contains(*id))
        .cloned()
        .collect();
    let unsorted = missing.len() as i32;
    if !missing.is_empty() {
        out.push(StructureModule {
            title: UNSORTED.into(),
            summary: None,
            lesson_ids: missing,
        });
    }
    (out, unsorted, dropped)
}

/// Replaces the course's modules with `structure` in one transaction, saving the previous
/// structure for `undo`. `structure` must list every lesson exactly once.
pub fn apply(
    conn: &Connection,
    course_id: &str,
    structure: &[StructureModule],
    save_undo: bool,
) -> AppResult<()> {
    let course_lessons: Vec<String> = lessons::list_course_lessons(conn, course_id)?
        .into_iter()
        .map(|l| l.id)
        .collect();
    let mut listed: Vec<&str> = structure
        .iter()
        .flat_map(|m| m.lesson_ids.iter().map(String::as_str))
        .collect();
    listed.sort_unstable();
    let mut expected: Vec<&str> = course_lessons.iter().map(String::as_str).collect();
    expected.sort_unstable();
    if listed != expected {
        return Err(AppError::invalid(
            "the new structure must contain every lesson of the course exactly once",
        ));
    }
    if structure.iter().any(|m| m.title.trim().is_empty()) {
        return Err(AppError::invalid("every module needs a title"));
    }
    let before = snapshot(conn, course_id)?;
    let old: Vec<String> = modules::list_modules(conn, course_id)?
        .into_iter()
        .map(|m| m.id)
        .collect();
    let tx = conn.unchecked_transaction()?;
    let ts = now();
    for (mi, m) in structure.iter().enumerate() {
        let id = new_id();
        tx.execute(
            "INSERT INTO modules (id, course_id, title, position, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, course_id, m.title.trim(), mi as i32, ts],
        )?;
        for (li, lesson) in m.lesson_ids.iter().enumerate() {
            tx.execute(
                "UPDATE lessons SET module_id = ?2, position = ?3, updated_at = ?4 WHERE id = ?1",
                params![lesson, id, li as i32, ts],
            )?;
        }
    }
    for id in old {
        // All lessons have moved, so this never cascades into lessons.
        tx.execute("DELETE FROM modules WHERE id = ?1", [id])?;
    }
    // The user re-ordered the course on purpose: plans follow the new outline from now on.
    tx.execute(
        "UPDATE courses SET plan_order = 'outline' WHERE id = ?1",
        [course_id],
    )?;
    if save_undo {
        settings::set(&tx, &undo_key(course_id), &before)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn has_undo(conn: &Connection, course_id: &str) -> AppResult<bool> {
    Ok(
        settings::get::<Vec<StructureModule>>(conn, &undo_key(course_id))?
            .is_some_and(|v| !v.is_empty()),
    )
}

/// Restores the structure saved by the last `apply`. Lessons added since then are kept in an
/// "Unsorted" module.
pub fn undo(conn: &Connection, course_id: &str) -> AppResult<()> {
    let prev: Vec<StructureModule> = settings::get(conn, &undo_key(course_id))?
        .filter(|v: &Vec<StructureModule>| !v.is_empty())
        .ok_or_else(|| AppError::invalid("there's nothing to undo for this course"))?;
    let current: Vec<String> = lessons::list_course_lessons(conn, course_id)?
        .into_iter()
        .map(|l| l.id)
        .collect();
    let (structure, _, _) = normalize(prev, &current);
    apply(conn, course_id, &structure, false)?;
    settings::set::<Vec<StructureModule>>(conn, &undo_key(course_id), &vec![])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::import;
    use crate::services::import::{ImportedPlaylist, ImportedVideo};

    fn m(title: &str, ids: &[&str]) -> StructureModule {
        StructureModule {
            title: title.into(),
            summary: None,
            lesson_ids: ids.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn normalize_fills_gaps_and_drops_junk() {
        let lessons: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let (out, unsorted, dropped) = normalize(
            vec![
                m(" Intro ", &["b", "a", "zzz"]),
                m("Empty", &["zzz"]),
                m("More", &["a", "c"]),
            ],
            &lessons,
        );
        assert_eq!(
            out,
            vec![
                m("Intro", &["b", "a"]),
                m("More", &["c"]),
                m(UNSORTED, &["d"])
            ]
        );
        assert_eq!((unsorted, dropped), (1, 3));
    }

    #[test]
    fn apply_and_undo_roundtrip_keeps_lessons() {
        let conn = open_in_memory().unwrap();
        let p = ImportedPlaylist {
            playlist_id: "PLrestructure1".into(),
            title: "C".into(),
            description: None,
            channel_title: None,
            thumbnail_url: None,
            videos: (0..4)
                .map(|i| ImportedVideo {
                    video_id: format!("vid{i:08}"),
                    title: format!("L{i}"),
                    description: None,
                    channel_title: None,
                    duration_sec: Some(60),
                    license: None,
                })
                .collect(),
            skipped: 0,
        };
        let (cid, _) = import::import_playlist(&conn, &p).unwrap();
        let before = snapshot(&conn, &cid).unwrap();
        let ids: Vec<String> = before.iter().flat_map(|m| m.lesson_ids.clone()).collect();
        let new = vec![
            StructureModule {
                title: "Week 2".into(),
                summary: None,
                lesson_ids: vec![ids[3].clone(), ids[2].clone()],
            },
            StructureModule {
                title: "Week 1".into(),
                summary: None,
                lesson_ids: vec![ids[0].clone(), ids[1].clone()],
            },
        ];
        assert!(
            apply(&conn, &cid, &new[..1], true).is_err(),
            "must include every lesson"
        );
        apply(&conn, &cid, &new, true).unwrap();
        let after = snapshot(&conn, &cid).unwrap();
        assert_eq!(
            after.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(),
            vec!["Week 2", "Week 1"]
        );
        assert_eq!(after[0].lesson_ids, vec![ids[3].clone(), ids[2].clone()]);
        assert!(has_undo(&conn, &cid).unwrap());
        undo(&conn, &cid).unwrap();
        assert_eq!(
            snapshot(&conn, &cid)
                .unwrap()
                .iter()
                .map(|m| (&m.title, &m.lesson_ids))
                .collect::<Vec<_>>(),
            before
                .iter()
                .map(|m| (&m.title, &m.lesson_ids))
                .collect::<Vec<_>>()
        );
        assert!(!has_undo(&conn, &cid).unwrap());
    }
}
