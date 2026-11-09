use tauri::State;

use crate::db::models::*;
use crate::db::repo::{annotations, notebooks};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn list_annotations(
    state: State<'_, AppState>,
    lesson_id: String,
) -> AppResult<Vec<Annotation>> {
    annotations::list_for_lesson(&*state.db()?, &lesson_id)
}

#[tauri::command]
#[specta::specta]
pub async fn list_course_annotations(
    state: State<'_, AppState>,
    course_id: String,
) -> AppResult<Vec<AnnotationInContext>> {
    annotations::list_for_course(&*state.db()?, &course_id)
}

#[tauri::command]
#[specta::specta]
pub async fn create_annotation(
    state: State<'_, AppState>,
    input: NewAnnotation,
) -> AppResult<Annotation> {
    annotations::create(&*state.db()?, &input)
}

#[tauri::command]
#[specta::specta]
pub async fn update_annotation(
    state: State<'_, AppState>,
    id: String,
    update: AnnotationUpdate,
) -> AppResult<Annotation> {
    annotations::update(&*state.db()?, &id, &update)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_annotation(state: State<'_, AppState>, id: String) -> AppResult<()> {
    annotations::delete(&*state.db()?, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_notebook(
    state: State<'_, AppState>,
    scope: NotebookScope,
    scope_id: String,
) -> AppResult<Notebook> {
    notebooks::get(&*state.db()?, scope, &scope_id)
}

#[tauri::command]
#[specta::specta]
pub async fn save_notebook(
    state: State<'_, AppState>,
    scope: NotebookScope,
    scope_id: String,
    body_md: String,
) -> AppResult<Notebook> {
    notebooks::save(&*state.db()?, scope, &scope_id, &body_md)
}
