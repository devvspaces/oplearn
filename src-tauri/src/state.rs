use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::paths::Paths;
use crate::services::{secrets, ytdlp::YtDlp};

pub struct AppState {
    db: Mutex<Connection>,
    pub paths: Paths,
    pub http: reqwest::Client,
    /// Loopback server hosting the YouTube player bridge page and local media.
    pub loopback: crate::services::player_server::Server,
    /// Cached so the keychain is read at most once per run (avoids repeated OS prompts in dev builds).
    api_key: Mutex<Option<Option<String>>>,
    /// Google OAuth access token (short-lived; the refresh token is in the keychain).
    pub google_token: Mutex<Option<crate::services::oauth::AccessToken>>,
}

impl AppState {
    pub fn new(paths: Paths, loopback: crate::services::player_server::Server) -> AppResult<Self> {
        let db = crate::db::open(&paths.db_file())?;
        // The data folder may have moved (e.g. to an external drive) since paths were stored.
        crate::db::repo::relocate::rebase_and_log(&db, &paths.data_dir);
        let http = reqwest::Client::builder()
            .user_agent(concat!("Lectern/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            db: Mutex::new(db),
            paths,
            http,
            loopback,
            api_key: Mutex::new(None),
            google_token: Mutex::new(None),
        })
    }

    pub fn db(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|_| AppError::internal("database lock poisoned"))
    }

    /// Locates yt-dlp and applies the user's cookies setting. Briefly locks the DB, so never
    /// call it while holding `db()`.
    pub fn ytdlp(&self) -> AppResult<YtDlp> {
        let mut y = YtDlp::locate(&self.paths.bin_dir())?;
        y.cookies_browser = crate::db::repo::settings::get::<String>(
            &*self.db()?,
            crate::services::ytdlp::COOKIES_BROWSER_KEY,
        )
        .ok()
        .flatten()
        .filter(|b| crate::services::ytdlp::COOKIE_BROWSERS.contains(&b.as_str()));
        Ok(y)
    }

    pub fn api_key(&self) -> AppResult<Option<String>> {
        let mut cache = self
            .api_key
            .lock()
            .map_err(|_| AppError::internal("lock poisoned"))?;
        if cache.is_none() {
            *cache = Some(secrets::get(secrets::YOUTUBE_API_KEY).unwrap_or_else(|e| {
                log::warn!("keychain read failed: {e}");
                None
            }));
        }
        Ok(cache.clone().flatten())
    }

    pub fn set_api_key(&self, key: Option<&str>) -> AppResult<()> {
        match key {
            Some(k) => secrets::set(secrets::YOUTUBE_API_KEY, k)?,
            None => secrets::delete(secrets::YOUTUBE_API_KEY)?,
        }
        *self
            .api_key
            .lock()
            .map_err(|_| AppError::internal("lock poisoned"))? = Some(key.map(str::to_string));
        Ok(())
    }
}
