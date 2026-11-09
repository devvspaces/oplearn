//! The single place that spawns yt-dlp (SPEC §14.4).
//!
//! Binary resolution order:
//! 1. `<data_dir>/bin/yt-dlp` — a copy installed by the in-app updater (the app bundle is read-only)
//! 2. the bundled sidecar next to the executable (`bundle.externalBin`)
//! 3. `yt-dlp` on `PATH` (handy in development)
//!
//! Cancellation: every call is a future; dropping/aborting it kills the child (`kill_on_drop`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;

use crate::db::models::{License, SearchKind, YtSearchResult};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::services::import::{ImportedPlaylist, ImportedVideo};

#[derive(Debug, Clone)]
pub struct YtDlp {
    pub binary: PathBuf,
    /// Browser whose YouTube cookies yt-dlp reads (`--cookies-from-browser`). Off by default
    /// (SPEC §4.5, §7.9); set from Settings → Downloads → Advanced.
    pub cookies_browser: Option<String>,
    /// JavaScript runtime for YouTube's "n" challenge (`--js-runtimes name:path`). Without one,
    /// signed-in (cookie) requests only get storyboard images.
    pub js_runtime: Option<(String, PathBuf)>,
}

/// Settings key holding the browser name for `--cookies-from-browser` (absent = off).
pub const COOKIES_BROWSER_KEY: &str = "ytdlp_cookies_browser";

/// Browsers accepted for `--cookies-from-browser`.
pub const COOKIE_BROWSERS: &[&str] = &[
    "chrome", "firefox", "safari", "edge", "brave", "chromium", "opera", "vivaldi",
];

impl YtDlp {
    pub fn locate(bin_dir: &Path) -> AppResult<Self> {
        crate::paths::locate_binary("yt-dlp", bin_dir)
            .map(|binary| Self {
                binary,
                cookies_browser: None,
                js_runtime: find_js_runtime(bin_dir),
            })
            .ok_or_else(|| {
            AppError::new(
                ErrorCode::YtDlpMissing,
                "yt-dlp was not found. Run `pnpm fetch-binaries` (development) or reinstall Lectern.",
            )
        })
    }

    /// Arguments every yt-dlp call starts with. Cookies are only sent when `with_cookies`.
    pub fn base_args(&self, with_cookies: bool) -> Vec<String> {
        let mut a = vec!["--ignore-config".to_string()];
        if let Some((name, path)) = &self.js_runtime {
            a.push("--js-runtimes".into());
            a.push(format!("{name}:{}", path.display()));
        }
        if with_cookies && let Some(b) = &self.cookies_browser {
            a.push("--cookies-from-browser".into());
            a.push(b.clone());
        }
        a
    }

    fn classify(&self, stderr: &str, with_cookies: bool) -> AppError {
        classify_error_with(
            stderr,
            self.cookies_browser.as_deref().filter(|_| with_cookies),
            self.js_runtime.is_some(),
        )
    }

    /// Cookies are a fallback: the first attempt goes without them, and only YouTube's sign-in
    /// check (with cookies configured) triggers a second attempt with them.
    fn retry_with_cookies(&self, e: &AppError, with_cookies: bool) -> bool {
        !with_cookies && self.cookies_browser.is_some() && e.code == ErrorCode::SignInRequired
    }

    async fn run(&self, args: &[&str], timeout: Duration) -> AppResult<String> {
        match self.run_once(args, timeout, false).await {
            Err(e) if self.retry_with_cookies(&e, false) => {
                log::info!("YouTube asked for sign-in; retrying with browser cookies");
                self.run_once(args, timeout, true).await
            }
            r => r,
        }
    }

    async fn run_once(
        &self,
        args: &[&str],
        timeout: Duration,
        with_cookies: bool,
    ) -> AppResult<String> {
        let mut cmd = Command::new(&self.binary);
        if args == ["--version"] {
            cmd.arg("--ignore-config");
        } else {
            cmd.args(self.base_args(with_cookies));
        }
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let child = cmd
            .spawn()
            .map_err(|e| AppError::new(ErrorCode::YtDlp, format!("failed to start yt-dlp: {e}")))?;
        let out = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| AppError::new(ErrorCode::Timeout, "yt-dlp timed out"))??;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(self.classify(&stderr, with_cookies));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub async fn version(&self) -> AppResult<String> {
        Ok(self
            .run(&["--version"], Duration::from_secs(30))
            .await?
            .trim()
            .to_string())
    }

    /// Fast playlist listing: titles, ids and (usually) durations, no per-video requests.
    pub async fn dump_playlist_flat(&self, playlist_id: &str) -> AppResult<ImportedPlaylist> {
        let url = format!("https://www.youtube.com/playlist?list={playlist_id}");
        let json = self
            .run(
                &[
                    "--flat-playlist",
                    "--dump-single-json",
                    "--no-warnings",
                    &url,
                ],
                Duration::from_secs(180),
            )
            .await?;
        parse_flat_playlist(&json, playlist_id)
    }

