//! Shared search → preview → import commands for NPTEL, OpenStax, LibreTexts and Open Yale.
//! (MIT OCW keeps its own commands in `ocw.rs`.) Nothing is fetched until the user searches,
//! previews or clicks Import; catalogues are cached for a day.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;
use tauri::ipc::Channel;

use crate::error::{AppError, AppResult};
use crate::services::providers::common::{self, Polite};
use crate::services::providers::{
    ImportProgress, Provider, ProviderCard, ProviderImportReport, ProviderPreview, build,
    libretexts, nptel, openstax, yale,
};
use crate::state::AppState;

fn polite(state: &AppState, p: Provider) -> Arc<Polite> {
    let gap = match p {
        Provider::Nptel => nptel::GAP,
        Provider::Openstax => openstax::GAP,
        Provider::Libretexts => libretexts::GAP,
        Provider::Yale => yale::GAP,
        _ => std::time::Duration::from_millis(250),
    };
    Polite::new(&state.http, gap)
}

fn unsupported() -> AppError {
    AppError::invalid("use the YouTube or MIT OCW tab for that provider")
}

fn key(p: Provider) -> String {
    format!("catalog:{}", p.as_str())
}

async fn nptel_catalog(state: &AppState) -> AppResult<Vec<ProviderCard>> {
    let pol = polite(state, Provider::Nptel);
    common::catalogue(state, &key(Provider::Nptel), || async move {
        nptel::catalog(&pol).await
    })
    .await
}

async fn openstax_catalog(state: &AppState) -> AppResult<Vec<openstax::Book>> {
    let pol = polite(state, Provider::Openstax);
    common::catalogue(state, &key(Provider::Openstax), || async move {
        openstax::catalog(&pol).await
    })
    .await
}

async fn libretexts_catalog(state: &AppState) -> AppResult<Vec<libretexts::Book>> {
    let pol = polite(state, Provider::Libretexts);
    common::catalogue(state, &key(Provider::Libretexts), || async move {
        libretexts::catalog(&pol).await
    })
    .await
}

async fn yale_catalog(state: &AppState) -> AppResult<Vec<yale::Listed>> {
    let pol = polite(state, Provider::Yale);
    common::catalogue(state, &key(Provider::Yale), || async move {
        yale::catalog(&pol).await
    })
    .await
}

