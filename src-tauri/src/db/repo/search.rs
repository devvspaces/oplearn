//! Global full-text search across transcripts, annotations, lessons and notebooks (SPEC §7.10).

use rusqlite::{Connection, params};

use crate::db::models::{SearchHit, SearchResults};
use crate::error::AppResult;

// Private-use sentinels for snippet highlights; replaced after HTML-escaping.
const OPEN: &str = "\u{E000}";
const CLOSE: &str = "\u{E001}";

/// Turns free text into a safe FTS5 query: every token is quoted (so operators and punctuation
/// can't break the syntax) and the last token is a prefix match for search-as-you-type.
pub fn to_fts_query(input: &str) -> Option<String> {
    let tokens: Vec<String> = input
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '_'))
        .map(|t| t.trim_matches('\''))
        .filter(|t| !t.is_empty())
        .take(12)
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let n = tokens.len();
    Some(
        tokens
            .into_iter()
            .enumerate()
            .map(|(i, t)| if i == n - 1 { format!("{t}*") } else { t })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// HTML-escapes the snippet and turns highlight sentinels into `<mark>` tags.
fn render_snippet(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace(OPEN, "<mark>")
        .replace(CLOSE, "</mark>")
}

fn snippet_expr(table: &str, col: i32) -> String {
    format!("snippet({table}, {col}, '{OPEN}', '{CLOSE}', '…', 14)")
}

pub fn global_search(conn: &Connection, input: &str, limit: u32) -> AppResult<SearchResults> {
    let Some(q) = to_fts_query(input) else {
        return Ok(SearchResults::default());
    };
    let limit = limit.clamp(1, 50);
    let hit = |r: &rusqlite::Row<'_>| -> rusqlite::Result<SearchHit> {
        Ok(SearchHit {
            course_id: r.get(0)?,
            course_title: r.get(1)?,
            lesson_id: r.get(2)?,
            lesson_title: r.get(3)?,
            start_sec: r.get(4)?,
            snippet: render_snippet(&r.get::<_, String>(5)?),
            annotation_type: r.get(6)?,
            item_id: r.get(7)?,
            page: r.get(8)?,
        })
    };

    let transcripts = conn
        .prepare(&format!(
            "SELECT co.id, co.title, l.id, l.title, c.start_sec, {}, NULL, NULL, NULL
             FROM transcript_fts JOIN transcript_cues c ON c.id = transcript_fts.rowid
             JOIN lessons l ON l.id = c.lesson_id JOIN modules m ON m.id = l.module_id JOIN courses co ON co.id = m.course_id
             WHERE transcript_fts MATCH ?1 ORDER BY bm25(transcript_fts) LIMIT ?2",
            snippet_expr("transcript_fts", 0)
        ))?
        .query_map(params![q, limit], hit)?
        .collect::<Result<_, _>>()?;

    let annotations = conn
        .prepare(&format!(
            "SELECT co.id, co.title, l.id, l.title, a.start_sec, {}, a.type, NULL, NULL
             FROM annotations_fts JOIN annotations a ON a.rid = annotations_fts.rowid
             JOIN lessons l ON l.id = a.lesson_id JOIN modules m ON m.id = l.module_id JOIN courses co ON co.id = m.course_id
             WHERE annotations_fts MATCH ?1 ORDER BY bm25(annotations_fts) LIMIT ?2",
            snippet_expr("annotations_fts", -1)
        ))?
        .query_map(params![q, limit], hit)?
        .collect::<Result<_, _>>()?;

    let lessons = conn
        .prepare(&format!(
            "SELECT co.id, co.title, l.id, l.title, NULL, {}, NULL, NULL, NULL
             FROM lessons_fts JOIN lessons l ON l.id = lessons_fts.lesson_id
             JOIN modules m ON m.id = l.module_id JOIN courses co ON co.id = m.course_id
             WHERE lessons_fts MATCH ?1 ORDER BY bm25(lessons_fts) LIMIT ?2",
            snippet_expr("lessons_fts", -1)
        ))?
        .query_map(params![q, limit], hit)?
        .collect::<Result<_, _>>()?;

    // Notebooks: lesson-scoped ones resolve to their lesson and course, course-scoped ones to the course.
    let notebooks = conn
        .prepare(&format!(
            "SELECT co.id, co.title, l.id, l.title, NULL, {}, NULL, NULL, NULL
             FROM notebooks_fts JOIN notebooks n ON n.id = notebooks_fts.notebook_id
             LEFT JOIN lessons l ON n.scope = 'lesson' AND l.id = n.scope_id
             LEFT JOIN modules m ON m.id = l.module_id
             JOIN courses co ON co.id = CASE WHEN n.scope = 'course' THEN n.scope_id ELSE m.course_id END
             WHERE notebooks_fts MATCH ?1 ORDER BY bm25(notebooks_fts) LIMIT ?2",
            snippet_expr("notebooks_fts", 1)
        ))?
        .query_map(params![q, limit], hit)?
        .collect::<Result<_, _>>()?;

    // Flashcards: course comes from the source lesson (cards without one show as "No course").
    let flashcards = conn
        .prepare(&format!(
            "SELECT coalesce(co.id, ''), coalesce(co.title, 'No course'), l.id, l.title, f.source_sec, {}, NULL, f.id, NULL
             FROM flashcards_fts JOIN flashcards f ON f.id = flashcards_fts.card_id
             LEFT JOIN lessons l ON l.id = f.lesson_id LEFT JOIN modules m ON m.id = l.module_id
             LEFT JOIN courses co ON co.id = m.course_id
             WHERE flashcards_fts MATCH ?1 ORDER BY bm25(flashcards_fts) LIMIT ?2",
            snippet_expr("flashcards_fts", -1)
        ))?
        .query_map(params![q, limit], hit)?
        .collect::<Result<_, _>>()?;

    // Course materials (PDF pages). `lesson_title` carries the material title.
    let materials = conn
        .prepare(&format!(
            "SELECT co.id, co.title, mt.lesson_id, mt.title, NULL, {}, NULL, mt.id, CAST(material_pages_fts.page AS INTEGER)
             FROM material_pages_fts JOIN materials mt ON mt.id = material_pages_fts.material_id
             JOIN courses co ON co.id = mt.course_id
             WHERE material_pages_fts MATCH ?1 ORDER BY bm25(material_pages_fts) LIMIT ?2",
            snippet_expr("material_pages_fts", 2)
        ))?
        .query_map(params![q, limit], hit)?
        .collect::<Result<_, _>>()?;

    Ok(SearchResults {
        transcripts,
        annotations,
        lessons,
        notebooks,
        flashcards,
        materials,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{AnnotationType, NotebookScope};
    use crate::db::open_in_memory;
    use crate::db::repo::annotations::{
        self,
        tests::{lesson, new},
    };
    use crate::db::repo::{notebooks, transcripts};
    use crate::services::vtt::Cue;

    #[test]
    fn sanitizes_queries() {
        assert_eq!(
            to_fts_query("map reduce").as_deref(),
            Some("\"map\" \"reduce\"*")
        );
        assert_eq!(to_fts_query("  ").as_deref(), None);
        assert_eq!(
            to_fts_query("raft) OR (\"x").as_deref(),
            Some("\"raft\" \"OR\" \"x\"*")
        );
        assert_eq!(
            to_fts_query("NEAR(a b)").as_deref(),
            Some("\"NEAR\" \"a\" \"b\"*")
        );
        assert_eq!(to_fts_query("don't").as_deref(), Some("\"don't\"*"));
    }

    #[test]
    fn finds_everything_grouped_with_safe_snippets() {
        let conn = open_in_memory().unwrap();
        let (course, l) = lesson(&conn);
        transcripts::replace(
            &conn,
            &l,
            "auto",
            Some("en"),
            &[Cue {
                start: 754.0,
                end: 757.0,
                text: "today we talk about <MapReduce> & GFS".into(),
            }],
            &[],
        )
        .unwrap();
        annotations::create(
            &conn,
            &new(
                &l,
                AnnotationType::Question,
                760.0,
                "why does mapreduce need a master?",
            ),
        )
        .unwrap();
        notebooks::save(&conn, NotebookScope::Course, &course, "MapReduce summary").unwrap();
        notebooks::save(&conn, NotebookScope::Lesson, &l, "mapreduce lesson notes").unwrap();
        conn.execute(
            "UPDATE lessons SET title = 'Lecture 1: MapReduce' WHERE id = ?1",
            [&l],
        )
        .unwrap();

        let r = global_search(&conn, "mapred", 10).unwrap();
        assert_eq!(r.transcripts.len(), 1);
        let t = &r.transcripts[0];
        assert_eq!(t.start_sec, Some(754.0));
        assert_eq!(t.lesson_id.as_deref(), Some(l.as_str()));
        assert!(
            t.snippet
                .contains("&lt;<mark>MapReduce</mark>&gt; &amp; GFS"),
            "{}",
            t.snippet
        );
        assert_eq!(r.annotations.len(), 1);
        assert_eq!(
            r.annotations[0].annotation_type,
            Some(AnnotationType::Question)
        );
        assert_eq!(r.lessons.len(), 1, "title updates are indexed");
        assert_eq!(r.notebooks.len(), 2);
        assert!(r.notebooks.iter().all(|h| h.course_id == course));

        // Flashcards are searchable, with their lesson/course context, and updates reindex.
        let card = crate::db::repo::flashcards::create(
            &conn,
            &crate::db::models::NewFlashcard {
                lesson_id: Some(l.clone()),
                source_sec: Some(12.0),
                source_material_id: None,
                source_page: None,
                front_md: "Why does MapReduce need a master?".into(),
                back_md: "Scheduling".into(),
            },
        )
        .unwrap();
        let r = global_search(&conn, "mapreduce master", 10).unwrap();
        assert_eq!(r.flashcards.len(), 1);
        assert_eq!(r.flashcards[0].item_id.as_deref(), Some(card.id.as_str()));
        assert_eq!(r.flashcards[0].course_id, course);
        crate::db::repo::flashcards::update(&conn, &card.id, "Raft leader", "terms").unwrap();
        assert!(
            global_search(&conn, "mapreduce master", 10)
                .unwrap()
                .flashcards
                .is_empty()
        );
        assert_eq!(
            global_search(&conn, "raft", 10).unwrap().flashcards.len(),
            1
        );
        assert!(
            global_search(&conn, "\"\"", 10)
                .unwrap()
                .transcripts
                .is_empty()
        );
    }

    #[test]
    fn search_is_fast_on_100k_cues() {
        let conn = open_in_memory().unwrap();
        let (_, l) = lesson(&conn);
        let words = [
            "raft",
            "paxos",
            "leader",
            "election",
            "log",
            "replication",
            "consensus",
            "shard",
            "lease",
            "clock",
        ];
        let cues: Vec<Cue> = (0..100_000)
            .map(|i| Cue {
                start: i as f64,
                end: i as f64 + 1.0,
                text: format!(
                    "{} {} {} item{}",
                    words[i % 10],
                    words[(i * 7) % 10],
                    words[(i * 3) % 10],
                    i
                ),
            })
            .collect();
        transcripts::replace(&conn, &l, "auto", Some("en"), &cues, &[]).unwrap();
        let t = std::time::Instant::now();
        let r = global_search(&conn, "leader lo", 20).unwrap();
        let elapsed = t.elapsed();
        assert_eq!(r.transcripts.len(), 20);
        assert!(elapsed.as_millis() < 150, "search took {elapsed:?}");
    }
}