    pub async fn dump_video(&self, video_id: &str) -> AppResult<ImportedVideo> {
        let url = format!("https://www.youtube.com/watch?v={video_id}");
        let json = self
            .run(
                &[
                    "--dump-single-json",
                    "--skip-download",
                    "--no-playlist",
                    "--no-warnings",
                    &url,
                ],
                Duration::from_secs(120),
            )
            .await?;
        parse_video(&json)
    }
}

impl YtDlp {
    /// Search fallback when the Data API is unavailable: no license data, no filters.
    pub async fn search(&self, query: &str, kind: SearchKind) -> AppResult<Vec<YtSearchResult>> {
        let target = match kind {
            SearchKind::Video => format!("ytsearch25:{query}"),
            // YouTube's own "Type: Playlist" results filter.
            SearchKind::Playlist => format!(
                "https://www.youtube.com/results?search_query={}&sp=EgIQAw%253D%253D",
                url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>()
            ),
        };
        let json = self
            .run(
                &[
                    "--flat-playlist",
                    "--dump-single-json",
                    "--no-warnings",
                    "--playlist-end",
                    "25",
                    &target,
                ],
                Duration::from_secs(90),
            )
            .await?;
        parse_search(&json, kind)
    }
}

#[derive(Debug, Deserialize)]
struct SearchEntry {
    id: Option<String>,
    title: Option<String>,
    url: Option<String>,
    duration: Option<f64>,
    channel: Option<String>,
    uploader: Option<String>,
    view_count: Option<f64>,
    #[serde(default)]
    thumbnails: Vec<Thumbnail>,
    playlist_count: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct SearchDump {
    #[serde(default)]
    entries: Vec<Option<SearchEntry>>,
}

pub fn parse_search(json: &str, kind: SearchKind) -> AppResult<Vec<YtSearchResult>> {
    let d: SearchDump = serde_json::from_str(json)
        .map_err(|e| AppError::new(ErrorCode::YtDlp, format!("unexpected yt-dlp output: {e}")))?;
    Ok(d.entries
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let id = match kind {
                SearchKind::Video => e.id.filter(|i| i.len() == 11)?,
                SearchKind::Playlist => e
                    .url
                    .as_deref()
                    .and_then(|u| crate::services::import::parse_youtube_url(u)?.playlist_id)
                    .or(e.id.filter(|i| i.len() > 11))?,
            };
            Some(YtSearchResult {
                kind,
                thumbnail_url: match kind {
                    SearchKind::Video => Some(crate::services::import::thumbnail_url(&id)),
                    SearchKind::Playlist => e
                        .thumbnails
                        .iter()
                        .max_by_key(|t| t.width.unwrap_or(0))
                        .map(|t| t.url.clone()),
                },
                id,
                title: e.title.unwrap_or_default(),
                channel_title: e.channel.or(e.uploader),
                duration_sec: e.duration.map(|d| d.round() as i32),
                view_count: e.view_count,
                license: None,
                published_at: None,
                item_count: e.playlist_count,
            })
        })
        .collect())
}

impl YtDlp {
    /// Lists available subtitle tracks and chapters without downloading anything.
    pub async fn probe_subtitles(&self, video_id: &str) -> AppResult<SubtitleProbe> {
        let url = format!("https://www.youtube.com/watch?v={video_id}");
        let json = self
            .run(
                &[
                    "--dump-single-json",
                    "--skip-download",
                    "--no-playlist",
                    "--no-warnings",
                    &url,
                ],
                Duration::from_secs(120),
            )
            .await?;
        parse_subtitle_probe(&json)
    }

