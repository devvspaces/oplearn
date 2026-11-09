use std::collections::HashMap;

use tauri::State;

use crate::db::models::{NotebookScope, SearchResults};
use crate::db::repo::{annotations, courses, notebooks, search};
use crate::error::{AppError, AppResult};
use crate::services::export::build_course_markdown;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn search_everything(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> AppResult<SearchResults> {
    search::global_search(&*state.db()?, &query, limit)
}

/// Writes the course as Markdown to `path` (chosen by the user in a save dialog).
#[tauri::command]
#[specta::specta]
pub async fn export_course_markdown(
    state: State<'_, AppState>,
    course_id: String,
    path: String,
) -> AppResult<String> {
    let p = std::path::PathBuf::from(&path);
    if !p.is_absolute() {
        return Err(AppError::invalid("choose a file location"));
    }
    let p = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
        p
    } else {
        p.with_extension("md")
    };
    let md = {
        let conn = state.db()?;
        let detail = courses::get_course_detail(&conn, &course_id)?;
        let anns = annotations::list_for_course(&conn, &course_id)?;
        let mut nbs = HashMap::new();
        for l in detail.modules.iter().flat_map(|m| &m.lessons) {
            let nb = notebooks::get(&conn, NotebookScope::Lesson, &l.id)?;
            if !nb.body_md.trim().is_empty() {
                nbs.insert(l.id.clone(), nb.body_md);
            }
        }
        let course_nb = notebooks::get(&conn, NotebookScope::Course, &course_id)?.body_md;
        build_course_markdown(
            &detail,
            &anns,
            &nbs,
            &course_nb,
            &chrono::Local::now().format("%Y-%m-%d").to_string(),
        )
    };
    // Copy captured frames next to the export and point the Markdown at them.
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("course")
        .to_string();
    let assets = format!("{stem}.assets");
    let (md, files) = crate::services::export::rewrite_attachment_links(&md, &assets);
    if !files.is_empty() {
        let dir = p.with_file_name(&assets);
        std::fs::create_dir_all(&dir)?;
        for rel in files {
            let src = state.paths.data_dir.join(&rel);
            if let Some(name) = src.file_name() {
                let _ = std::fs::copy(&src, dir.join(name));
            }
        }
    }
    std::fs::write(&p, md)?;
    Ok(p.display().to_string())
}
