//! Runs the real app binary in PDF-helper mode.

use std::path::Path;

use lectern_lib::services::pdf_text::{TIMEOUT, extract};

const EXE: &str = env!("CARGO_BIN_EXE_lectern");

#[tokio::test]
async fn malformed_pdf_is_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.pdf");
    std::fs::write(&bad, b"%PDF-1.4\n garbage that is not a pdf \n%%EOF").unwrap();
    assert_eq!(extract(Path::new(EXE), &bad, TIMEOUT).await, None);
}

/// `PDF_PATH=/path/to/file.pdf cargo test --test pdf_helper -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn real_pdf_through_the_helper() {
    let path = std::env::var("PDF_PATH").expect("set PDF_PATH");
    let pages = extract(Path::new(EXE), Path::new(&path), TIMEOUT)
        .await
        .expect("helper returned pages");
    println!("{} pages; first: {:.120}", pages.len(), pages[0].trim());
    assert!(!pages.is_empty());
}
