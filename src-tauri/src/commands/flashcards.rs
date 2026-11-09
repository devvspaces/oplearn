//! Phase 5: flashcards and global annotation lists.

use chrono::Utc;
use tauri::State;

use crate::db::models::*;
use crate::db::repo::{annotations, flashcards, study};
use crate::error::AppResult;
use crate::services::sm2::Grade;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn create_flashcard(
    state: State<'_, AppState>,
    card: NewFlashcard,
) -> AppResult<Flashcard> {
    flashcards::create(&*state.db()?, &card)
}

#[tauri::command]
#[specta::specta]
pub async fn update_flashcard(
    state: State<'_, AppState>,
    id: String,
    front_md: String,
    back_md: String,
) -> AppResult<Flashcard> {
    flashcards::update(&*state.db()?, &id, &front_md, &back_md)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_flashcard(state: State<'_, AppState>, id: String) -> AppResult<()> {
    flashcards::delete(&*state.db()?, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn list_flashcards(
    state: State<'_, AppState>,
    lesson_id: Option<String>,
) -> AppResult<Vec<Flashcard>> {
    flashcards::list(&*state.db()?, lesson_id.as_deref())
}

#[tauri::command]
#[specta::specta]
pub async fn due_flashcards(state: State<'_, AppState>, limit: i32) -> AppResult<Vec<Flashcard>> {
    flashcards::due(&*state.db()?, Utc::now(), limit)
}

/// Grades a card; `seconds` spent on it count toward today's study minutes.
#[tauri::command]
#[specta::specta]
pub async fn review_flashcard(
    state: State<'_, AppState>,
    id: String,
    grade: Grade,
    seconds: f64,
) -> AppResult<Flashcard> {
    let conn = state.db()?;
    let card = flashcards::review(&conn, &id, grade, Utc::now())?;
    study::record_activity(
        &conn,
        card.lesson_id.as_deref(),
        ActivitySource::Review,
        seconds.min(120.0),
        Utc::now(),
    )?;
    Ok(card)
}

/// All questions / to-dos / references across courses.
#[tauri::command]
#[specta::specta]
pub async fn list_annotations_by_type(
    state: State<'_, AppState>,
    kind: AnnotationType,
) -> AppResult<Vec<AnnotationWithCourse>> {
    annotations::list_by_type(&*state.db()?, kind)
}
