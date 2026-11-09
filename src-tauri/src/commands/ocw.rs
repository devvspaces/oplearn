//! MIT OpenCourseWare: search (MIT Learn API), preview, import (course ZIP), and materials.

use std::path::PathBuf;

use tauri::State;
use tauri::ipc::Channel;
use tokio::io::AsyncWriteExt;

use crate::db::models::*;
use crate::db::repo::materials;
use crate::error::{AppError, AppResult};
use crate::services::providers::common;
use crate::services::providers::ocw::{self, OcwCourseCard, OcwPreview};
use crate::services::providers::ocw_import;
use crate::services::providers::{ImportProgress, ProviderImportReport};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn search_ocw(
    state: State<'_, AppState>,
    query: String,
) -> AppResult<Vec<OcwCourseCard>> {
    if query.trim().is_empty() {
        return Err(AppError::invalid("type something to search for"));
    }
    ocw::search(&state.http, query.trim()).await
}

fn slug_of(input: &str) -> AppResult<String> {
    ocw::parse_slug(input)
        .ok_or_else(|| AppError::invalid("that doesn't look like an OpenCourseWare course link"))
}

#[tauri::command]
#[specta::specta]
pub async fn preview_ocw(state: State<'_, AppState>, course: String) -> AppResult<OcwPreview> {
    ocw::preview(&state.http, &slug_of(&course)?).await
}

fn progress(
    ch: &Channel<ImportProgress>,
    stage: &str,
    done: f64,
    total: f64,
    message: impl Into<String>,
) {
    common::progress(ch, stage, done, total, message);
}

