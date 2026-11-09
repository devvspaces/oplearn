// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Helper mode: extract one PDF's text and exit (see services::pdf_text).
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = lectern_lib::services::pdf_text::helper_main(&args) {
        std::process::exit(code);
    }
    lectern_lib::run()
}