/// Searches the provider's catalogue locally (an empty query lists the first results).
#[tauri::command]
#[specta::specta]
pub async fn provider_search(
    state: State<'_, AppState>,
    provider: Provider,
    query: String,
) -> AppResult<Vec<ProviderCard>> {
    let q = query.trim();
    Ok(match provider {
        Provider::Nptel => {
            let all = nptel_catalog(&state).await?;
            common::rank(q, &all, |c| {
                (
                    c.title.clone(),
                    format!(
                        "{} {}",
                        c.subtitle.clone().unwrap_or_default(),
                        c.tags.join(" ")
                    ),
                )
            })
        }
        Provider::Openstax => {
            let all = openstax_catalog(&state).await?;
            common::rank(q, &all, |b| {
                (
                    b.title.clone(),
                    format!("{} {}", b.subjects.join(" "), b.authors.join(" ")),
                )
            })
            .iter()
            .map(openstax::card)
            .collect()
        }
        Provider::Libretexts => {
            let all = libretexts_catalog(&state).await?;
            common::rank(q, &all, |b| {
                (
                    b.title.clone(),
                    [&b.author, &b.affiliation, &b.subject, &b.library]
                        .into_iter()
                        .flatten()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            })
            .iter()
            .map(libretexts::card)
            .collect()
        }
        Provider::Yale => {
            let all = yale_catalog(&state).await?;
            common::rank(q, &all, |c| {
                (
                    c.title.clone(),
                    format!("{} {} {}", c.number, c.department, c.professor),
                )
            })
            .iter()
            .map(yale::card)
            .collect()
        }
        _ => return Err(unsupported()),
    })
}

/// A course id, or a pasted link to one.
fn normalize(provider: Provider, input: &str) -> AppResult<String> {
    let s = input.trim();
    let bad = || {
        AppError::invalid(format!(
            "that doesn't look like a {} course",
            provider.label()
        ))
    };
    match provider {
        Provider::Nptel => nptel::parse_id(s).ok_or_else(bad),
        Provider::Yale => yale::parse_id(s).ok_or_else(bad),
        Provider::Openstax => {
            let slug = s
                .split("/books/")
                .nth(1)
                .map(|r| r.split('/').next().unwrap_or(r))
                .unwrap_or(s);
            let ok = !slug.is_empty()
                && slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            ok.then(|| slug.to_string()).ok_or_else(bad)
        }
        Provider::Libretexts => {
            let id = s.trim_end_matches('/').rsplit('/').next().unwrap_or(s);
            let ok = id.contains('-') && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            ok.then(|| id.to_string()).ok_or_else(bad)
        }
        _ => Err(unsupported()),
    }
}

async fn openstax_book(state: &AppState, slug: &str) -> AppResult<openstax::Book> {
    openstax_catalog(state)
        .await?
        .into_iter()
        .find(|b| b.slug == slug)
        .ok_or_else(|| AppError::not_found("that OpenStax book"))
}

async fn libretexts_book(state: &AppState, id: &str) -> AppResult<libretexts::Book> {
    libretexts_catalog(state)
        .await?
        .into_iter()
        .find(|b| b.id == id)
        .ok_or_else(|| AppError::not_found("that LibreTexts book"))
}

#[tauri::command]
#[specta::specta]
pub async fn provider_preview(
    state: State<'_, AppState>,
    provider: Provider,
    id: String,
) -> AppResult<ProviderPreview> {
    let id = normalize(provider, &id)?;
    let pol = polite(&state, provider);
    match provider {
        Provider::Nptel => Ok(nptel::preview_of(&nptel::course(&pol, &id).await?)),
        Provider::Openstax => {
            let l = openstax::load(&pol, openstax_book(&state, &id).await?).await?;
            let pdf = match &l.book.pdf_url {
                Some(u) => pol.size(u).await,
                None => None,
            };
            Ok(openstax::preview_of(&l, pdf))
        }
        Provider::Libretexts => {
            let l = libretexts::load(&pol, libretexts_book(&state, &id).await?).await?;
            let size = match &l.book.zip {
                Some(u) => pol.size(u).await,
                None => None,
            };
            Ok(libretexts::preview_of(&l, size))
        }
        Provider::Yale => Ok(yale::preview_of(&yale::course(&pol, &id).await?)),
        _ => Err(unsupported()),
    }
}

/// Fetches the course, builds it, moves its files under the course's media folder and indexes
/// PDFs. `with_pdf` also downloads the full-book PDF (OpenStax).
#[tauri::command]
#[specta::specta]
pub async fn provider_import(
    state: State<'_, AppState>,
    provider: Provider,
    id: String,
    with_pdf: bool,
    on_progress: Channel<ImportProgress>,
) -> AppResult<ProviderImportReport> {
    let id = normalize(provider, &id)?;
    if let Ok(existing) = state.db()?.query_row(
        "SELECT id FROM courses WHERE provider = ?1 AND provider_ref = ?2",
        [provider.as_str(), &id],
        |r| r.get::<_, String>(0),
    ) {
        return Err(AppError::invalid(format!(
            "this course is already in your library (id {existing})"
        )));
    }
    let staging = common::staging_dir(
        &state,
        &format!("{}-{}", provider.as_str(), id.replace('/', "-")),
    )
    .await?;
    let r = run_import(&state, provider, &id, with_pdf, &staging, &on_progress).await;
    if r.is_err() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    r
}

async fn run_import(
    state: &AppState,
    provider: Provider,
    id: &str,
    with_pdf: bool,
    staging: &std::path::Path,
    ch: &Channel<ImportProgress>,
) -> AppResult<ProviderImportReport> {
    let pol = polite(state, provider);
    let on = |stage: &str, d: f64, t: f64, m: &str| common::progress(ch, stage, d, t, m);
    common::progress(ch, "download", 0.0, 0.0, "Reading the course outline…");
    let plan = match provider {
        Provider::Nptel => nptel::plan(&pol, &nptel::course(&pol, id).await?, &on).await?,
        Provider::Openstax => {
            let l = openstax::load(&pol, openstax_book(state, id).await?).await?;
            openstax::plan(&pol, &l, staging, with_pdf, &on).await?
        }
        Provider::Libretexts => {
            let l = libretexts::load(&pol, libretexts_book(state, id).await?).await?;
            libretexts::plan(&pol, &l, staging, &on).await?
        }
        Provider::Yale => yale::plan(&pol, &yale::course(&pol, id).await?, staging, &on).await?,
        _ => return Err(unsupported()),
    };
    if plan.lesson_count() == 0 {
        return Err(AppError::invalid(
            "this course has no lessons Lectern can use",
        ));
    }
    common::progress(ch, "build", 0.0, 0.0, "Building lessons and modules…");
    let written = build::write(&*state.db()?, &plan, staging)?;
    let final_dir = common::relocate(state, &written.course_id, staging, provider.as_str()).await?;
    let pdfs: Vec<(String, PathBuf)> = written
        .pdfs
        .iter()
        .filter_map(|(mid, p)| Some((mid.clone(), final_dir.join(p.strip_prefix(staging).ok()?))))
        .collect();
    let indexed = common::index_pdfs(state, pdfs, ch).await?;
    common::progress(ch, "done", 1.0, 1.0, "Done");
    Ok(ProviderImportReport {
        course_id: written.course_id,
        lessons: written.lessons,
        materials: written.materials,
        transcripts: written.transcripts,
        pdfs_indexed: indexed,
        skipped: plan.skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_links_normalize() {
        assert_eq!(
            normalize(
                Provider::Openstax,
                "https://openstax.org/details/books/calculus-volume-1"
            )
            .unwrap(),
            "calculus-volume-1"
        );
        assert_eq!(
            normalize(
                Provider::Openstax,
                "https://openstax.org/books/calculus-volume-1/pages/1-introduction"
            )
            .unwrap(),
            "calculus-volume-1"
        );
        assert_eq!(
            normalize(
                Provider::Libretexts,
                "https://commons.libretexts.org/book/human-354234"
            )
            .unwrap(),
            "human-354234"
        );
        assert_eq!(
            normalize(Provider::Nptel, "https://nptel.ac.in/courses/106106145").unwrap(),
            "106106145"
        );
        assert!(normalize(Provider::Openstax, "../etc/passwd").is_err());
        assert!(normalize(Provider::Youtube, "x").is_err());
    }
}
