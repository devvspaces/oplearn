//! Loopback HTTP server for things the webview can't load from the Tauri origin.
//!
//! - `/player`: the YouTube player bridge page. YouTube's embed rejects pages without a normal
//!   http(s) referrer (errors 152/153), and a packaged app is served from `tauri://localhost`.
//! - `/media/<token>/<rel path>`: downloaded videos under `<data_dir>/media`, with HTTP Range
//!   support. WKWebView plays custom-scheme (`asset://`) video **without audio**, so local
//!   playback uses this instead. The per-launch random token stops other local web pages from
//!   embedding the user's files; paths are restricted to the media folder.
//!
//! The app itself stays on the Tauri origin, so nothing served here has IPC access.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const PLAYER_HTML: &str = include_str!("../../assets/player.html");

/// Origins allowed to embed the player page (CSP `frame-ancestors`).
const FRAME_ANCESTORS: &str =
    "tauri://localhost http://tauri.localhost https://tauri.localhost http://localhost:1430";

const PAGE_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' https://www.youtube.com https://s.ytimg.com; \
     frame-src https://www.youtube-nocookie.com https://www.youtube.com; style-src 'unsafe-inline'; img-src https: data:";

#[derive(Debug, Clone)]
pub struct Server {
    pub port: u16,
    pub token: String,
    media_root: PathBuf,
}

impl Server {
    /// URL for a file inside the media folder, or None if `path` is outside it.
    pub fn media_url(&self, path: &Path) -> Option<String> {
        let rel = match path.strip_prefix(&self.media_root) {
            Ok(r) => r.to_path_buf(),
            // Tolerate symlinked data dirs (e.g. /var vs /private/var on macOS).
            Err(_) => {
                let root = self.media_root.canonicalize().ok()?;
                path.canonicalize()
                    .ok()?
                    .strip_prefix(&root)
                    .ok()?
                    .to_path_buf()
            }
        };
        let parts: Vec<&str> = rel.iter().map(|c| c.to_str()).collect::<Option<_>>()?;
        if parts.is_empty() || !parts.iter().all(|p| safe_component(p)) {
            return None;
        }
        Some(format!(
            "http://127.0.0.1:{}/media/{}/{}",
            self.port,
            self.token,
            parts.join("/")
        ))
    }
}

fn new_token() -> String {
    format!(
        "{:032x}{:032x}",
        ulid::Ulid::new().random(),
        ulid::Ulid::new().random()
    )
}

/// Binds to an ephemeral loopback port and serves forever on the current Tokio runtime.
pub async fn start(media_root: PathBuf) -> std::io::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let server = Server {
        port: listener.local_addr()?.port(),
        token: new_token(),
        media_root,
    };
    let shared = Arc::new(server.clone());
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let s = shared.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle(stream, &s).await {
                            log::debug!("loopback server: {e}");
                        }
                    });
                }
                Err(e) => log::warn!("loopback server accept failed: {e}"),
            }
        }
    });
    Ok(server)
}

fn safe_component(c: &str) -> bool {
    !c.is_empty()
        && c != "."
        && c != ".."
        && c.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Parses a single `Range: bytes=…` header into an inclusive (start, end).
/// `Err(())` means unsatisfiable (416); `Ok(None)` means no usable range (serve everything).
pub(crate) fn parse_range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Ok(None); // multi-range: just send the whole file
    }
    let (a, b) = spec.split_once('-').ok_or(())?;
    let (a, b) = (a.trim(), b.trim());
    if len == 0 {
        return Err(());
    }
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        (len.saturating_sub(n), len - 1)
    } else {
        let s: u64 = a.parse().map_err(|_| ())?;
        let e: u64 = if b.is_empty() {
            len - 1
        } else {
            b.parse::<u64>().map_err(|_| ())?.min(len - 1)
        };
        (s, e)
    };
    if start > end || start >= len {
        return Err(());
    }
    Ok(Some((start, end)))
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("pdf") => "application/pdf",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        // SVG in <img> can't run scripts.
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Resolves `/media/<token>/<rel>` to a file inside the media root.
fn resolve_media(server: &Server, path: &str) -> Option<PathBuf> {
    let rest = path.strip_prefix("/media/")?;
    let (token, rel) = rest.split_once('/')?;
    if token.len() != server.token.len() || token != server.token {
        return None;
    }
    let mut p = server.media_root.clone();
    for c in rel.split('/') {
        if !safe_component(c) {
            return None;
        }
        p.push(c);
    }
    let root = server.media_root.canonicalize().ok()?;
    let file = p.canonicalize().ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}

