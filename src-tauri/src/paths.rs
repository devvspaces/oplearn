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

impl Paths {
    pub fn resolve(config_dir: &Path) -> AppResult<Self> {
        let bootstrap_file = config_dir.join("bootstrap.json");
        let bootstrap: Bootstrap = std::fs::read_to_string(&bootstrap_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let data_dir = bootstrap.data_dir.unwrap_or_else(default_data_dir);
        let paths = Self {
            bootstrap_file,
            data_dir,
        };
        paths.ensure()?;
        Ok(paths)
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
}

fn default_data_dir() -> PathBuf {
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
}