    /// Downloads exactly one subtitle track as VTT into `out_dir` and returns its path.
    /// Only one track is requested: asking for several (or for machine translations) is what
    /// triggers YouTube's HTTP 429 on the timedtext endpoint.
    pub async fn download_subtitle(
        &self,
        video_id: &str,
        track: &str,
        kind: TrackKind,
        out_dir: &Path,
    ) -> AppResult<PathBuf> {
        std::fs::create_dir_all(out_dir)?;
        let url = format!("https://www.youtube.com/watch?v={video_id}");
        let template = out_dir.join("%(id)s.%(ext)s");
        let template = template.to_string_lossy();
        let flag = match kind {
            TrackKind::Manual => "--write-subs",
            TrackKind::Auto => "--write-auto-subs",
        };
        let lang_re = regex::escape(track);
        self.run(
            &[
                "--skip-download",
                "--no-playlist",
                "--no-warnings",
                flag,
                "--sub-langs",
                &lang_re,
                "--sub-format",
                "vtt",
                "-o",
                &template,
                &url,
            ],
            Duration::from_secs(180),
        )
        .await?;
        let path = out_dir.join(format!("{video_id}.{track}.vtt"));
        if path.is_file() {
            Ok(path)
        } else {
            Err(AppError::new(
                ErrorCode::YtDlp,
                "yt-dlp finished but no subtitle file was written",
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Manual,
    Auto,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterInfo {
    pub start: f64,
    pub end: Option<f64>,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubtitleProbe {
    /// The video's spoken language as reported by YouTube (e.g. `en-US`).
    pub language: Option<String>,
    pub manual: Vec<String>,
    pub auto: Vec<String>,
    pub chapters: Vec<ChapterInfo>,
}

#[derive(Debug, Deserialize)]
struct ProbeJson {
    language: Option<String>,
    #[serde(default)]
    subtitles: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    automatic_captions: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    chapters: Option<Vec<ProbeChapter>>,
}

#[derive(Debug, Deserialize)]
struct ProbeChapter {
    start_time: f64,
    end_time: Option<f64>,
    title: Option<String>,
}

pub fn parse_subtitle_probe(json: &str) -> AppResult<SubtitleProbe> {
    let p: ProbeJson = serde_json::from_str(json)
        .map_err(|e| AppError::new(ErrorCode::YtDlp, format!("unexpected yt-dlp output: {e}")))?;
    let keys = |m: Option<serde_json::Map<String, serde_json::Value>>| -> Vec<String> {
        m.map(|m| {
            m.into_iter()
                .filter(|(k, _)| k != "live_chat")
                .map(|(k, _)| k)
                .collect()
        })
        .unwrap_or_default()
    };
    Ok(SubtitleProbe {
        language: p.language,
        manual: keys(p.subtitles),
        auto: keys(p.automatic_captions),
        chapters: p
            .chapters
            .unwrap_or_default()
            .into_iter()
            .map(|c| ChapterInfo {
                start: c.start_time,
                end: c.end_time,
                title: c.title.unwrap_or_default(),
            })
            .filter(|c| !c.title.trim().is_empty())
            .collect(),
    })
}

fn lang_matches(key: &str, pref: &str) -> bool {
    key.eq_ignore_ascii_case(pref)
        || key
            .to_ascii_lowercase()
            .starts_with(&format!("{}-", pref.to_ascii_lowercase()))
}

/// Picks the best subtitle track for the preferred languages:
/// 1. a human-made track in a preferred language;
/// 2. the original-language auto-captions (`<lang>-orig`) in a preferred language;
/// 3. the original-language auto-captions in any language (better than nothing);
/// 4. a machine-translated auto track in a preferred language (most likely to be rate-limited).
pub fn choose_track(p: &SubtitleProbe, prefs: &[String]) -> Option<(String, TrackKind)> {
    for pref in prefs {
        if let Some(k) = p.manual.iter().find(|k| lang_matches(k, pref)) {
            return Some((k.clone(), TrackKind::Manual));
        }
    }
    for pref in prefs {
        if let Some(k) = p
            .auto
            .iter()
            .find(|k| k.ends_with("-orig") && lang_matches(k, pref))
        {
            return Some((k.clone(), TrackKind::Auto));
        }
    }
    if let Some(k) = p.auto.iter().find(|k| k.ends_with("-orig")) {
        return Some((k.clone(), TrackKind::Auto));
    }
    for pref in prefs {
        if let Some(k) = p.auto.iter().find(|k| k.eq_ignore_ascii_case(pref)) {
            return Some((k.clone(), TrackKind::Auto));
        }
    }
    None
}

// ------------------------------------------------------------------------------------ downloads

/// Download quality presets (SPEC §7.3). Stored as text like `720p`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum Quality {
    #[serde(rename = "360p")]
    P360,
    #[serde(rename = "480p")]
    P480,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "1080p")]
    P1080,
}

impl Quality {
    pub fn height(self) -> u32 {
        match self {
            Self::P360 => 360,
            Self::P480 => 480,
            Self::P720 => 720,
            Self::P1080 => 1080,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P360 => "360p",
            Self::P480 => "480p",
            Self::P720 => "720p",
            Self::P1080 => "1080p",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "360p" => Self::P360,
            "480p" => Self::P480,
            "720p" => Self::P720,
            "1080p" => Self::P1080,
            _ => return None,
        })
    }
    /// Rough average bitrate (bits/s) for size estimates of H.264 lecture video + AAC audio.
    pub fn est_bitrate(self) -> f64 {
        match self {
            Self::P360 => 0.6e6,
            Self::P480 => 1.0e6,
            Self::P720 => 1.8e6,
            Self::P1080 => 3.5e6,
        }
    }
    /// yt-dlp format selector: prefer H.264 + AAC (plays everywhere), then anything ≤ height.
    pub fn format_selector(self) -> String {
        let h = self.height();
        format!(
            "bv*[height<={h}][vcodec^=avc1]+ba[acodec^=mp4a]/b[height<={h}][vcodec^=avc1]/\
             bv*[height<={h}]+ba/b[height<={h}]/wv*+ba/w"
        )
    }
}

pub const PROGRESS_PREFIX: &str = "LECTERN|";
pub const SIZE_PREFIX: &str = "LECTERNSIZE|";

/// A progress snapshot combined across the separate video and audio downloads.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressSnapshot {
    pub fraction: f64,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub speed_bps: Option<f64>,
    pub eta_sec: Option<u64>,
}

/// Folds yt-dlp output lines into overall progress. yt-dlp downloads the video and the audio
/// stream one after the other, each reporting its own byte counts; we know the expected
/// combined size from a `before_dl` print, so the bar moves monotonically from 0 to 1.
#[derive(Debug, Default)]
pub struct ProgressTracker {
    expected_total: Option<u64>,
    finished_bytes: u64,
    current_format: Option<String>,
    current_done: u64,
    current_total: Option<u64>,
}

fn num(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s == "NA" || s == "None" {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

impl ProgressTracker {
    /// Feeds one stdout line; returns a snapshot when progress changed.
    pub fn feed(&mut self, line: &str) -> Option<ProgressSnapshot> {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(SIZE_PREFIX) {
            self.expected_total = num(rest).map(|v| v as u64).filter(|v| *v > 0);
            return None;
        }
        let rest = line.strip_prefix(PROGRESS_PREFIX)?;
        let f: Vec<&str> = rest.split('|').collect();
        if f.len() < 6 {
            return None;
        }
        let downloaded = num(f[0]).unwrap_or(0.0) as u64;
        let total = num(f[1]).or_else(|| num(f[2])).map(|v| v as u64);
        let speed = num(f[3]);
        let format = f[5].trim().to_string();
        if self.current_format.as_deref() != Some(format.as_str()) {
            if self.current_format.is_some() {
                self.finished_bytes += self
                    .current_total
                    .unwrap_or(self.current_done)
                    .max(self.current_done);
            }
            self.current_format = Some(format);
            self.current_done = 0;
        }
        self.current_done = downloaded;
        self.current_total = total;
        let done = self.finished_bytes + downloaded;
        let total_all = self
            .expected_total
            .filter(|t| *t >= done)
            .or_else(|| total.map(|t| self.finished_bytes + t));
        let fraction = match total_all {
            Some(t) if t > 0 => (done as f64 / t as f64).min(0.995),
            _ => 0.0,
        };
        let eta = match (total_all, speed) {
            (Some(t), Some(sp)) if sp > 0.0 && t >= done => {
                Some(((t - done) as f64 / sp).round() as u64)
            }
            _ => num(f[4]).map(|v| v as u64),
        };
        Some(ProgressSnapshot {
            fraction,
            bytes_done: done,
            bytes_total: total_all,
            speed_bps: speed,
            eta_sec: eta,
        })
    }
}

/// Why a download stopped early (requested by the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Pause,
    Cancel,
}

pub enum DownloadOutcome {
    Completed,
    Stopped(StopReason),
}

pub struct DownloadOptions<'a> {
    pub video_id: &'a str,
    pub quality: Quality,
    pub out_dir: &'a Path,
    /// Directory containing ffmpeg + ffprobe.
    pub ffmpeg_dir: Option<&'a Path>,
}

impl YtDlp {
    /// Downloads one video (resumable via `.part` files) into `out_dir/video.<ext>`, with
    /// `video.jpg` (thumbnail) and `info.info.json`. Calls `on_progress` for each progress line, and stops
    /// (killing yt-dlp) when `stop` receives a reason.
    pub async fn download_video(
        &self,
        o: &DownloadOptions<'_>,
        mut on_progress: impl FnMut(ProgressSnapshot),
        stop: tokio::sync::watch::Receiver<Option<StopReason>>,
    ) -> AppResult<DownloadOutcome> {
        match self
            .download_video_once(o, &mut on_progress, stop.clone(), false)
            .await
        {
            Err(e) if self.retry_with_cookies(&e, false) => {
                log::info!("YouTube asked for sign-in; retrying the download with browser cookies");
                self.download_video_once(o, &mut on_progress, stop, true)
                    .await
            }
            r => r,
        }
    }