async fn read_head(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = vec![0u8; 8192];
    let mut len = 0;
    let fut = async {
        loop {
            let n = stream.read(&mut buf[len..]).await?;
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") || len == buf.len() {
                break;
            }
        }
        Ok::<_, std::io::Error>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), fut)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "request timeout"))??;
    Ok(String::from_utf8_lossy(&buf[..len]).into_owned())
}

/// CORS headers for a request from one of the app's own origins (pdf.js fetches PDFs with
/// `fetch`), empty for anyone else.
fn cors_headers(origin: Option<&str>) -> String {
    match origin {
        Some(o) if FRAME_ANCESTORS.split_whitespace().any(|a| a == o) => format!(
            "Access-Control-Allow-Origin: {o}\r\nVary: Origin\r\n\
             Access-Control-Expose-Headers: Accept-Ranges, Content-Range, Content-Length\r\n"
        ),
        _ => String::new(),
    }
}

const NOT_FOUND: &str = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

async fn handle(mut stream: TcpStream, server: &Server) -> std::io::Result<()> {
    let req = read_head(&mut stream).await?;
    let mut lines = req.lines();
    let mut parts = lines.next().unwrap_or("").split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let path = target.split('?').next().unwrap_or("");
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let range = header("range");
    let cors = cors_headers(header("origin"));

    if method == "GET" && path == "/player" {
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
             Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
             Referrer-Policy: strict-origin-when-cross-origin\r\n\
             Content-Security-Policy: {PAGE_CSP}; frame-ancestors {FRAME_ANCESTORS}\r\n\
             Connection: close\r\n\r\n{PLAYER_HTML}",
            PLAYER_HTML.len()
        );
        stream.write_all(resp.as_bytes()).await?;
    } else if method == "OPTIONS" && path.starts_with("/media/") {
        // CORS preflight from pdf.js (it sends Range), answered only for the app's origins.
        let resp = if cors.is_empty() {
            NOT_FOUND.to_string()
        } else {
            format!(
                "HTTP/1.1 204 No Content\r\n{cors}Access-Control-Allow-Methods: GET, HEAD\r\n\
                 Access-Control-Allow-Headers: Range\r\nAccess-Control-Max-Age: 600\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            )
        };
        stream.write_all(resp.as_bytes()).await?;
    } else if matches!(method, "GET" | "HEAD") && path.starts_with("/media/") {
        serve_media(&mut stream, server, path, range, method == "HEAD", &cors).await?;
    } else {
        stream.write_all(NOT_FOUND.as_bytes()).await?;
    }
    stream.shutdown().await
}

