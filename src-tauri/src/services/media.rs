//! Local media helpers: ffmpeg/ffprobe (verification, frame capture) and disk usage.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths::locate_binary;

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Ffmpeg {
    pub fn locate(bin_dir: &Path) -> AppResult<Self> {
        match (
            locate_binary("ffmpeg", bin_dir),
            locate_binary("ffprobe", bin_dir),
        ) {
            (Some(ffmpeg), Some(ffprobe)) => Ok(Self { ffmpeg, ffprobe }),
            _ => Err(AppError::new(
                ErrorCode::YtDlpMissing,
                "ffmpeg was not found. Run `pnpm fetch-binaries` (development) or reinstall Lectern.",
            )),
        }
    }

    /// Directory to pass to `yt-dlp --ffmpeg-location` (it finds ffprobe next to ffmpeg).
    pub fn dir(&self) -> Option<&Path> {
        self.ffmpeg.parent()
    }

    async fn run(bin: &Path, args: &[&std::ffi::OsStr], timeout: Duration) -> AppResult<String> {
        let mut cmd = Command::new(bin);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let out = tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| AppError::new(ErrorCode::Timeout, "ffmpeg timed out"))??;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(AppError::new(
                ErrorCode::Internal,
                format!("ffmpeg failed: {}", err.lines().last().unwrap_or("")),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Container duration in seconds.
    pub async fn probe_duration(&self, file: &Path) -> AppResult<f64> {
        let out = Self::run(
            &self.ffprobe,
            &[
                "-v".as_ref(),
                "error".as_ref(),
                "-show_entries".as_ref(),
                "format=duration".as_ref(),
                "-of".as_ref(),
                "default=nw=1:nk=1".as_ref(),
                file.as_os_str(),
            ],
            Duration::from_secs(60),
        )
        .await?;
        out.trim()
            .parse::<f64>()
            .map_err(|_| AppError::internal("ffprobe returned no duration"))
    }

    /// Height of the first video stream in pixels.
    pub async fn probe_height(&self, file: &Path) -> AppResult<u32> {
        let out = Self::run(
            &self.ffprobe,
            &[
                "-v".as_ref(),
                "error".as_ref(),
                "-select_streams".as_ref(),
                "v:0".as_ref(),
                "-show_entries".as_ref(),
                "stream=height".as_ref(),
                "-of".as_ref(),
                "default=nw=1:nk=1".as_ref(),
                file.as_os_str(),
            ],
            Duration::from_secs(60),
        )
        .await?;
        out.lines()
            .next()
            .and_then(|l| l.trim().parse::<u32>().ok())
            .ok_or_else(|| AppError::internal("ffprobe returned no height"))
    }

    /// Saves the frame at `sec` as PNG.
    pub async fn capture_frame(&self, video: &Path, sec: f64, out: &Path) -> AppResult<()> {
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let ts = format!("{:.3}", sec.max(0.0));
        Self::run(
            &self.ffmpeg,
            &[
                "-v".as_ref(),
                "error".as_ref(),
                "-y".as_ref(),
                "-ss".as_ref(),
                ts.as_ref(),
                "-i".as_ref(),
                video.as_os_str(),
                "-frames:v".as_ref(),
                "1".as_ref(),
                out.as_os_str(),
            ],
            Duration::from_secs(60),
        )
        .await?;
        if out.is_file() {
            Ok(())
        } else {
            Err(AppError::internal("ffmpeg produced no frame"))
        }
    }
}

/// SPEC §7.3 invariant: a download only counts if it is a real file of plausible size whose
/// duration matches the lesson (±3%, at least ±10 s). Unknown expected duration skips that check.
pub fn verify_download(
    size_bytes: u64,
    probed_sec: f64,
    expected_sec: Option<f64>,
) -> Result<(), String> {
    if size_bytes < 1_000_000 {
        return Err(format!("file is too small ({size_bytes} bytes)"));
    }
    if !(probed_sec.is_finite() && probed_sec > 1.0) {
        return Err("file has no playable duration".into());
    }
    if let Some(exp) = expected_sec.filter(|e| *e > 0.0) {
        let tolerance = (exp * 0.03).max(10.0);
        if (probed_sec - exp).abs() > tolerance {
            return Err(format!(
                "duration {probed_sec:.0}s doesn't match the lesson ({exp:.0}s)"
            ));
        }
    }
    Ok(())
}

/// Display label for a video height: snaps to the usual YouTube rungs ("720p"), since
/// letterboxed videos report odd heights (e.g. 718 or 1072).
pub fn quality_label(height: u32) -> String {
    const RUNGS: [u32; 8] = [144, 240, 360, 480, 720, 1080, 1440, 2160];
    let near = RUNGS
        .iter()
        .copied()
        .min_by_key(|r| r.abs_diff(height))
        .unwrap_or(height);
    let h = if near.abs_diff(height) * 100 <= near * 8 {
        near
    } else {
        height
    };
    format!("{h}p")
}

/// Finds the finished video in a lesson folder (`video.<ext>`, ignoring partial/temporary files).
pub fn find_video(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut found: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("video.")
                && !name.ends_with(".part")
                && !name.ends_with(".ytdl")
                && !name.contains(".temp.")
                && !name.contains(".f") // per-format pieces like video.f136.mp4
                && matches!(p.extension().and_then(|e| e.to_str()), Some("mp4" | "mkv" | "webm" | "m4v"))
        })
        .collect();
    found.sort_by_key(|p| p.extension().and_then(|e| e.to_str()) != Some("mp4"));
    found.into_iter().next()
}

