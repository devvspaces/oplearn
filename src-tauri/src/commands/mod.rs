//! Tauri commands, grouped by domain. Every command returns `AppResult` and is exported
//! to `src/bindings.ts` by tauri-specta.

pub mod ai;
pub mod ai_assist;
pub mod ai_study;
pub mod annotations;
pub mod backup;
pub mod downloads;
pub mod flashcards;
pub mod import;
pub mod library;
pub mod ocw;
pub mod player;
pub mod providers;
pub mod search;
pub mod settings;
pub mod study;
pub mod transcripts;
pub mod youtube;
