use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub has_api_key: bool,
    pub ytdlp_path: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn get_app_info(state: State<'_, AppState>) -> AppResult<AppInfo> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.paths.data_dir.display().to_string(),
        has_api_key: state.api_key()?.is_some(),
        ytdlp_path: state.ytdlp().ok().map(|y| y.binary.display().to_string()),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_ytdlp_version(state: State<'_, AppState>) -> AppResult<String> {
    state.ytdlp()?.version().await
}

#[tauri::command]
#[specta::specta]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> AppResult<()> {
    let key = key.trim();
    if key.len() < 20 || key.chars().any(char::is_whitespace) {
        return Err(AppError::invalid(
            "that doesn't look like a YouTube Data API key",
        ));
    }
    state.set_api_key(Some(key))
}

#[tauri::command]
#[specta::specta]
pub async fn clear_api_key(state: State<'_, AppState>) -> AppResult<()> {
    state.set_api_key(None)
}

/// Changes the data directory. Takes effect after restart; existing data is not moved.
#[tauri::command]
#[specta::specta]
pub async fn set_data_dir(state: State<'_, AppState>, path: String) -> AppResult<()> {
    let p = std::path::PathBuf::from(&path);
    if !p.is_absolute() {
        return Err(AppError::invalid("choose an absolute folder path"));
    }
    std::fs::create_dir_all(&p)?;
    state.paths.set_data_dir(&p)
}

/// Preferred subtitle languages, in priority order (e.g. `["en", "fr"]`).
#[tauri::command]
#[specta::specta]
pub async fn get_subtitle_langs(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    Ok(crate::db::repo::settings::get::<Vec<String>>(
        &*state.db()?,
        crate::services::transcripts::SUBTITLE_LANGS_KEY,
    )?
    .filter(|v| !v.is_empty())
    .unwrap_or_else(|| vec!["en".into()]))
}

#[tauri::command]
#[specta::specta]
pub async fn set_subtitle_langs(state: State<'_, AppState>, langs: Vec<String>) -> AppResult<()> {
    let langs: Vec<String> = langs
        .iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if langs.is_empty()
        || langs
            .iter()
            .any(|l| l.len() > 12 || !l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    {
        return Err(AppError::invalid("use language codes like en, en-GB, fr"));
    }
    crate::db::repo::settings::set(
        &*state.db()?,
        crate::services::transcripts::SUBTITLE_LANGS_KEY,
        &langs,
    )
}

/// False on a fresh library (shows the welcome flow).
#[tauri::command]
#[specta::specta]
pub async fn get_onboarded(state: State<'_, AppState>) -> AppResult<bool> {
    crate::db::repo::settings::onboarded(&*state.db()?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_onboarded(state: State<'_, AppState>, done: bool) -> AppResult<()> {
    crate::db::repo::settings::set_onboarded(&*state.db()?, done)
}

/// Relaunches the app (e.g. after choosing a new data folder). Pending notes are flushed first
/// by the normal quit path.
#[tauri::command]
#[specta::specta]
pub async fn restart_app(app: tauri::AppHandle) -> AppResult<()> {
    app.request_restart();
    Ok(())
}
