//! Lectern: turn YouTube playlists into courses you own.

pub mod commands;
pub mod db;
pub mod error;
pub mod paths;
pub mod services;
pub mod shutdown;
pub mod state;

use tauri::Manager;
use tauri_specta::{Builder, ErrorHandlingMode, collect_commands, collect_events};

use crate::commands::{
    ai, ai_assist, ai_study, annotations, backup, downloads, flashcards, import, library, ocw,
    player, providers, search, settings, study, transcripts, youtube,
};

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .error_handling(ErrorHandlingMode::Throw)
        .disable_serde_phases()
        .events(collect_events![
            services::downloads::DownloadUpdate,
            services::downloads::DownloadsChanged,
            services::downloads::LessonMediaChanged,
            study::LessonCompleted,
        ])
        .commands(collect_commands![
            library::list_courses,
            library::get_course_detail,
            library::create_course,
            library::update_course,
            library::delete_course,
            library::create_module,
            library::rename_module,
            library::delete_module,
            library::reorder_modules,
            library::apply_layout,
            library::create_lesson,
            library::update_lesson,
            library::delete_lesson,
            library::set_lesson_completed,
            library::get_lesson_context,
            import::import_playlist,
            import::resync_course,
            import::add_video_to_module,
            player::save_position,
            player::get_progress,
            player::flush_complete,
            player::get_player_url,
            player::get_media_url,
            annotations::list_annotations,
            annotations::list_course_annotations,
            annotations::create_annotation,
            annotations::update_annotation,
            annotations::delete_annotation,
            annotations::get_notebook,
            annotations::save_notebook,
            transcripts::get_transcript,
            transcripts::get_chapters,
            transcripts::fetch_transcript,
            transcripts::fetch_course_transcripts,
            search::search_everything,
            search::export_course_markdown,
            downloads::list_downloads,
            downloads::enqueue_lesson_download,
            downloads::set_course_mode,
            downloads::set_lesson_mode_override,
            downloads::pause_download,
            downloads::resume_download,
            downloads::retry_download,
            downloads::cancel_download,
            downloads::delete_local_files,
            downloads::get_storage_usage,
            downloads::estimate_download_size,
            downloads::get_download_settings,
            downloads::set_download_settings,
            downloads::capture_frame,
            downloads::check_ytdlp_update,
            settings::get_app_info,
            settings::get_ytdlp_version,
            settings::set_api_key,
            settings::clear_api_key,
            settings::set_data_dir,
            settings::get_onboarded,
            settings::set_onboarded,
            settings::restart_app,
            settings::get_subtitle_langs,
            settings::set_subtitle_langs,
            study::get_study_settings,
            study::set_study_settings,
            study::record_watch,
            study::record_activity,
            study::get_today,
            study::get_activity,
            study::get_weekly_review,
            study::get_course_plan,
            study::set_course_plan,
            study::rebalance_plan,
            study::set_plan_order,
            study::get_planned,
            study::test_reminder,
            study::get_todos_between,
            youtube::search_youtube,
            youtube::get_quota,
            youtube::get_google_account,
            youtube::set_google_client,
            youtube::google_sign_in,
            youtube::google_sign_out,
            youtube::list_my_playlists,
            youtube::preview_playlist,
            flashcards::create_flashcard,
            flashcards::update_flashcard,
            flashcards::delete_flashcard,
            flashcards::list_flashcards,
            flashcards::due_flashcards,
            flashcards::review_flashcard,
            flashcards::list_annotations_by_type,
            ai::get_ai_status,
            ai::set_ai_settings,
            ai::set_ai_key,
            ai::estimate_restructure,
            ai::propose_restructure,
            ai::apply_course_structure,
            ai::undo_restructure,
            ai::can_undo_restructure,
            ai::get_ai_usage,
            ai::clear_ai_usage,
            backup::list_backups,
            backup::backup_now,
            backup::restore_backup,
            backup::export_library_json,
            backup::import_library_json,
            ai_assist::estimate_assist,
            ai_assist::run_assist,
            ai_assist::save_ai_answer,
            ai_assist::save_explanation,
            ai_assist::insert_lesson_summary,
            ai_assist::estimate_references,
            ai_assist::suggest_references,
            ai_assist::save_reference_drafts,
            ai_study::estimate_flashcards,
            ai_study::generate_flashcards,
            ai_study::save_flashcard_drafts,
            ai_study::estimate_quiz,
            ai_study::generate_quiz,
            ai_study::save_quiz_questions,
            ai_study::list_quiz,
            ai_study::record_quiz_answer,
            ai_study::delete_quiz_question,
            ai_study::estimate_course_plan,
            ai_study::propose_course_plan,
            ai_study::apply_course_plan,
            ocw::search_ocw,
            ocw::preview_ocw,
            ocw::import_ocw,
            providers::provider_search,
            providers::provider_preview,
            providers::provider_import,
            ocw::get_course_info,
            ocw::list_materials,
            ocw::list_lesson_materials,
            ocw::get_material,
            ocw::get_material_html,
            ocw::get_material_url,
            ocw::get_material_indexed_pages,
            ocw::set_material_pages,
        ])
}

pub const BINDINGS_PATH: &str = "../src/bindings.ts";

pub fn export_bindings(builder: &Builder<tauri::Wry>, path: &str) {
    builder
        .export(
            specta_typescript::Typescript::default().header("// @ts-nocheck\n/* eslint-disable */"),
            path,
        )
        .expect("failed to export typescript bindings");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    export_bindings(&builder, BINDINGS_PATH);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let config_dir = app.path().app_config_dir()?;
            let paths = paths::Paths::resolve(&config_dir)?;
            app.asset_protocol_scope()
                .allow_directory(&paths.data_dir, true)?;
            let loopback =
                tauri::async_runtime::block_on(services::player_server::start(paths.media_dir()))?;
            let state = state::AppState::new(paths, loopback)?;
            services::downloads::recover(&state);
            app.manage(state);
            let downloads = services::downloads::DownloadManager::new();
            downloads.start(app.handle().clone());
            app.manage(downloads);
            services::reminders::start(app.handle().clone());
            services::backup::start(app.handle().clone());
            // At most once a day: keep yt-dlp current (YouTube changes often break old versions).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let st = handle.state::<state::AppState>();
                services::downloads::backfill_quality(&st).await;
                match services::updater::check(&st, false).await {
                    Ok(r) if r.updated => log::info!("yt-dlp updated to {:?}", r.latest),
                    Ok(_) => {}
                    Err(e) => log::warn!("yt-dlp update check failed: {e}"),
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && shutdown::begin(window.app_handle())
            {
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event
                && shutdown::begin(app)
            {
                api.prevent_exit();
            }
        });
}

#[cfg(test)]
mod bindings_test {
    /// Keeps `src/bindings.ts` in sync without launching the app: `cargo test export_bindings`.
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder(), super::BINDINGS_PATH);
    }
}
