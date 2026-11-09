use tauri::State;

use crate::db::models::*;
use crate::db::repo::{courses, lessons, modules};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn list_courses(state: State<'_, AppState>) -> AppResult<Vec<CourseSummary>> {
    courses::list_course_summaries(&*state.db()?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_course_detail(state: State<'_, AppState>, id: String) -> AppResult<CourseDetail> {
    courses::get_course_detail(&*state.db()?, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn create_course(state: State<'_, AppState>, input: NewCourse) -> AppResult<Course> {
    let conn = state.db()?;
    let tx = conn.unchecked_transaction()?;
    let c = courses::create_course(&tx, &input, None)?;
    modules::create_module(&tx, &c.id, "Module 1")?;
    tx.commit()?;
    Ok(c)
}

#[tauri::command]
#[specta::specta]
pub async fn update_course(
    state: State<'_, AppState>,
    id: String,
    update: CourseUpdate,
) -> AppResult<Course> {
    courses::update_course(&*state.db()?, &id, &update)
}

/// Deletes a course and all its rows. Local media for the course is removed as well.
#[tauri::command]
#[specta::specta]
pub async fn delete_course(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let c = courses::delete_course(&*state.db()?, &id)?;
    let dir = state.paths.media_dir().join(&c.slug);
    if dir.starts_with(state.paths.media_dir()) && dir.is_dir() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn create_module(
    state: State<'_, AppState>,
    course_id: String,
    title: String,
) -> AppResult<Module> {
    modules::create_module(&*state.db()?, &course_id, &title)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_module(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> AppResult<Module> {
    modules::rename_module(&*state.db()?, &id, &title)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_module(state: State<'_, AppState>, id: String) -> AppResult<()> {
    modules::delete_module(&*state.db()?, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_modules(
    state: State<'_, AppState>,
    course_id: String,
    module_ids: Vec<String>,
) -> AppResult<()> {
    modules::reorder_modules(&*state.db()?, &course_id, &module_ids)
}

#[tauri::command]
#[specta::specta]
pub async fn apply_layout(
    state: State<'_, AppState>,
    course_id: String,
    layout: Vec<ModuleOrder>,
) -> AppResult<()> {
    lessons::apply_layout(&*state.db()?, &course_id, &layout)
}

#[tauri::command]
#[specta::specta]
pub async fn create_lesson(state: State<'_, AppState>, input: NewLesson) -> AppResult<Lesson> {
    lessons::create_lesson(&*state.db()?, &input)
}

#[tauri::command]
#[specta::specta]
pub async fn update_lesson(
    state: State<'_, AppState>,
    id: String,
    update: LessonUpdate,
) -> AppResult<Lesson> {
    lessons::update_lesson(&*state.db()?, &id, &update)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_lesson(state: State<'_, AppState>, id: String) -> AppResult<()> {
    lessons::delete_lesson(&*state.db()?, &id)
}

#[tauri::command]
#[specta::specta]
pub async fn set_lesson_completed(
    state: State<'_, AppState>,
    id: String,
    completed: bool,
) -> AppResult<Lesson> {
    lessons::set_completed(&*state.db()?, &id, completed)
}

#[tauri::command]
#[specta::specta]
pub async fn get_lesson_context(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<LessonContext> {
    lessons::get_lesson_context(&*state.db()?, &id)
}
