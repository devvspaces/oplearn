use tauri::{AppHandle, State};

use crate::db::models::WatchProgress;
use crate::db::repo::progress;
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn save_position(
    state: State<'_, AppState>,
    lesson_id: String,
    position_sec: f64,
) -> AppResult<()> {
    progress::save_position(&*state.db()?, &lesson_id, position_sec)
}

#[tauri::command]
#[specta::specta]
pub async fn get_progress(
    state: State<'_, AppState>,
    lesson_id: String,
) -> AppResult<Option<WatchProgress>> {
    progress::get_progress(&*state.db()?, &lesson_id)
}

/// Called by the renderer after it has flushed pending writes in response to a `flush-request` event.
#[tauri::command]
#[specta::specta]
pub async fn flush_complete(app: AppHandle) -> AppResult<()> {
    crate::shutdown::finish(&app);
    Ok(())
}

/// Base URL of the loopback player bridge page (see `services/player_server.rs`).
#[tauri::command]
#[specta::specta]
pub async fn get_player_url(state: State<'_, AppState>) -> AppResult<String> {
    Ok(format!("http://127.0.0.1:{}/player", state.loopback.port))
}

/// Loopback URL for a downloaded file (WKWebView plays `asset://` video without audio).
#[tauri::command]
#[specta::specta]
pub async fn get_media_url(state: State<'_, AppState>, path: String) -> AppResult<String> {
    state
        .loopback
        .media_url(std::path::Path::new(&path))
        .ok_or_else(|| crate::error::AppError::invalid("file is outside the media folder"))
}