    async fn download_video_once(
        &self,
        o: &DownloadOptions<'_>,
        on_progress: &mut impl FnMut(ProgressSnapshot),
        mut stop: tokio::sync::watch::Receiver<Option<StopReason>>,
        with_cookies: bool,
    ) -> AppResult<DownloadOutcome> {
        use tokio::io::{AsyncBufReadExt, BufReader};
        std::fs::create_dir_all(o.out_dir)?;
        let out = |name: &str| o.out_dir.join(name).to_string_lossy().into_owned();
        let url = format!("https://www.youtube.com/watch?v={}", o.video_id);
        let template = format!(
            "download:{PROGRESS_PREFIX}%(progress.downloaded_bytes)s|%(progress.total_bytes)s|\
             %(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.format_id)s"
        );
        let mut cmd = Command::new(&self.binary);
        cmd.args(self.base_args(with_cookies))
            .args([
                "--no-playlist",
                "--no-warnings",
                "--continue",
                "--no-overwrites",
                "--newline",
            ])
            .args([
                "--retries",
                "5",
                "--fragment-retries",
                "10",
                "--socket-timeout",
                "30",
            ])
            .args([
                "-f",
                &o.quality.format_selector(),
                "--merge-output-format",
                "mp4",
                // moov atom first, so local playback starts immediately.
                "--postprocessor-args",
                "Merger+ffmpeg_o:-movflags +faststart",
            ])
            .args([
                "--write-thumbnail",
                "--convert-thumbnails",
                "jpg",
                "--write-info-json",
            ])
            .args(["-o", &out("video.%(ext)s")])
            .args(["-o", &format!("infojson:{}", out("info"))])
            // Expected combined size, printed before downloading; --print implies --simulate otherwise.
            .args([
                "--print",
                &format!("before_dl:{SIZE_PREFIX}%(filesize,filesize_approx)s"),
                "--no-simulate",
            ])
            .args(["--progress", "--progress-template", &template]);
        if let Some(dir) = o.ffmpeg_dir {
            cmd.arg("--ffmpeg-location").arg(dir);
        }
        cmd.arg(&url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::new(ErrorCode::YtDlp, format!("failed to start yt-dlp: {e}")))?;
        let mut stdout = BufReader::new(child.stdout.take().expect("piped")).lines();
        let mut stderr = child.stderr.take().expect("piped");
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await;
            buf
        });
        let mut tracker = ProgressTracker::default();
        let reason = loop {
            tokio::select! {
                line = stdout.next_line() => match line? {
                    Some(l) => {
                        if let Some(snap) = tracker.feed(&l) {
                            on_progress(snap);
                        }
                    }
                    None => break None,
                },
                changed = stop.changed() => {
                    if changed.is_ok() && let Some(r) = *stop.borrow() {
                        break Some(r);
                    }
                }
            }
        };
        if let Some(r) = reason {
            let _ = child.kill().await;
            return Ok(DownloadOutcome::Stopped(r));
        }
        let status = child.wait().await?;
        let err = stderr_task.await.unwrap_or_default();
        if status.success() {
            Ok(DownloadOutcome::Completed)
        } else {
            Err(self.classify(&err, with_cookies))
        }
    }
}