/// Downloads the course ZIP, builds the course, then indexes PDF text for search and AI.
#[tauri::command]
#[specta::specta]
pub async fn import_ocw(
    state: State<'_, AppState>,
    course: String,
    on_progress: Channel<ImportProgress>,
) -> AppResult<ProviderImportReport> {
    let slug = slug_of(&course)?;
    if let Ok(id) = state.db()?.query_row(
        "SELECT id FROM courses WHERE provider = 'mit_ocw' AND provider_ref = ?1",
        [&slug],
        |r| r.get::<_, String>(0),
    ) {
        return Err(AppError::invalid(format!(
            "this course is already in your library (id {id})"
        )));
    }
    progress(
        &on_progress,
        "download",
        0.0,
        0.0,
        "Finding the course download…",
    );
    let pv = ocw::preview(&state.http, &slug).await?;
    let zip_url = pv.zip_url.ok_or_else(|| {
        AppError::internal("OpenCourseWare doesn't offer a download for this course")
    })?;

    let incoming = state.paths.media_dir().join(".incoming");
    tokio::fs::create_dir_all(&incoming).await?;
    let zip_path = incoming.join(format!("{slug}.zip"));
    let extract_dir = incoming.join(&slug);
    let _ = tokio::fs::remove_dir_all(&extract_dir).await;

    // Download (streamed to disk).
    let mut resp = state
        .http
        .get(&zip_url)
        .timeout(std::time::Duration::from_secs(60 * 60))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AppError::internal(format!("course download failed: {e}")))?;
    let total = resp
        .content_length()
        .map(|n| n as f64)
        .or(pv.zip_bytes)
        .unwrap_or(0.0);
    let mut f = tokio::fs::File::create(&zip_path).await?;
    let mut done = 0.0;
    let mut last = std::time::Instant::now();
    while let Some(chunk) = resp.chunk().await? {
        f.write_all(&chunk).await?;
        done += chunk.len() as f64;
        if last.elapsed().as_millis() > 250 {
            progress(
                &on_progress,
                "download",
                done,
                total,
                "Downloading course files…",
            );
            last = std::time::Instant::now();
        }
    }
    f.flush().await?;
    drop(f);
    if total > 0.0 && (done - total).abs() > 1.0 {
        return Err(AppError::internal(
            "the course download was incomplete; try again",
        ));
    }

    progress(&on_progress, "extract", 0.0, 0.0, "Unpacking…");
    let (zp, ed) = (zip_path.clone(), extract_dir.clone());
    tokio::task::spawn_blocking(move || ocw::extract_zip(&zp, &ed))
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
    let _ = tokio::fs::remove_file(&zip_path).await;

    progress(&on_progress, "build", 0.0, 0.0, "Building the course…");
    let (ed, sl) = (extract_dir.clone(), slug.clone());
    let loaded = tokio::task::spawn_blocking(move || ocw_import::load(&ed, &sl))
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
    let written = ocw_import::write(&*state.db()?, &loaded)?;

    // Move the files under the course's own media folder and fix the stored paths.
    let final_dir = common::relocate(&state, &written.course_id, &extract_dir, "ocw").await?;
    let pdfs: Vec<(String, PathBuf)> = written
        .pdfs
        .iter()
        .filter_map(|(id, p)| {
            Some((
                id.clone(),
                final_dir.join(p.strip_prefix(&extract_dir).ok()?),
            ))
        })
        .collect();
    let indexed = common::index_pdfs(&state, pdfs, &on_progress).await?;
    progress(&on_progress, "done", 1.0, 1.0, "Done");
    Ok(ProviderImportReport {
        course_id: written.course_id,
        lessons: written.lessons,
        materials: written.materials,
        transcripts: written.transcripts,
        pdfs_indexed: indexed,
        skipped: 0,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_course_info(
    state: State<'_, AppState>,
    course_id: String,
) -> AppResult<CourseInfo> {
    materials::course_info(&*state.db()?, &course_id)
}

#[tauri::command]
#[specta::specta]
pub async fn list_materials(
    state: State<'_, AppState>,
    course_id: String,
) -> AppResult<Vec<Material>> {
    materials::list_for_course(&*state.db()?, &course_id)
}

#[tauri::command]
#[specta::specta]
pub async fn list_lesson_materials(
    state: State<'_, AppState>,
    lesson_id: String,
) -> AppResult<Vec<Material>> {
    materials::list_for_lesson(&*state.db()?, &lesson_id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_material(state: State<'_, AppState>, id: String) -> AppResult<Material> {
    materials::get(&*state.db()?, &id)
}

/// Sanitised HTML of an imported page (syllabus, calendar…), links already rewritten.
#[tauri::command]
#[specta::specta]
pub async fn get_material_html(state: State<'_, AppState>, id: String) -> AppResult<String> {
    let html = materials::html(&*state.db()?, &id)?.ok_or_else(|| AppError::not_found("page"))?;
    // Stored images (`lectern-media:`) point at this launch's loopback media URL.
    Ok(common::resolve_media(
        &html,
        &state.paths.media_dir(),
        |p| state.loopback.media_url(p),
    ))
}

/// Loopback URL for a material's local file (PDFs open in the in-app viewer).
#[tauri::command]
#[specta::specta]
pub async fn get_material_url(state: State<'_, AppState>, id: String) -> AppResult<String> {
    let m = materials::get(&*state.db()?, &id)?;
    let p = m
        .local_path
        .ok_or_else(|| AppError::not_found("a local copy of this file"))?;
    state
        .loopback
        .media_url(std::path::Path::new(&p))
        .ok_or_else(|| AppError::invalid("file is outside the media folder"))
}

/// How many pages of a PDF material have searchable text (0 = never indexed or skipped).
#[tauri::command]
#[specta::specta]
pub async fn get_material_indexed_pages(state: State<'_, AppState>, id: String) -> AppResult<i32> {
    materials::indexed_pages(&*state.db()?, &id)
}

/// Page text extracted by the in-app viewer for a PDF the import couldn't read. Only fills a
/// material that has no text yet, so it never overwrites the import's extraction.
#[tauri::command]
#[specta::specta]
pub async fn set_material_pages(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<String>,
) -> AppResult<bool> {
    const MAX_PAGES: usize = 3000;
    const MAX_CHARS: usize = 40_000;
    let conn = state.db()?;
    let m = materials::get(&conn, &id)?;
    if m.file_type.as_deref() != Some("application/pdf")
        || pages.is_empty()
        || pages.len() > MAX_PAGES
    {
        return Err(AppError::invalid("not a PDF page list"));
    }
    if materials::indexed_pages(&conn, &id)? > 0 {
        return Ok(false);
    }
    let pages: Vec<String> = pages
        .into_iter()
        .map(|p| p.chars().take(MAX_CHARS).collect())
        .collect();
    materials::set_pages(&conn, &id, &pages)?;
    Ok(true)
}
