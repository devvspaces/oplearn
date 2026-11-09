//! Graceful shutdown: before the app exits, ask the renderer to flush pending writes
//! (e.g. the current playback position), then exit. A timeout guarantees we never hang.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

static FLUSHING: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);

pub const FLUSH_EVENT: &str = "flush-request";

/// Returns true if the exit should be prevented now (a flush was started).
pub fn begin(app: &AppHandle) -> bool {
    if READY.load(Ordering::SeqCst) {
        return false;
    }
    if FLUSHING.swap(true, Ordering::SeqCst) {
        return true; // already in progress
    }
    let _ = app.emit(FLUSH_EVENT, ());
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        finish(&handle);
    });
    true
}

pub fn finish(app: &AppHandle) {
    if !READY.swap(true, Ordering::SeqCst) {
        app.exit(0);
    }
}
