//! PDF text extraction in a short-lived helper process.
//!
//! `pdf-extract` can panic or loop forever on malformed files. Running it as
//! `<our exe> --extract-pdf-text <path>` means a crash or hang only loses that one PDF:
//! the parent kills it after a timeout and moves on.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Hidden argument that turns the app binary into the extraction helper.
pub const HELPER_ARG: &str = "--extract-pdf-text";
/// Per-file limit before the helper is killed.
pub const TIMEOUT: Duration = Duration::from_secs(20);
/// Helpers running at once during an import.
pub const CONCURRENCY: usize = 3;

#[derive(Debug, Serialize, Deserialize)]
struct Output {
    pages: Vec<String>,
}

/// In-process extraction. Only the helper (and tests) call this directly.
pub fn extract_in_process(path: &Path) -> Result<Vec<String>, String> {
    pdf_extract::extract_text_by_pages(path).map_err(|e| e.to_string())
}

/// Entry point for helper mode: returns an exit code when `args` ask for it, else `None`.
pub fn helper_main(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some(HELPER_ARG) {
        return None;
    }
    let Some(path) = args.get(2) else {
        eprintln!("usage: {HELPER_ARG} <file.pdf>");
        return Some(2);
    };
    Some(match extract_in_process(Path::new(path)) {
        Ok(pages) => match serde_json::to_string(&Output { pages }) {
            Ok(json) => {
                println!("{json}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Err(e) => {
            eprintln!("{e}");
            1
        }
    })
}

/// Pages from a finished helper, or `None` unless it exited cleanly with valid output.
pub fn parse_output(success: bool, stdout: &[u8]) -> Option<Vec<String>> {
    if !success {
        return None;
    }
    serde_json::from_slice::<Output>(stdout)
        .ok()
        .map(|o| o.pages)
}

/// Runs the helper on one PDF. `None` on crash, error, bad output or timeout (the child is
/// killed when the future is dropped).
pub async fn extract(exe: &Path, path: &Path, timeout: Duration) -> Option<Vec<String>> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg(HELPER_ARG).arg(path);
    run(cmd, path, timeout).await
}

async fn run(
    mut cmd: tokio::process::Command,
    path: &Path,
    timeout: Duration,
) -> Option<Vec<String>> {
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) => {
            log::warn!("couldn't start the PDF helper: {e}");
            return None;
        }
    };
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => {
            let pages = parse_output(out.status.success(), &out.stdout);
            if pages.is_none() {
                log::warn!(
                    "skipped PDF {} ({}): {}",
                    path.display(),
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            pages
        }
        Ok(Err(e)) => {
            log::warn!("PDF helper failed for {}: {e}", path.display());
            None
        }
        Err(_) => {
            log::warn!("skipped PDF {} (took over {timeout:?})", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_mode_only_with_the_flag() {
        assert_eq!(helper_main(&["lectern".into()]), None);
        assert_eq!(helper_main(&["lectern".into(), "--other".into()]), None);
        assert_eq!(helper_main(&["lectern".into(), HELPER_ARG.into()]), Some(2));
        assert_eq!(
            helper_main(&[
                "lectern".into(),
                HELPER_ARG.into(),
                "/no/such/file.pdf".into()
            ]),
            Some(1)
        );
    }

    #[test]
    fn output_parsing() {
        assert_eq!(
            parse_output(true, br#"{"pages":["a","b"]}"#),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(parse_output(false, br#"{"pages":["a"]}"#), None);
        assert_eq!(parse_output(true, b"thread panicked"), None);
        assert_eq!(parse_output(true, b""), None);
    }

    #[tokio::test]
    async fn missing_helper_is_a_skip() {
        assert_eq!(
            extract(Path::new("/no/such/exe"), Path::new("x.pdf"), TIMEOUT).await,
            None
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_and_skips() {
        let mut cmd = tokio::process::Command::new("/bin/sleep");
        cmd.arg("5");
        let t = std::time::Instant::now();
        let r = run(cmd, Path::new("x.pdf"), Duration::from_millis(200)).await;
        assert_eq!(r, None);
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "the child is killed, not waited for"
        );
    }
}
