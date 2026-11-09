//! Phase 5: YouTube search (Data API with yt-dlp fallback), quota, and Google sign-in.

use chrono::Utc;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::commands::import::record_units;
use crate::db::models::*;
use crate::db::repo::{settings, yt_search};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::services::import::ImportSource;
use crate::services::oauth::{self, AccessToken};
use crate::services::secrets;
use crate::services::youtube::YouTubeApi;
use crate::state::AppState;

/// Searches YouTube. Uses the Data API when a key is set and quota remains; otherwise (or on a
/// quota error) falls back to yt-dlp, which can't filter by license or apply the other filters.
#[tauri::command]
#[specta::specta]
pub async fn search_youtube(
    state: State<'_, AppState>,
    query: YtSearchQuery,
) -> AppResult<YtSearchResponse> {
    if query.query.trim().is_empty() {
        return Err(AppError::invalid("type something to search for"));
    }
    let key = yt_search::cache_key(&query);
    let now = Utc::now();
    let (cached, quota) = {
        let c = state.db()?;
        (
            yt_search::cached(&c, &key, now)?,
            yt_search::quota(&c, now)?,
        )
    };
    if let Some(results) = cached {
        return Ok(YtSearchResponse {
            results,
            source: ImportSource::Api,
            cached: true,
            notice: None,
        });
    }
    let reason = match state.api_key()? {
        None => "No YouTube API key is set, so results come from yt-dlp".to_string(),
        Some(_) if quota.used + yt_search::SEARCH_COST > quota.limit => {
            "Today's YouTube API quota is used up, so results come from yt-dlp".to_string()
        }
        Some(api_key) => {
            let api = YouTubeApi::new(&state.http, &api_key);
            let r = api.search(&query).await;
            record_units(&state, &api);
            match r {
                Ok(results) => {
                    yt_search::store(&*state.db()?, &key, &results, now)?;
                    return Ok(YtSearchResponse {
                        results,
                        source: ImportSource::Api,
                        cached: false,
                        notice: None,
                    });
                }
                Err(e) if e.code == ErrorCode::Quota => {
                    yt_search::exhaust(&*state.db()?, now)?;
                    "The YouTube API quota ran out, so results come from yt-dlp".to_string()
                }
                Err(e) if e.code == ErrorCode::Invalid => return Err(e),
                Err(e) => {
                    log::warn!("search API failed ({e}); falling back to yt-dlp");
                    "The YouTube API failed, so results come from yt-dlp".to_string()
                }
            }
        }
    };
    let results = state
        .ytdlp()?
        .search(query.query.trim(), query.kind)
        .await?;
    let ignored = query.cc_only || query.duration.is_some() || query.upload.is_some();
    Ok(YtSearchResponse {
        results,
        source: ImportSource::Ytdlp,
        cached: false,
        notice: Some(format!(
            "{reason}. License info{} isn't available.",
            if ignored { " and filters" } else { "" }
        )),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_quota(state: State<'_, AppState>) -> AppResult<QuotaInfo> {
    yt_search::quota(&*state.db()?, Utc::now())
}

// ---- Google account ----

fn account(state: &AppState) -> AppResult<GoogleAccount> {
    let c = state.db()?;
    let client_id: Option<String> = settings::get(&c, oauth::CLIENT_ID_KEY)?;
    let channel_title: Option<String> = settings::get(&c, oauth::ACCOUNT_KEY)?;
    let signed_in = secrets::get(oauth::REFRESH_NAME).ok().flatten().is_some();
    Ok(GoogleAccount {
        configured: client_id.is_some(),
        client_id,
        signed_in,
        channel_title,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_google_account(state: State<'_, AppState>) -> AppResult<GoogleAccount> {
    account(&state)
}

/// Saves the user's own OAuth client (Google Cloud → Credentials → "Desktop app").
#[tauri::command]
#[specta::specta]
pub async fn set_google_client(
    state: State<'_, AppState>,
    client_id: String,
    client_secret: Option<String>,
) -> AppResult<GoogleAccount> {
    let id = client_id.trim();
    if !id.ends_with(".apps.googleusercontent.com") {
        return Err(AppError::invalid(
            "that doesn't look like a Google OAuth client id (…apps.googleusercontent.com)",
        ));
    }
    settings::set(&*state.db()?, oauth::CLIENT_ID_KEY, id)?;
    match client_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => secrets::set(oauth::SECRET_NAME, s)?,
        None => secrets::delete(oauth::SECRET_NAME)?,
    }
    account(&state)
}

fn client(state: &AppState) -> AppResult<(String, Option<String>)> {
    let id: Option<String> = settings::get(&*state.db()?, oauth::CLIENT_ID_KEY)?;
    let id = id.ok_or_else(|| {
        AppError::invalid("add your Google OAuth client id in Settings → Accounts first")
    })?;
    Ok((id, secrets::get(oauth::SECRET_NAME)?))
}

/// Opens the browser for consent and waits (up to 5 min) for the loopback redirect.
#[tauri::command]
#[specta::specta]
pub async fn google_sign_in(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<GoogleAccount> {
    let (client_id, secret) = client(&state)?;
    let pkce = oauth::pkce();
    let (listener, redirect) = oauth::listen().await?;
    app.opener()
        .open_url(oauth::auth_url(&client_id, &redirect, &pkce), None::<&str>)
        .map_err(|e| AppError::internal(format!("couldn't open the browser: {e}")))?;
    let code = oauth::wait_for_code(listener, &pkce.state).await?;
    let tok = oauth::exchange_code(
        &state.http,
        &client_id,
        secret.as_deref(),
        &code,
        &redirect,
        &pkce.verifier,
    )
    .await?;
    let refresh = tok
        .refresh_token
        .clone()
        .ok_or_else(|| AppError::internal("Google didn't return a refresh token; remove Lectern's access in your Google account and try again"))?;
    secrets::set(oauth::REFRESH_NAME, &refresh)?;
    let access = AccessToken::from_response(&tok);
    *state
        .google_token
        .lock()
        .map_err(|_| AppError::internal("lock poisoned"))? = Some(access.clone());
    let api = YouTubeApi::bearer(&state.http, &access.token);
    let title = api.my_channel_title().await.unwrap_or(None);
    record_units(&state, &api);
    settings::set(&*state.db()?, oauth::ACCOUNT_KEY, &title)?;
    account(&state)
}

async fn access_token(state: &AppState) -> AppResult<String> {
    if let Some(t) = state
        .google_token
        .lock()
        .map_err(|_| AppError::internal("lock poisoned"))?
        .clone()
        && t.valid()
    {
        return Ok(t.token);
    }
    let refresh = secrets::get(oauth::REFRESH_NAME)?
        .ok_or_else(|| AppError::invalid("sign in with Google first"))?;
    let (client_id, secret) = client(state)?;
    let tok = match oauth::refresh(&state.http, &client_id, secret.as_deref(), &refresh).await {
        Ok(t) => t,
        Err(e) if e.code == ErrorCode::Invalid => {
            // Revoked or expired: forget it so the UI shows "Sign in" again.
            let _ = secrets::delete(oauth::REFRESH_NAME);
            return Err(AppError::invalid(
                "your Google sign-in expired; sign in again",
            ));
        }
        Err(e) => return Err(e),
    };
    let access = AccessToken::from_response(&tok);
    *state
        .google_token
        .lock()
        .map_err(|_| AppError::internal("lock poisoned"))? = Some(access.clone());
    Ok(access.token)
}

#[tauri::command]
#[specta::specta]
pub async fn google_sign_out(state: State<'_, AppState>) -> AppResult<GoogleAccount> {
    if let Some(r) = secrets::get(oauth::REFRESH_NAME)? {
        oauth::revoke(&state.http, &r).await;
    }
    secrets::delete(oauth::REFRESH_NAME)?;
    *state
        .google_token
        .lock()
        .map_err(|_| AppError::internal("lock poisoned"))? = None;
    settings::set::<Option<String>>(&*state.db()?, oauth::ACCOUNT_KEY, &None)?;
    account(&state)
}

/// The signed-in user's playlists (1 quota unit per 50).
#[tauri::command]
#[specta::specta]
pub async fn list_my_playlists(state: State<'_, AppState>) -> AppResult<Vec<YtSearchResult>> {
    let token = access_token(&state).await?;
    let api = YouTubeApi::bearer(&state.http, &token);
    let r = api.my_playlists().await;
    record_units(&state, &api);
    r
}

/// A playlist's videos before importing it (cached 24 h).
#[tauri::command]
#[specta::specta]
pub async fn preview_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
) -> AppResult<PlaylistPreview> {
    let key = format!("playlist:{playlist_id}");
    if let Some(p) = yt_search::cached_json::<PlaylistPreview>(&*state.db()?, &key, Utc::now())? {
        return Ok(p);
    }
    let (p, _) = crate::commands::import::fetch_playlist(&state, &playlist_id).await?;
    let preview = PlaylistPreview {
        playlist_id: p.playlist_id,
        title: p.title,
        description: p.description,
        channel_title: p.channel_title,
        thumbnail_url: p.thumbnail_url,
        total_sec: p.videos.iter().filter_map(|v| v.duration_sec).sum(),
        videos: p
            .videos
            .into_iter()
            .map(|v| PreviewVideo {
                id: v.video_id,
                title: v.title,
                duration_sec: v.duration_sec,
                license: v.license,
            })
            .collect(),
        skipped: p.skipped,
    };
    yt_search::store_json(&*state.db()?, &key, &preview, Utc::now())?;
    Ok(preview)
}