/// A JavaScript runtime yt-dlp can use for YouTube's challenges: deno, node or bun, looked up like
/// the other binaries, then in common install folders (macOS GUI apps get a minimal PATH).
pub fn find_js_runtime(bin_dir: &Path) -> Option<(String, PathBuf)> {
    const NAMES: [&str; 3] = ["deno", "node", "bun"];
    for name in NAMES {
        if let Some(p) = crate::paths::locate_binary(name, bin_dir) {
            return Some((name.to_string(), p));
        }
    }
    let home = dirs::home_dir()?;
    let mut dirs = vec![
        home.join(".deno/bin"),
        home.join(".bun/bin"),
        home.join(".volta/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    // nvm: newest installed version first.
    if let Ok(rd) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("bin")).collect();
        v.sort_by_key(|p| std::cmp::Reverse(node_version(p)));
        dirs.extend(v);
    }
    for name in NAMES {
        let exe = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        };
        if let Some(p) = dirs.iter().map(|d| d.join(&exe)).find(|p| p.is_file()) {
            return Some((name.to_string(), p));
        }
    }
    None
}

/// `(major, minor, patch)` from an nvm `.../vX.Y.Z/bin` path, for sorting.
fn node_version(bin: &Path) -> (u32, u32, u32) {
    let v = bin
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .trim_start_matches('v');
    let mut it = v.split('.').map(|x| x.parse().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// Turns yt-dlp stderr into a human-readable error (SPEC §7.3).
pub fn classify_error(stderr: &str) -> AppError {
    classify_error_with(stderr, None, true)
}

/// Like [`classify_error`]. `cookies_browser` is the browser whose cookies were sent (if any) and
/// `has_js` whether a JS runtime was available; both tailor the advice.
pub fn classify_error_with(stderr: &str, cookies_browser: Option<&str>, has_js: bool) -> AppError {
    let s = stderr.to_lowercase().replace('\u{2019}', "'");
    // YouTube's "Sign in to confirm you're not a bot" check for IPs it thinks are automated.
    if s.contains("confirm you're not a bot") || s.contains("sign in to confirm") {
        let msg = match cookies_browser {
            None => {
                "YouTube wants a sign-in check for this network (\"confirm you're not a bot\"). \
                     Wait a while and retry, or turn on Settings → Downloads → Advanced → \
                     Use browser cookies."
                    .to_string()
            }
            Some(b) => format!(
                "YouTube still asked for a sign-in check. Make sure you're signed in to YouTube in \
                 {b}, then retry."
            ),
        };
        return AppError::new(ErrorCode::SignInRequired, msg);
    }
    // Signed-in requests need the JS challenge solved; without a runtime only storyboards remain.
    if !has_js
        && (s.contains("only images are available")
            || s.contains("requested format is not available"))
    {
        return AppError::new(
            ErrorCode::YtDlp,
            "YouTube only offers this video to clients that can run JavaScript. Install Node.js \
             (nodejs.org) or Deno, then retry.",
        );
    }
    let last = stderr
        .lines()
        .rev()
        .find(|l| l.contains("ERROR"))
        .unwrap_or(stderr.trim())
        .trim();
    let (code, msg) = if s.contains("429") || s.contains("too many requests") {
        (
            ErrorCode::Http,
            "YouTube is rate-limiting requests right now. Try again in a few minutes.",
        )
    } else if s.contains("private video") {
        (ErrorCode::YtDlp, "This video is private.")
    } else if s.contains("confirm your age") || s.contains("age-restricted") {
        (
            ErrorCode::YtDlp,
            "This video is age-restricted and needs a signed-in account.",
        )
    } else if s.contains("not available in your country") || s.contains("geo restrict") {
        (
            ErrorCode::YtDlp,
            "This video is not available in your region.",
        )
    } else if s.contains("video unavailable")
        || s.contains("has been removed")
        || s.contains("does not exist")
    {
        (
            ErrorCode::NotFound,
            "This video or playlist is unavailable or has been removed.",
        )
    } else if s.contains("unable to download")
        || s.contains("getaddrinfo")
        || s.contains("timed out")
        || s.contains("network")
    {
        (ErrorCode::Http, "Network error while contacting YouTube.")
    } else {
        return AppError::new(ErrorCode::YtDlp, format!("yt-dlp failed: {last}"));
    };
    AppError::new(code, msg)
}

#[derive(Debug, Deserialize)]
struct FlatEntry {
    id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    duration: Option<f64>,
    channel: Option<String>,
    uploader: Option<String>,
    availability: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Thumbnail {
    url: String,
    width: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct FlatPlaylist {
    id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    channel: Option<String>,
    uploader: Option<String>,
    #[serde(default)]
    thumbnails: Vec<Thumbnail>,
    #[serde(default)]
    entries: Vec<Option<FlatEntry>>,
}

fn entry_available(e: &FlatEntry) -> bool {
    let t = e.title.as_deref().unwrap_or("");
    !matches!(t, "[Private video]" | "[Deleted video]" | "")
        && !matches!(
            e.availability.as_deref(),
            Some("private" | "needs_auth" | "subscriber_only" | "premium_only")
        )
}

pub fn parse_flat_playlist(json: &str, playlist_id: &str) -> AppResult<ImportedPlaylist> {
    let p: FlatPlaylist = serde_json::from_str(json)
        .map_err(|e| AppError::new(ErrorCode::YtDlp, format!("unexpected yt-dlp output: {e}")))?;
    let mut skipped = 0;
    let mut videos = Vec::new();
    for e in p.entries.into_iter().flatten() {
        let Some(id) = e.id.clone().filter(|_| entry_available(&e)) else {
            skipped += 1;
            continue;
        };
        videos.push(ImportedVideo {
            video_id: id,
            title: e.title.unwrap_or_default(),
            description: e.description.filter(|d| !d.trim().is_empty()),
            channel_title: e.channel.or(e.uploader),
            duration_sec: e.duration.map(|d| d.round() as i32),
            license: None, // not exposed by flat listing
        });
    }
    let thumbnail_url = p
        .thumbnails
        .iter()
        .max_by_key(|t| t.width.unwrap_or(0))
        .map(|t| t.url.clone());
    Ok(ImportedPlaylist {
        playlist_id: p.id.unwrap_or_else(|| playlist_id.to_string()),
        title: p.title.unwrap_or_else(|| "Untitled playlist".into()),
        description: p.description.filter(|d| !d.trim().is_empty()),
        channel_title: p.channel.or(p.uploader),
        thumbnail_url,
        videos,
        skipped,
    })
}

#[derive(Debug, Deserialize)]
struct FullVideo {
    id: String,
    title: String,
    description: Option<String>,
    duration: Option<f64>,
    channel: Option<String>,
    uploader: Option<String>,
    license: Option<String>,
}

pub fn parse_video(json: &str) -> AppResult<ImportedVideo> {
    let v: FullVideo = serde_json::from_str(json)
        .map_err(|e| AppError::new(ErrorCode::YtDlp, format!("unexpected yt-dlp output: {e}")))?;
    // yt-dlp reports e.g. "Creative Commons Attribution license (reuse allowed)"; absent means standard license.
    let license = match v.license.as_deref() {
        Some(l) if l.to_ascii_lowercase().contains("creative commons") => License::CreativeCommon,
        _ => License::Youtube,
    };
    Ok(ImportedVideo {
        video_id: v.id,
        title: v.title,
        description: v.description.filter(|d| !d.trim().is_empty()),
        channel_title: v.channel.or(v.uploader),
        duration_sec: v.duration.map(|d| d.round() as i32),
        license: Some(license),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_results() {
        let v = r#"{"entries": [
            {"id": "aaaaaaaaaaa", "title": "Raft lecture", "duration": 3600.4, "channel": "MIT", "view_count": 1200},
            {"id": "UCxxxxxxxxxxxxxxxxxxxxxx", "title": "A channel"},
            null
        ]}"#;
        let r = parse_search(v, SearchKind::Video).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].duration_sec, Some(3600));
        assert!(
            r[0].thumbnail_url
                .as_deref()
                .unwrap()
                .contains("aaaaaaaaaaa")
        );
        let p = r#"{"entries": [{"id": "PLabcdefghijk123", "title": "Course", "url": "https://www.youtube.com/playlist?list=PLabcdefghijk123",
            "thumbnails": [{"url": "s.jpg", "width": 10}, {"url": "b.jpg", "width": 300}]}]}"#;
        let r = parse_search(p, SearchKind::Playlist).unwrap();
        assert_eq!(r[0].id, "PLabcdefghijk123");
        assert_eq!(r[0].thumbnail_url.as_deref(), Some("b.jpg"));
    }

    #[test]
    fn parses_flat_playlist_fixture() {
        let json = r#"{
          "id": "PLabcdefghijk", "title": "MIT 6.824", "description": "", "channel": "MIT",
          "thumbnails": [{"url": "small.jpg", "width": 168}, {"url": "big.jpg", "width": 720}],
          "entries": [
            {"id": "aaaaaaaaaaa", "title": "Lecture 1: Introduction", "duration": 4813.0, "channel": "MIT"},
            {"id": "bbbbbbbbbbb", "title": "[Private video]", "duration": null},
            null,
            {"id": "ccccccccccc", "title": "Lecture 2: RPC and Threads", "duration": 4711.4, "uploader": "MIT"}
          ]
        }"#;
        let p = parse_flat_playlist(json, "PLabcdefghijk").unwrap();
        assert_eq!(p.title, "MIT 6.824");
        assert_eq!(p.description, None);
        assert_eq!(p.thumbnail_url.as_deref(), Some("big.jpg"));
        assert_eq!(p.skipped, 1);
        assert_eq!(p.videos.len(), 2);
        assert_eq!(p.videos[1].duration_sec, Some(4711));
        assert_eq!(p.videos[1].channel_title.as_deref(), Some("MIT"));
    }

    #[test]
    fn parses_video_license() {
        let cc = r#"{"id":"aaaaaaaaaaa","title":"T","duration":10,"license":"Creative Commons Attribution license (reuse allowed)"}"#;
        assert_eq!(
            parse_video(cc).unwrap().license,
            Some(License::CreativeCommon)
        );
        let std = r#"{"id":"aaaaaaaaaaa","title":"T"}"#;
        assert_eq!(parse_video(std).unwrap().license, Some(License::Youtube));
    }

    #[test]
    fn quality_selectors() {
        assert!(
            Quality::P720
                .format_selector()
                .starts_with("bv*[height<=720][vcodec^=avc1]+ba[acodec^=mp4a]")
        );
        assert_eq!(Quality::parse("1080p"), Some(Quality::P1080));
        assert_eq!(Quality::parse("4k"), None);
    }

    #[test]
    fn tracks_progress_across_video_and_audio() {
        let mut t = ProgressTracker::default();
        assert_eq!(t.feed("LECTERNSIZE|1000"), None);
        let s = t.feed("LECTERN|0|900|NA|100|9|136").unwrap();
        assert_eq!(s.fraction, 0.0);
        let s = t.feed("LECTERN|450|900|NA|150|3|136").unwrap();
        assert_eq!(s.bytes_done, 450);
        assert!((s.fraction - 0.45).abs() < 1e-9);
        assert_eq!(
            s.eta_sec,
            Some(((1000 - 450) as f64 / 150.0).round() as u64)
        );
        t.feed("LECTERN|900|900|NA|150|0|136");
        // Audio starts: overall keeps rising instead of dropping back to 0.
        let s = t.feed("LECTERN|50|100|NA|100|0|140").unwrap();
        assert_eq!(s.bytes_done, 950);
        assert!((s.fraction - 0.95).abs() < 1e-9);
        let s = t.feed("LECTERN|100|100|NA|NA|NA|140").unwrap();
        assert!(s.fraction <= 0.995, "never shows 100% before merging");
        assert_eq!(t.feed("[Merger] Merging formats"), None);
    }

    #[test]
    fn progress_without_expected_size_or_totals() {
        let mut t = ProgressTracker::default();
        let s = t.feed("LECTERN|200|NA|1000|NA|NA|18").unwrap();
        assert!(
            (s.fraction - 0.2).abs() < 1e-9,
            "falls back to the estimate"
        );
        let s = t.feed("LECTERN|300|NA|NA|NA|NA|18").unwrap();
        assert_eq!(s.bytes_total, None);
        assert_eq!(s.fraction, 0.0);
        assert!(t.feed("LECTERN|garbage").is_none());
    }

    #[test]
    fn chooses_subtitle_tracks() {
        let json = r#"{"language":"en-US","subtitles":{"live_chat":[],"de":[]},
            "automatic_captions":{"en":[],"en-orig":[],"fr":[]},
            "chapters":[{"start_time":0,"end_time":478,"title":"Distributed Systems"},{"start_time":478,"title":"Course Overview"}]}"#;
        let p = parse_subtitle_probe(json).unwrap();
        assert_eq!(p.chapters.len(), 2);
        assert_eq!(p.chapters[1].end, None);
        let en = vec!["en".to_string()];
        assert_eq!(
            choose_track(&p, &en),
            Some(("en-orig".into(), TrackKind::Auto))
        );
        let de = vec!["de".to_string(), "en".to_string()];
        assert_eq!(
            choose_track(&p, &de),
            Some(("de".into(), TrackKind::Manual))
        );
        let manual = SubtitleProbe {
            manual: vec!["en-GB".into()],
            auto: vec!["en-orig".into()],
            ..Default::default()
        };
        assert_eq!(
            choose_track(&manual, &en),
            Some(("en-GB".into(), TrackKind::Manual))
        );
        let foreign = SubtitleProbe {
            auto: vec!["uk-orig".into(), "en".into()],
            ..Default::default()
        };
        assert_eq!(
            choose_track(&foreign, &en),
            Some(("uk-orig".into(), TrackKind::Auto))
        );
        let translated_only = SubtitleProbe {
            auto: vec!["en".into()],
            ..Default::default()
        };
        assert_eq!(
            choose_track(&translated_only, &en),
            Some(("en".into(), TrackKind::Auto))
        );
        assert_eq!(choose_track(&SubtitleProbe::default(), &en), None);
    }

    #[test]
    fn base_args_add_cookies_only_when_set() {
        let mut y = YtDlp {
            binary: "yt-dlp".into(),
            cookies_browser: None,
            js_runtime: None,
        };
        assert_eq!(y.base_args(true), vec!["--ignore-config"]);
        y.cookies_browser = Some("firefox".into());
        assert_eq!(y.base_args(false), vec!["--ignore-config"]);
        assert_eq!(
            y.base_args(true),
            vec!["--ignore-config", "--cookies-from-browser", "firefox"]
        );
        y.js_runtime = Some(("node".into(), "/x/node".into()));
        assert_eq!(
            y.base_args(false),
            vec!["--ignore-config", "--js-runtimes", "node:/x/node"]
        );
        let bot = classify_error("ERROR: Sign in to confirm you're not a bot");
        assert!(y.retry_with_cookies(&bot, false));
        assert!(!y.retry_with_cookies(&bot, true));
        assert!(!y.retry_with_cookies(&classify_error("ERROR: Private video"), false));
        y.cookies_browser = None;
        assert!(!y.retry_with_cookies(&bot, false));
    }

    #[test]
    fn classifies_errors() {
        assert!(classify_error("ERROR: Unable to download video subtitles for 'en': HTTP Error 429: Too Many Requests")
            .message
            .contains("rate-limiting"));
        assert_eq!(
            classify_error("ERROR: [youtube] x: Private video. Sign in").message,
            "This video is private."
        );
        assert_eq!(
            classify_error("ERROR: [youtube] x: Video unavailable").code,
            ErrorCode::NotFound
        );
        let bot = "ERROR: [youtube] UzzcUS2OHqo: Sign in to confirm you\u{2019}re not a bot. This helps \
                   protect our community. Use --cookies-from-browser or --cookies for the authentication.";
        let e = classify_error(bot);
        assert_eq!(e.code, ErrorCode::SignInRequired);
        assert!(e.message.contains("Use browser cookies"));
        let no_js = classify_error_with(
            "ERROR: [youtube] x: Requested format is not available",
            None,
            false,
        );
        assert!(no_js.message.contains("Node.js"));
        assert!(
            !classify_error("ERROR: [youtube] x: Requested format is not available")
                .message
                .contains("Node.js")
        );
        assert_eq!(
            node_version(Path::new("/h/.nvm/versions/node/v24.10.0/bin")),
            (24, 10, 0)
        );
        let e = classify_error_with(bot, Some("firefox"), true);
        assert_eq!(e.code, ErrorCode::SignInRequired);
        assert!(e.message.contains("firefox"));
        assert_eq!(
            classify_error("ERROR: [youtube] x: Sign in to confirm you're not a bot").code,
            ErrorCode::SignInRequired
        );
        assert!(
            classify_error("ERROR: something odd")
                .message
                .contains("something odd")
        );
    }
}
