//! Resolution of the data directory and the files inside it.
//!
//! The data directory (default `~/Lectern`) is user-configurable so a library can
//! live on an external drive. Its location is stored in a small bootstrap file in
//! the OS config dir, because the database itself lives inside the data dir.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Bootstrap {
    data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub bootstrap_file: PathBuf,
    pub data_dir: PathBuf,
}

/// Why the configured data folder can't be used right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// The folder is on a drive (`/Volumes/<name>`) that isn't mounted.
    MissingVolume { volume: PathBuf },
    /// The folder couldn't be created or opened (read-only drive, permissions…).
    NotWritable(String),
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingVolume { volume } => {
                write!(f, "drive {} is not connected", volume.display())
            }
            Self::NotWritable(e) => write!(f, "{e}"),
        }
    }
}

impl Paths {
    pub fn resolve(config_dir: &Path) -> AppResult<Self> {
        let paths = Self::load(config_dir);
        paths.check().map_err(|e| {
            crate::error::AppError::new(
                crate::error::ErrorCode::Io,
                format!("data folder {}: {e}", paths.data_dir.display()),
            )
        })?;
        Ok(paths)
    }

    /// Reads the bootstrap file. Touches nothing in the data folder.
    pub fn load(config_dir: &Path) -> Self {
        let bootstrap_file = config_dir.join("bootstrap.json");
        let bootstrap: Bootstrap = std::fs::read_to_string(&bootstrap_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            bootstrap_file,
            data_dir: bootstrap.data_dir.unwrap_or_else(default_data_dir),
        }
    }

    /// Makes sure the data folder is usable, creating its subfolders. A folder on an
    /// unmounted drive is reported **before** anything is created, so we never make a
    /// stray `/Volumes/<name>` folder that would look like the drive.
    pub fn check(&self) -> Result<(), Unavailable> {
        self.check_with_volumes(Path::new("/Volumes"))
    }

    fn check_with_volumes(&self, volumes: &Path) -> Result<(), Unavailable> {
        if let Some(volume) = volume_root(&self.data_dir, volumes)
            && !volume.is_dir()
        {
            return Err(Unavailable::MissingVolume { volume });
        }
        self.ensure()
            .map_err(|e| Unavailable::NotWritable(e.to_string()))
    }

    /// For tests: everything under one root.
    pub fn at(root: &Path) -> AppResult<Self> {
        let paths = Self {
            bootstrap_file: root.join("bootstrap.json"),
            data_dir: root.join("data"),
        };
        paths.ensure()?;
        Ok(paths)
    }

    fn ensure(&self) -> AppResult<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.media_dir())?;
        std::fs::create_dir_all(self.bin_dir())?;
        Ok(())
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("library.db")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    pub fn media_dir(&self) -> PathBuf {
        self.data_dir.join("media")
    }
    /// Binaries updated at runtime (e.g. yt-dlp) live here, because the app bundle is read-only.
    pub fn bin_dir(&self) -> PathBuf {
        self.data_dir.join("bin")
    }

    /// Persist a new data dir. Takes effect on next launch (the DB is reopened from there).
    pub fn set_data_dir(&self, dir: &Path) -> AppResult<()> {
        if let Some(parent) = self.bootstrap_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let b = Bootstrap {
            data_dir: Some(dir.to_path_buf()),
        };
        std::fs::write(&self.bootstrap_file, serde_json::to_string_pretty(&b)?)?;
        Ok(())
    }

    /// Go back to the default data dir (`~/Lectern`) from the next resolve on.
    pub fn clear_data_dir(&self) -> AppResult<()> {
        if let Some(parent) = self.bootstrap_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let b = Bootstrap { data_dir: None };
        std::fs::write(&self.bootstrap_file, serde_json::to_string_pretty(&b)?)?;
        Ok(())
    }
}

/// `<volumes>/<name>` when `path` lives on a mounted-drive path such as `/Volumes/SEA DRIVE/…`.
fn volume_root(path: &Path, volumes: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(volumes).ok()?;
    let name = rest.components().next()?;
    Some(volumes.join(name))
}

pub fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Lectern")
}

/// Finds a sidecar binary (`yt-dlp`, `ffmpeg`, `ffprobe`). Resolution order:
/// 1. `<data_dir>/bin/<name>`: copies installed by the in-app updater (the app bundle is read-only);
/// 2. next to the executable: bundled sidecars (`bundle.externalBin`), or one level up when running
///    as a `cargo test` binary from `target/<profile>/deps`;
/// 3. `PATH` (development convenience; GUI apps on macOS get a minimal PATH).
pub fn locate_binary(name: &str, bin_dir: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let updated = bin_dir.join(&exe);
    if updated.is_file() {
        return Some(updated);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        for d in [Some(dir.as_path()), dir.parent()].into_iter().flatten() {
            let p = d.join(&exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(&exe))
        .find(|p| p.is_file())
}

/// Lowercase, ASCII-only, dash-separated slug.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.len() > 60 {
        out.truncate(60);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() { "course".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(
            slugify("MIT 6.824: Distributed Systems (Spring 2020)"),
            "mit-6-824-distributed-systems-spring-2020"
        );
        assert_eq!(slugify("  ---  "), "course");
        assert_eq!(slugify("Ünïcode Title"), "n-code-title");
    }

    #[test]
    fn volume_root_only_for_volume_paths() {
        let v = Path::new("/Volumes");
        assert_eq!(
            volume_root(Path::new("/Volumes/SEA DRIVE/Lectern"), v),
            Some(PathBuf::from("/Volumes/SEA DRIVE"))
        );
        assert_eq!(volume_root(Path::new("/Users/me/Lectern"), v), None);
        assert_eq!(volume_root(Path::new("/Volumes"), v), None);
    }

    #[test]
    fn missing_volume_is_reported_without_creating_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let volumes = tmp.path().join("Volumes");
        std::fs::create_dir_all(&volumes).unwrap();
        let p = Paths {
            bootstrap_file: tmp.path().join("bootstrap.json"),
            data_dir: volumes.join("SEA DRIVE").join("Lectern"),
        };
        assert_eq!(
            p.check_with_volumes(&volumes),
            Err(Unavailable::MissingVolume {
                volume: volumes.join("SEA DRIVE")
            })
        );
        assert!(!volumes.join("SEA DRIVE").exists());

        std::fs::create_dir(volumes.join("SEA DRIVE")).unwrap();
        assert_eq!(p.check_with_volumes(&volumes), Ok(()));
        assert!(p.media_dir().is_dir());
    }

    #[test]
    fn set_and_clear_data_dir_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::load(tmp.path());
        p.set_data_dir(Path::new("/elsewhere/Lectern")).unwrap();
        assert_eq!(
            Paths::load(tmp.path()).data_dir,
            PathBuf::from("/elsewhere/Lectern")
        );
        p.clear_data_dir().unwrap();
        assert_eq!(Paths::load(tmp.path()).data_dir, default_data_dir());
    }
}
