//! yt-dlp self-updater (SPEC §7.3). Downloads the latest release for this platform into
//! `<data_dir>/bin/` (the signed app bundle is read-only) after verifying its SHA-256.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use crate::db::repo::settings;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::state::AppState;

const LAST_CHECK_KEY: &str = "ytdlp_last_update_check";

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub current: Option<String>,
    pub latest: Option<String>,
    pub updated: bool,
}

/// Release asset name for the platform this binary was built for.
pub fn asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) => Some("yt-dlp_macos"),
        ("windows", "x86_64") => Some("yt-dlp.exe"),
        ("windows", "aarch64") => Some("yt-dlp_arm64.exe"),
        ("linux", "x86_64") => Some("yt-dlp_linux"),
        ("linux", "aarch64") => Some("yt-dlp_linux_aarch64"),
        _ => None,
    }
}

/// yt-dlp versions are dates like `2026.10.26` (optionally `.1`); compare numerically.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse =
        |s: &str| -> Vec<u32> { s.trim().split('.').filter_map(|p| p.parse().ok()).collect() };
    parse(latest) > parse(current)
}

pub fn checksum_for<'a>(listing: &'a str, name: &str) -> Option<&'a str> {
    listing.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (hash, file) = (it.next()?, it.next()?);
        (file.trim_start_matches('*') == name).then_some(hash)
    })
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

pub async fn check(state: &AppState, force: bool) -> AppResult<UpdateResult> {
    if !force {
        let last: Option<String> = settings::get(&*state.db()?, LAST_CHECK_KEY)?;
        if let Some(t) = last.and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok())
            && chrono::Utc::now().signed_duration_since(t) < chrono::Duration::hours(24)
        {
            return Ok(UpdateResult {
                current: None,
                latest: None,
                updated: false,
            });
        }
    }
    let current = match state.ytdlp() {
        Ok(y) => y.version().await.ok(),
        Err(_) => None,
    };
    let release: Release = state
        .http
        .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    settings::set(&*state.db()?, LAST_CHECK_KEY, &crate::db::now())?;
    let latest = release.tag_name;
    if current.as_deref().is_some_and(|c| !is_newer(&latest, c)) {
        return Ok(UpdateResult {
            current,
            latest: Some(latest),
            updated: false,
        });
    }
    let asset =
        asset_name().ok_or_else(|| AppError::invalid("no yt-dlp build for this platform"))?;
    let base = format!("https://github.com/yt-dlp/yt-dlp/releases/download/{latest}");
    let sums = state
        .http
        .get(format!("{base}/SHA2-256SUMS"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected = checksum_for(&sums, asset)
        .ok_or_else(|| AppError::new(ErrorCode::Http, "release has no checksum for this platform"))?
        .to_lowercase();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let bytes = client
        .get(format!("{base}/{asset}"))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != expected {
        return Err(AppError::new(
            ErrorCode::Http,
            "downloaded yt-dlp failed checksum verification",
        ));
    }
    let dir = state.paths.bin_dir();
    std::fs::create_dir_all(&dir)?;
    let name = if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    let tmp = dir.join(format!("{name}.download"));
    std::fs::write(&tmp, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, dir.join(name))?;
    log::info!("updated yt-dlp {current:?} -> {latest}");
    Ok(UpdateResult {
        current,
        latest: Some(latest),
        updated: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("2026.11.02", "2026.10.26"));
        assert!(is_newer("2026.10.26.1", "2026.10.26"));
        assert!(!is_newer("2026.10.26", "2026.10.26"));
        assert!(!is_newer("2026.09.01", "2026.10.26"));
    }

    #[test]
    fn finds_checksums() {
        let l = "abc  yt-dlp\ndef  yt-dlp_macos\n";
        assert_eq!(checksum_for(l, "yt-dlp_macos"), Some("def"));
        assert_eq!(checksum_for(l, "nope"), None);
        assert!(asset_name().is_some());
    }
}
