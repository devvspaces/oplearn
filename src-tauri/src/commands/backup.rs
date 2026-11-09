//! Backups, restore and JSON export/import (Settings → Backups).

use std::path::PathBuf;

use chrono::Local;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::services::backup::{self, BackupInfo, BackupKind, ImportSummary};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn list_backups(state: State<'_, AppState>) -> AppResult<Vec<BackupInfo>> {
    backup::list(&state.paths.backups_dir())
}

#[tauri::command]
#[specta::specta]
pub async fn backup_now(state: State<'_, AppState>) -> AppResult<BackupInfo> {
    let dir = state.paths.backups_dir();
    let path = backup::backup(
        &*state.db()?,
        &dir,
        BackupKind::Manual,
        Local::now().naive_local(),
    )?;
    backup::list(&dir)?
        .into_iter()
        .find(|b| std::path::Path::new(&b.path) == path)
        .ok_or_else(|| AppError::internal("backup written but not found"))
}

/// Replaces the library with a backup file (a safety backup of the current one is made first).
#[tauri::command]
#[specta::specta]
pub async fn restore_backup(state: State<'_, AppState>, path: String) -> AppResult<String> {
    let safety = backup::restore(&state, &PathBuf::from(path))?;
    crate::db::repo::relocate::rebase_and_log(&*state.db()?, &state.paths.data_dir);
    Ok(safety.to_string_lossy().into_owned())
}

/// Writes the whole library (without media files) to a JSON file.
#[tauri::command]
#[specta::specta]
pub async fn export_library_json(state: State<'_, AppState>, path: String) -> AppResult<()> {
    let v = backup::export_json(&*state.db()?, Local::now())?;
    std::fs::write(&path, serde_json::to_vec_pretty(&v)?)?;
    Ok(())
}

/// Replaces the library with a JSON export, after a safety backup.
#[tauri::command]
#[specta::specta]
pub async fn import_library_json(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<ImportSummary> {
    let bytes = std::fs::read(&path)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::invalid("that file isn't valid JSON"))?;
    let conn = state.db()?;
    backup::backup(
        &conn,
        &state.paths.backups_dir(),
        BackupKind::Safety,
        Local::now().naive_local(),
    )?;
    let summary = backup::import_json(&conn, &v)?;
    crate::db::repo::relocate::rebase_and_log(&conn, &state.paths.data_dir);
    Ok(summary)
}