/// Total size of a directory tree in bytes (0 if missing).
pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Removes `dir` only if it lives inside `root` (defence against bad paths).
pub fn remove_dir_within(root: &Path, dir: &Path) -> AppResult<()> {
    if !dir.exists() {
        return Ok(());
    }
    let root = root.canonicalize()?;
    let target = dir.canonicalize()?;
    if !target.starts_with(&root) || target == root {
        return Err(AppError::invalid(
            "refusing to delete outside the media folder",
        ));
    }
    std::fs::remove_dir_all(target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_labels() {
        assert_eq!(quality_label(720), "720p");
        assert_eq!(quality_label(718), "720p");
        assert_eq!(quality_label(1072), "1080p");
        assert_eq!(quality_label(360), "360p");
        assert_eq!(quality_label(600), "600p");
    }

    #[test]
    fn verification_rules() {
        assert!(verify_download(500, 100.0, Some(100.0)).is_err());
        assert!(verify_download(5_000_000, 0.0, Some(100.0)).is_err());
        assert!(verify_download(5_000_000, 4775.0, Some(4780.0)).is_ok());
        assert!(
            verify_download(5_000_000, 4775.0, Some(4900.0)).is_ok(),
            "within 3%"
        );
        assert!(
            verify_download(5_000_000, 600.0, Some(4775.0)).is_err(),
            "truncated"
        );
        assert!(
            verify_download(5_000_000, 95.0, Some(100.0)).is_ok(),
            "±10 s floor for short videos"
        );
        assert!(verify_download(5_000_000, 300.0, None).is_ok());
    }

    #[test]
    fn finds_finished_video_and_sizes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("video.mp4.part"), b"x").unwrap();
        std::fs::write(d.path().join("video.f136.mp4"), b"x").unwrap();
        assert_eq!(find_video(d.path()), None);
        std::fs::write(d.path().join("video.webm"), b"abc").unwrap();
        std::fs::write(d.path().join("video.mp4"), b"abcd").unwrap();
        assert_eq!(
            find_video(d.path()).unwrap().file_name().unwrap(),
            "video.mp4"
        );
        std::fs::create_dir(d.path().join("frames")).unwrap();
        std::fs::write(d.path().join("frames/a.png"), b"12345").unwrap();
        assert_eq!(dir_size(d.path()), 1 + 1 + 3 + 4 + 5);
    }

    #[test]
    fn refuses_to_delete_outside_root() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        assert!(remove_dir_within(root.path(), other.path()).is_err());
        assert!(remove_dir_within(root.path(), root.path()).is_err());
        let inner = root.path().join("course/lesson");
        std::fs::create_dir_all(&inner).unwrap();
        remove_dir_within(root.path(), &inner).unwrap();
        assert!(!inner.exists());
    }
}