async fn serve_media(
    stream: &mut TcpStream,
    server: &Server,
    path: &str,
    range: Option<&str>,
    head_only: bool,
    cors: &str,
) -> std::io::Result<()> {
    let Some(file_path) = resolve_media(server, path) else {
        return stream.write_all(NOT_FOUND.as_bytes()).await;
    };
    let mut file = tokio::fs::File::open(&file_path).await?;
    let len = file.metadata().await?.len();
    let common = format!(
        "Content-Type: {}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n{cors}",
        content_type(&file_path)
    );
    let (status, start, count, extra) = match parse_range(range, len) {
        Err(()) => {
            let resp = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return stream.write_all(resp.as_bytes()).await;
        }
        Ok(Some((s, e))) => (
            "206 Partial Content",
            s,
            e - s + 1,
            format!("Content-Range: bytes {s}-{e}/{len}\r\n"),
        ),
        Ok(None) => ("200 OK", 0, len, String::new()),
    };
    let head = format!("HTTP/1.1 {status}\r\n{common}{extra}Content-Length: {count}\r\n\r\n");
    stream.write_all(head.as_bytes()).await?;
    if head_only || count == 0 {
        return Ok(());
    }
    file.seek(std::io::SeekFrom::Start(start)).await?;
    // The player often aborts a range mid-stream when it seeks; that surfaces as a write error.
    tokio::io::copy(&mut file.take(count), stream).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn raw(port: u16, path: &str, headers: &str) -> Vec<u8> {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{headers}\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        out
    }

    async fn get(port: u16, path: &str) -> String {
        String::from_utf8_lossy(&raw(port, path, "").await).into_owned()
    }

    fn body(resp: &[u8]) -> &[u8] {
        let i = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        &resp[i + 4..]
    }

    #[tokio::test]
    async fn serves_player_page() {
        let dir = tempfile::tempdir().unwrap();
        let s = start(dir.path().to_path_buf()).await.unwrap();
        let ok = get(s.port, "/player?v=abcdefghijk&start=10").await;
        assert!(ok.starts_with("HTTP/1.1 200"));
        assert!(ok.contains("frame-ancestors tauri://localhost"));
        assert!(ok.contains("iframe_api"));
        assert!(get(s.port, "/etc/passwd").await.starts_with("HTTP/1.1 404"));
        assert!(get(s.port, "/").await.starts_with("HTTP/1.1 404"));
    }

    #[tokio::test]
    async fn serves_media_with_ranges_inside_root_only() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("media");
        std::fs::create_dir_all(media.join("course/lesson")).unwrap();
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let file = media.join("course/lesson/video.mp4");
        std::fs::write(&file, &data).unwrap();
        std::fs::write(dir.path().join("secret.txt"), b"nope").unwrap();

        let s = start(media.clone()).await.unwrap();
        let url = s.media_url(&file).unwrap();
        let path = url
            .split_once(&format!(":{}", s.port))
            .unwrap()
            .1
            .to_string();
        assert!(path.ends_with("/course/lesson/video.mp4"));

        let full = raw(s.port, &path, "").await;
        assert!(full.starts_with(b"HTTP/1.1 200"));
        assert!(String::from_utf8_lossy(&full).contains("Content-Type: video/mp4"));
        assert_eq!(body(&full), &data[..]);

        let part = raw(s.port, &path, "Range: bytes=0-99\r\n").await;
        let head = String::from_utf8_lossy(&part);
        assert!(head.starts_with("HTTP/1.1 206"));
        assert!(head.contains("Content-Range: bytes 0-99/1000"));
        assert_eq!(body(&part), &data[..100]);

        let tail = raw(s.port, &path, "range: bytes=900-\r\n").await;
        assert_eq!(body(&tail), &data[900..]);

        let bad = raw(s.port, &path, "Range: bytes=5000-\r\n").await;
        assert!(bad.starts_with(b"HTTP/1.1 416"));

        // CORS: only the app's origins get headers, and preflight is answered for them.
        let app =
            String::from_utf8_lossy(&raw(s.port, &path, "Origin: tauri://localhost\r\n").await)
                .into_owned();
        assert!(app.contains("Access-Control-Allow-Origin: tauri://localhost"));
        assert!(app.contains("Access-Control-Expose-Headers: Accept-Ranges, Content-Range"));
        let other =
            String::from_utf8_lossy(&raw(s.port, &path, "Origin: https://evil.example\r\n").await)
                .into_owned();
        assert!(!other.contains("Access-Control-Allow-Origin"));
        let mut c = TcpStream::connect(("127.0.0.1", s.port)).await.unwrap();
        c.write_all(
            format!("OPTIONS {path} HTTP/1.1\r\nOrigin: http://localhost:1430\r\nAccess-Control-Request-Headers: range\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
        let mut pre = Vec::new();
        c.read_to_end(&mut pre).await.unwrap();
        let pre = String::from_utf8_lossy(&pre);
        assert!(
            pre.starts_with("HTTP/1.1 204") && pre.contains("Access-Control-Allow-Headers: Range")
        );

        let wrong_token = path.replace(&s.token, &"0".repeat(s.token.len()));
        assert!(get(s.port, &wrong_token).await.starts_with("HTTP/1.1 404"));
        let t = &s.token;
        for p in [
            format!("/media/{t}/../secret.txt"),
            format!("/media/{t}/course/../../secret.txt"),
            format!("/media/{t}//etc/passwd"),
            format!("/media/{t}/course%2F..%2F..%2Fsecret.txt"),
            format!("/media/{t}/course/lesson"),
        ] {
            assert!(get(s.port, &p).await.starts_with("HTTP/1.1 404"), "{p}");
        }
        assert!(s.media_url(&dir.path().join("secret.txt")).is_none());
    }

    #[test]
    fn range_parsing() {
        assert_eq!(parse_range(None, 100), Ok(None));
        assert_eq!(parse_range(Some("bytes=0-9"), 100), Ok(Some((0, 9))));
        assert_eq!(parse_range(Some("bytes=90-"), 100), Ok(Some((90, 99))));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Ok(Some((90, 99))));
        assert_eq!(parse_range(Some("bytes=0-500"), 100), Ok(Some((0, 99))));
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), Ok(None));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Err(()));
        assert_eq!(parse_range(Some("bytes=9-3"), 100), Err(()));
        assert_eq!(parse_range(Some("bytes=x-"), 100), Err(()));
    }
}
